//! HTTP mechanics and URL fetching for `Source::Url` ingestion (spec §4.2).
//!
//! Two responsibilities live here:
//!
//! 1. **Shared client mechanics** used by both URL fetching and the embedding
//!    client ([`build_blocking_client`], [`truncate_for_diagnostics`]) —
//!    timeout, proxy policy, and bounded error bodies are policy decisions
//!    that must not drift between the two HTTP call sites. Domain-specific
//!    parsing stays in each caller.
//! 2. **Fetch + format detection**: fetch a URL, follow redirects, and decide
//!    whether the response is HTML or Markdown by a layered policy —
//!    supported media type, then URL path extension (via the parsed URL, not
//!    string splitting), then strong HTML body signals, then strong Markdown
//!    body signals, then an HTML fallback. Detection evidence is recorded on
//!    the result for diagnostics.
//!
//! Network access is isolated here so the rest of the crate stays offline and
//! deterministic. `use_env_proxy` is honored in production and disabled by
//! tests so they can hit a loopback mock server directly.

use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

use crate::{Error, Result};

/// Maximum characters of a remote response body quoted into an error message.
const DIAGNOSTIC_BODY_LIMIT: usize = 512;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_MAX_REDIRECTS: usize = 10;

/// Truncate a remote response body for safe inclusion in an error message —
/// no provider may inject an arbitrarily large payload into diagnostics.
pub(crate) fn truncate_for_diagnostics(body: &str) -> String {
    if body.chars().count() <= DIAGNOSTIC_BODY_LIMIT {
        return body.to_string();
    }
    let truncated: String = body.chars().take(DIAGNOSTIC_BODY_LIMIT).collect();
    format!("{truncated}… (truncated)")
}

/// Build a blocking HTTP client with the crate's shared policy (timeout,
/// proxy honoring, optional user agent). Callers map the error into their
/// own domain variant.
pub(crate) fn build_blocking_client(
    timeout: Duration,
    use_env_proxy: bool,
    user_agent: Option<&str>,
) -> std::result::Result<reqwest::blocking::Client, reqwest::Error> {
    let mut builder = reqwest::blocking::Client::builder().timeout(timeout);
    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua.to_string());
    }
    if !use_env_proxy {
        builder = builder.no_proxy();
    }
    builder.build()
}

/// Which parser a fetched document should go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchedKind {
    Html,
    Markdown,
}

/// What the format decision was based on, most reliable first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionBasis {
    MediaType,
    UrlExtension,
    HtmlBodySignal,
    MarkdownBodySignal,
    Fallback,
}

/// A successfully fetched document.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// The URL after following redirects (what provenance should record).
    pub final_url: String,
    pub kind: FetchedKind,
    /// Evidence for the `kind` decision, for diagnostics on misclassification.
    pub detection_basis: DetectionBasis,
    pub body: String,
}

/// Fetch behavior. Defaults are production-appropriate; tests disable the
/// proxy to reach a loopback server.
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// Honor `HTTP(S)_PROXY`/`NO_PROXY` environment variables (default true).
    pub use_env_proxy: bool,
    pub timeout: Duration,
    /// Maximum response body size. Both declared and streamed sizes are checked.
    pub max_response_bytes: usize,
    /// Permit loopback/private/link-local destinations. Intended for trusted
    /// local integrations and hermetic tests; disabled by default.
    pub allow_private_networks: bool,
    /// Maximum redirects followed. Every redirect destination is revalidated.
    pub max_redirects: usize,
    /// Sent as the `User-Agent` header.
    pub user_agent: String,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            use_env_proxy: true,
            timeout: Duration::from_secs(30),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            allow_private_networks: false,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            user_agent: concat!("nucklavee/", env!("CARGO_PKG_VERSION")).to_string(),
        }
    }
}

/// Fetch a URL with default options (production path).
pub fn fetch(url: &str) -> Result<Fetched> {
    fetch_with_options(url, &FetchOptions::default())
}

