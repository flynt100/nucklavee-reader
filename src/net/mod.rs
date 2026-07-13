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

use std::time::Duration;

use crate::{Error, Result};

/// Maximum characters of a remote response body quoted into an error message.
const DIAGNOSTIC_BODY_LIMIT: usize = 512;

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
    /// Sent as the `User-Agent` header.
    pub user_agent: String,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            use_env_proxy: true,
            timeout: Duration::from_secs(30),
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
    let client = build_blocking_client(opts.timeout, opts.use_env_proxy, Some(&opts.user_agent))
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

    let body = response
        .text()
        .map_err(|e| Error::Network(format!("reading body of '{url}' failed: {e}")))?;

    let (kind, detection_basis) =
        detect_format(content_type.as_deref(), extension.as_deref(), &body);
    Ok(Fetched {
        final_url,
        kind,
        detection_basis,
        body,
    })
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
}
