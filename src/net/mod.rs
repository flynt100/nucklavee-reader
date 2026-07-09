//! URL fetching for `Source::Url` ingestion (spec §4.2 URL path).
//!
//! A small blocking `reqwest` wrapper: fetch a URL, follow redirects, and
//! decide whether the response is HTML or Markdown from the `Content-Type`
//! header (falling back to the URL path extension, then a body sniff). The
//! caller feeds `Fetched.body` into the matching parser and preserves
//! `Fetched.final_url` in document provenance.
//!
//! Network access is isolated here so the rest of the crate stays offline and
//! deterministic. `FetchOptions.use_env_proxy` is honored in production and
//! disabled by tests so they can hit a loopback mock server directly.

use std::time::Duration;

use crate::{Error, Result};

/// Which parser a fetched document should go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchedKind {
    Html,
    Markdown,
}

/// A successfully fetched document.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// The URL after following redirects (what provenance should record).
    pub final_url: String,
    pub kind: FetchedKind,
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
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(opts.timeout)
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

    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let body = response
        .text()
        .map_err(|e| Error::Network(format!("reading body of '{url}' failed: {e}")))?;

    let kind = sniff_kind(content_type.as_deref(), &final_url, &body);
    Ok(Fetched {
        final_url,
        kind,
        body,
    })
}

/// Decide HTML vs Markdown from the content type, then the URL path
/// extension, then a leading-angle-bracket body sniff, defaulting to HTML
/// (the common case for URLs).
fn sniff_kind(content_type: Option<&str>, url: &str, body: &str) -> FetchedKind {
    if let Some(ct) = content_type {
        let ct = ct.to_ascii_lowercase();
        let essence = ct.split(';').next().unwrap_or("").trim();
        if essence.contains("markdown") {
            return FetchedKind::Markdown;
        }
        if essence.contains("html") || essence.contains("xml") {
            return FetchedKind::Html;
        }
        // text/plain and unknown types fall through to extension/body sniffing.
    }

    if let Some(ext) = url_path_extension(url) {
        match ext.as_str() {
            "md" | "markdown" | "mdown" | "mkd" => return FetchedKind::Markdown,
            "html" | "htm" | "xhtml" => return FetchedKind::Html,
            _ => {}
        }
    }

    let trimmed = body.trim_start();
    if trimmed.starts_with("<!doctype")
        || trimmed.starts_with("<!DOCTYPE")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<HTML")
    {
        return FetchedKind::Html;
    }

    FetchedKind::Html
}

/// Lowercased extension of a URL's path component, if any (ignores query and
/// fragment).
fn url_path_extension(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let path = after_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .split('/')
        .skip(1)
        .collect::<Vec<_>>()
        .join("/");
    let last = path.rsplit('/').next()?;
    let dot = last.rfind('.')?;
    let ext = &last[dot + 1..];
    if ext.is_empty() {
        None
    } else {
        Some(ext.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_wins_over_extension() {
        assert_eq!(
            sniff_kind(Some("text/markdown"), "http://e.com/page.html", ""),
            FetchedKind::Markdown
        );
        assert_eq!(
            sniff_kind(Some("text/html; charset=utf-8"), "http://e.com/x.md", ""),
            FetchedKind::Html
        );
    }

    #[test]
    fn extension_used_when_content_type_ambiguous() {
        assert_eq!(
            sniff_kind(Some("text/plain"), "http://e.com/readme.md", ""),
            FetchedKind::Markdown
        );
        assert_eq!(
            sniff_kind(None, "http://e.com/a/b/index.htm?x=1", ""),
            FetchedKind::Html
        );
    }

    #[test]
    fn body_sniff_and_default() {
        assert_eq!(
            sniff_kind(None, "http://e.com/resource", "<!doctype html><html>"),
            FetchedKind::Html
        );
        assert_eq!(
            sniff_kind(None, "http://e.com/resource", "plain words"),
            FetchedKind::Html
        );
    }

    #[test]
    fn url_extension_parsing() {
        assert_eq!(url_path_extension("http://e.com/a.md"), Some("md".into()));
        assert_eq!(
            url_path_extension("https://e.com/docs/page.HTML?q=1"),
            Some("html".into())
        );
        assert_eq!(url_path_extension("http://e.com/"), None);
        assert_eq!(url_path_extension("http://e.com/path/no-ext"), None);
    }
}