/// Fetch a URL with explicit options. Used by tests with
/// `use_env_proxy: false` against a local mock server.
pub fn fetch_with_options(url: &str, opts: &FetchOptions) -> Result<Fetched> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| Error::Network(format!("invalid URL '{url}': {e}")))?;
    validate_destination(&parsed, opts.allow_private_networks)?;

    let allow_private = opts.allow_private_networks;
    let max_redirects = opts.max_redirects;
    let redirect_policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= max_redirects {
            return attempt.error("redirect limit exceeded");
        }
        match validate_destination(attempt.url(), allow_private) {
            Ok(()) => attempt.follow(),
            Err(error) => attempt.error(error.to_string()),
        }
    });
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(opts.timeout)
        .redirect(redirect_policy)
        .user_agent(opts.user_agent.clone());
    if !opts.use_env_proxy {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|e| Error::Network(format!("failed to build HTTP client: {e}")))?;

    let response = client
        .get(url)
        .send()
        .map_err(|e| Error::Network(format!("request to '{url}' failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(Error::Network(format!(
            "fetching '{url}' returned HTTP {}",
            status.as_u16()
        )));
    }

    if response
        .content_length()
        .is_some_and(|length| length > opts.max_response_bytes as u64)
    {
        return Err(response_too_large(url, opts.max_response_bytes));
    }

    // Take the extension from the parsed URL's path (encoded paths, ports,
    // and query/fragment placement are the URL library's problem, not ours).
    let final_url = response.url().to_string();
    let extension = std::path::Path::new(response.url().path())
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or(0)
            .min(opts.max_response_bytes as u64) as usize,
    );
    response
        .take(opts.max_response_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Network(format!("reading body of '{url}' failed: {e}")))?;
    if bytes.len() > opts.max_response_bytes {
        return Err(response_too_large(url, opts.max_response_bytes));
    }
    let body = decode_body(&bytes, content_type.as_deref());

    let (kind, detection_basis) =
        detect_format(content_type.as_deref(), extension.as_deref(), &body);
    Ok(Fetched {
        final_url,
        kind,
        detection_basis,
        body,
    })
}

fn response_too_large(url: &str, limit: usize) -> Error {
    Error::Network(format!(
        "response from '{url}' exceeded the {limit}-byte limit"
    ))
}

fn decode_body(bytes: &[u8], content_type: Option<&str>) -> String {
    let charset = content_type.and_then(|value| {
        value.split(';').skip(1).find_map(|parameter| {
            let (name, value) = parameter.trim().split_once('=')?;
            name.eq_ignore_ascii_case("charset")
                .then(|| value.trim().trim_matches(['"', '\'']))
        })
    });
    let encoding = charset
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (decoded, _, _) = encoding.decode(bytes);
    decoded.into_owned()
}

fn validate_destination(url: &reqwest::Url, allow_private_networks: bool) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::Network(format!(
            "unsupported URL scheme '{}'; expected http or https",
            url.scheme()
        )));
    }
    if allow_private_networks {
        return Ok(());
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::Network("URL has no host".to_string()))?;
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return Err(blocked_destination(host));
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Error::Network("URL has no usable port".to_string()))?;
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| Error::Network(format!("failed to resolve '{host}': {e}")))?
        .collect();
    if addresses.is_empty() {
        return Err(Error::Network(format!("'{host}' resolved to no addresses")));
    }
    if addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(blocked_destination(host));
    }
    Ok(())
}

fn blocked_destination(host: &str) -> Error {
    Error::Network(format!(
        "refusing private, loopback, link-local, or otherwise non-public destination '{host}'"
    ))
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            let shared = octets[0] == 100 && (64..=127).contains(&octets[1]);
            let benchmarking = octets[0] == 198 && matches!(octets[1], 18 | 19);
            let reserved = octets[0] == 0 || octets[0] >= 240;
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || shared
                || benchmarking
                || reserved)
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }

            let octets = ip.octets();
            let segments = ip.segments();
            // Fail closed around the current IANA global-unicast allocation
            // registry. These aggregates cover only ALLOCATED entries; holes
            // and RESERVED ranges remain blocked until the table is reviewed.
            let allocated_public_unicast = match segments[0] {
                0x2001 => matches!(
                    segments[1],
                    0x0000..=0x0fff
                        | 0x1200..=0x13ff
                        | 0x1400..=0x17ff
                        | 0x1800..=0x1fff
                        | 0x2000..=0x3fff
                        | 0x4000..=0x47ff
                        | 0x4800..=0x4bff
                        | 0x4c00..=0x4dff
                        | 0x5000..=0x5fff
                        | 0x8000..=0x9fff
                        | 0xa000..=0xbfff
                ),
                0x2002 => true,
                0x2003 => segments[1] <= 0x3fff,
                0x2400..=0x241f
                | 0x2600..=0x260f
                | 0x2630..=0x263f
                | 0x2800..=0x280f
                | 0x2a00..=0x2a1f
                | 0x2c00..=0x2c0f => true,
                0x2610 | 0x2620 => segments[1] <= 0x01ff,
                _ => false,
            };

            // IANA marks the 2001::/23 parent as non-global, with a small set
            // of explicitly globally reachable children. Preserve those exact
            // exceptions instead of either admitting the whole parent or
            // blocking legitimate anycast/protocol destinations.
            let ietf_protocol_assignments =
                octets[0] == 0x20 && octets[1] == 0x01 && octets[2] <= 0x01;
            let protocol_anycast = octets[..4] == [0x20, 0x01, 0x00, 0x01]
                && octets[4..15].iter().all(|byte| *byte == 0)
                && matches!(octets[15], 1..=3);
            let amt = octets[..4] == [0x20, 0x01, 0x00, 0x03];
            let as112 = octets[..6] == [0x20, 0x01, 0x00, 0x04, 0x01, 0x12];
            let orchid_v2 = octets[..3] == [0x20, 0x01, 0x00] && octets[3] & 0xf0 == 0x20;
            let dets = octets[..3] == [0x20, 0x01, 0x00] && octets[3] & 0xf0 == 0x30;
            let globally_reachable_ietf = protocol_anycast || amt || as112 || orchid_v2 || dets;

            let documentation = octets[..4] == [0x20, 0x01, 0x0d, 0xb8];
            let six_to_four = octets[..2] == [0x20, 0x02];

            allocated_public_unicast
                && (!ietf_protocol_assignments || globally_reachable_ietf)
                && !documentation
                && !six_to_four
        }
    }
}

/// Layered format detection: explicit media type → URL extension → strong
/// HTML body signals → strong Markdown body signals → HTML fallback.
fn detect_format(
    content_type: Option<&str>,
    extension: Option<&str>,
    body: &str,
) -> (FetchedKind, DetectionBasis) {
    if let Some(ct) = content_type {
        let ct = ct.to_ascii_lowercase();
        let essence = ct.split(';').next().unwrap_or("").trim();
        if essence.contains("markdown") {
            return (FetchedKind::Markdown, DetectionBasis::MediaType);
        }
        if essence.contains("html") || essence.contains("xml") {
            return (FetchedKind::Html, DetectionBasis::MediaType);
        }
        // text/plain and unknown types fall through to weaker evidence.
    }

    match extension {
        Some("md" | "markdown" | "mdown" | "mkd") => {
            return (FetchedKind::Markdown, DetectionBasis::UrlExtension);
        }
        Some("html" | "htm" | "xhtml") => {
            return (FetchedKind::Html, DetectionBasis::UrlExtension);
        }
        _ => {}
    }

    if looks_like_html(body) {
        return (FetchedKind::Html, DetectionBasis::HtmlBodySignal);
    }
    if looks_like_markdown(body) {
        return (FetchedKind::Markdown, DetectionBasis::MarkdownBodySignal);
    }

    (FetchedKind::Html, DetectionBasis::Fallback)
}

fn looks_like_html(body: &str) -> bool {
    let trimmed = body.trim_start();
    let lowered: String = trimmed.chars().take(64).collect::<String>().to_lowercase();
    lowered.starts_with("<!doctype") || lowered.starts_with("<html") || lowered.starts_with("<?xml")
}

/// Strong markdown evidence: an ATX heading or fenced code block alone is
/// decisive; otherwise require at least two weaker signals so a single stray
/// token cannot misclassify plain text.
fn looks_like_markdown(body: &str) -> bool {
    let mut strong = 0usize;
    let mut weak = 0usize;
    let mut list_lines = 0usize;

    for (i, line) in body.lines().take(200).enumerate() {
        let trimmed = line.trim_start();

        // ATX heading: `#`–`######` followed by a space.
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level_rest = rest.trim_start_matches('#');
            if trimmed.len() - level_rest.len() <= 6 && level_rest.starts_with(' ') {
                strong += 1;
            }
        }
        // Fenced code block.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            strong += 1;
        }
        // Frontmatter fence on the very first line.
        if i == 0 && line.trim_end() == "---" {
            weak += 1;
        }
        // Table delimiter row.
        if trimmed.starts_with('|') && trimmed.contains("---") {
            weak += 1;
        }
        // Markdown link `[text](url)`.
        if trimmed.contains("](") && trimmed.contains('[') {
            weak += 1;
        }
        // List markers across multiple lines.
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            list_lines += 1;
        }
    }
    if list_lines >= 2 {
        weak += 1;
    }

    strong >= 1 || weak >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(ct: Option<&str>, ext: Option<&str>, body: &str) -> FetchedKind {
        detect_format(ct, ext, body).0
    }

    #[test]
    fn content_type_wins_over_extension() {
        assert_eq!(
            kind(Some("text/markdown"), Some("html"), ""),
            FetchedKind::Markdown
        );
        assert_eq!(
            kind(Some("text/html; charset=utf-8"), Some("md"), ""),
            FetchedKind::Html
        );
    }

    #[test]
    fn extension_used_when_content_type_ambiguous() {
        assert_eq!(
            kind(Some("text/plain"), Some("md"), ""),
            FetchedKind::Markdown
        );
        assert_eq!(kind(None, Some("htm"), ""), FetchedKind::Html);
    }

    #[test]
    fn extensionless_markdown_detected_from_body() {
        let md = "# Title\n\nSome text with a [link](https://e.com).\n\n- one\n- two\n";
        let (k, basis) = detect_format(Some("text/plain"), None, md);
        assert_eq!(k, FetchedKind::Markdown);
        assert_eq!(basis, DetectionBasis::MarkdownBodySignal);
    }

    #[test]
    fn extensionless_html_detected_from_body() {
        let (k, basis) = detect_format(None, None, "  <!DOCTYPE html><html><body>x</body>");
        assert_eq!(k, FetchedKind::Html);
        assert_eq!(basis, DetectionBasis::HtmlBodySignal);
    }

    #[test]
    fn one_weak_markdown_token_is_not_enough() {
        // A single dash list line in otherwise plain text must not classify
        // as markdown.
        let text = "Meeting notes\n- follow up later\nRegards,\nTeam";
        let (k, basis) = detect_format(None, None, text);
        assert_eq!(k, FetchedKind::Html);
        assert_eq!(basis, DetectionBasis::Fallback);
    }

    #[test]
    fn weak_signals_combine() {
        let md = "---\ntitle: x\n---\n\nIntro with [ref](https://e.com).\n";
        assert_eq!(kind(None, None, md), FetchedKind::Markdown);
    }

    #[test]
    fn plain_ambiguous_text_falls_back_to_html() {
        assert_eq!(kind(None, None, "plain words"), FetchedKind::Html);
    }

    #[test]
    fn diagnostics_truncation_is_bounded() {
        let huge = "x".repeat(10_000);
        let out = truncate_for_diagnostics(&huge);
        assert!(out.chars().count() < 600);
        assert!(out.ends_with("(truncated)"));
        assert_eq!(truncate_for_diagnostics("short"), "short");
    }

    #[test]
    fn special_use_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.1.1",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "100::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().expect("valid IP")), "{ip}");
        }
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn ipv6_public_destination_policy_matches_iana_boundaries() {
        for ip in [
            "64:ff9b::c000:201",
            "64:ff9b:1::1",
            "2000::1",
            "2001::1",
            "2001:1::",
            "2001:1::4",
            "2001:2::1",
            "2001:4::1",
            "2001:4:111:ffff::1",
            "2001:4:113::1",
            "2001:5::1",
            "2001:10::1",
            "2001:1f:ffff::1",
            "2001:40::1",
            "2001:1000::1",
            "2001:4e00::1",
            "2002:c0a8:101::1",
            "2003:4000::1",
            "2004::1",
            "2420::1",
            "2610:200::1",
            "2620:200::1",
            "2640::1",
            "2810::1",
            "2a20::1",
            "2c10::1",
            "2d00::1",
            "2e00::1",
            "2fff::1",
            "3000::1",
            "3ffe::1",
            "3fff::1",
            "5f00::1",
        ] {
            assert!(!is_public_ip(ip.parse().expect("valid IP")), "{ip}");
        }

        for ip in [
            "2001:1::1",
            "2001:1::2",
            "2001:1::3",
            "2001:3::1",
            "2001:4:112::1",
            "2001:4:112:ffff:ffff:ffff:ffff:ffff",
            "2001:20::1",
            "2001:2f:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:30::1",
            "2001:3f:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:1200::1",
            "2001:13ff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:1400::1",
            "2001:17ff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:1800::1",
            "2001:1fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:2000::1",
            "2001:3fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:4000::1",
            "2001:47ff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:4800::1",
            "2001:4bff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:4c00::1",
            "2001:4dff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:5000::1",
            "2001:5fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:8000::1",
            "2001:9fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:a000::1",
            "2001:bfff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:4860:4860::8888",
            "2003:3fff::1",
            "2400::1",
            "241f::1",
            "2600::1",
            "260f::1",
            "2610:1ff::1",
            "2620:1ff::1",
            "2606:4700:4700::1111",
            "2620:4f:8000::1",
            "2630::1",
            "263f::1",
            "2800::1",
            "280f::1",
            "2a00:1450:4009:80b::200e",
            "2a1f::1",
            "2c00::1",
            "2c0f::1",
        ] {
            assert!(is_public_ip(ip.parse().expect("valid IP")), "{ip}");
        }
    }

    #[test]
    fn redirect_destination_validator_blocks_private_url() {
        let url = reqwest::Url::parse("http://127.0.0.1/redirect-target").unwrap();
        let error = validate_destination(&url, false).expect_err("must block redirect target");
        assert!(error.to_string().contains("non-public destination"));
    }

    #[test]
    fn declared_charset_is_preserved_after_bounded_read() {
        assert_eq!(
            decode_body(
                &[0x63, 0x61, 0x66, 0xe9],
                Some("text/plain; charset=windows-1252")
            ),
            "café"
        );
    }
}
