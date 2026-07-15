//! Phase-3 URL ingestion tests (audit Task 4).
//!
//! Hermetic: a tiny loopback HTTP server serves canned responses, and fetches
//! run with `use_env_proxy: false` so nothing leaves the machine.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use nucklavee::net::{FetchOptions, FetchedKind, fetch_with_options};

/// Serve exactly one HTTP/1.1 response, then close. Returns the base URL.
fn serve_once(status_line: &str, content_type: Option<&str>, body: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("addr");

    let mut response = format!("HTTP/1.1 {status_line}\r\n");
    if let Some(ct) = content_type {
        response.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    response.push_str("Connection: close\r\n\r\n");
    response.push_str(body);

    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf); // consume the request
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    format!("http://{addr}")
}

fn serve_raw_once(response: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("addr");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    format!("http://{addr}")
}

fn opts() -> FetchOptions {
    FetchOptions {
        use_env_proxy: false,
        allow_private_networks: true,
        timeout: Duration::from_secs(5),
        ..FetchOptions::default()
    }
}

#[test]
fn fetches_html_by_content_type() {
    let base = serve_once(
        "200 OK",
        Some("text/html; charset=utf-8"),
        "<html><body><h1>Hi</h1></body></html>",
    );
    let fetched = fetch_with_options(&format!("{base}/page"), &opts()).expect("fetch");
    assert_eq!(fetched.kind, FetchedKind::Html);
    assert!(fetched.body.contains("<h1>Hi</h1>"));
    assert!(fetched.final_url.starts_with("http://127.0.0.1:"));
}

#[test]
fn fetches_markdown_by_content_type() {
    let base = serve_once("200 OK", Some("text/markdown"), "# Title\n\nbody\n");
    let fetched = fetch_with_options(&format!("{base}/doc"), &opts()).expect("fetch");
    assert_eq!(fetched.kind, FetchedKind::Markdown);
    assert!(fetched.body.contains("# Title"));
}

#[test]
fn falls_back_to_extension_for_plain_text() {
    let base = serve_once("200 OK", Some("text/plain"), "# Md By Extension\n");
    let fetched = fetch_with_options(&format!("{base}/readme.md"), &opts()).expect("fetch");
    assert_eq!(fetched.kind, FetchedKind::Markdown);
}

#[test]
fn http_error_status_is_a_network_error() {
    let base = serve_once("404 Not Found", Some("text/html"), "nope");
    let err =
        fetch_with_options(&format!("{base}/missing"), &opts()).expect_err("404 must be an error");
    assert!(
        matches!(err, nucklavee::Error::Network(ref m) if m.contains("HTTP 404")),
        "unexpected error: {err}"
    );
}

#[test]
fn unreachable_host_is_a_network_error() {
    // Reserved TEST-NET-1 address that should not accept connections.
    let opts = FetchOptions {
        use_env_proxy: false,
        allow_private_networks: true,
        timeout: Duration::from_millis(600),
        ..FetchOptions::default()
    };
    let err = fetch_with_options("http://192.0.2.1:9/never", &opts)
        .expect_err("unreachable host must error");
    assert!(matches!(err, nucklavee::Error::Network(_)), "got {err}");
}

#[test]
fn private_networks_are_blocked_by_default() {
    let base = serve_once("200 OK", Some("text/plain"), "private");
    let err = fetch_with_options(&base, &FetchOptions::default())
        .expect_err("loopback must be blocked by default");
    assert!(err.to_string().contains("non-public destination"), "{err}");
}

#[test]
fn declared_oversized_response_is_rejected() {
    let base = serve_once("200 OK", Some("text/plain"), "123456");
    let err = fetch_with_options(
        &base,
        &FetchOptions {
            max_response_bytes: 5,
            ..opts()
        },
    )
    .expect_err("oversized response must fail");
    assert!(err.to_string().contains("5-byte limit"), "{err}");
}

#[test]
fn body_at_the_response_limit_succeeds() {
    let base = serve_once("200 OK", Some("text/plain"), "12345");
    let fetched = fetch_with_options(
        &base,
        &FetchOptions {
            max_response_bytes: 5,
            ..opts()
        },
    )
    .expect("exact limit is allowed");
    assert_eq!(fetched.body, "12345");
}

#[test]
fn lengthless_streamed_body_is_still_bounded() {
    let base = serve_raw_once(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n123456"
            .to_string(),
    );
    let err = fetch_with_options(
        &base,
        &FetchOptions {
            max_response_bytes: 5,
            ..opts()
        },
    )
    .expect_err("streamed body over limit must fail");
    assert!(err.to_string().contains("5-byte limit"), "{err}");
}

#[test]
fn library_ingests_url_with_explicit_trusted_local_policy() {
    use nucklavee::ir::Source;
    use nucklavee::storage::memory::InMemoryDocumentStore;
    use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
    use nucklavee::{Format, IngestOptions, Library};

    let base = serve_once(
        "200 OK",
        Some("text/html"),
        "<html><head><title>Fetched Page</title></head><body><main><h1>Fetched Page</h1><p>hello from <strong>the web</strong></p></main></body></html>",
    );
    let url = format!("{base}/article");
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex::default(),
        NoopEmbedder,
    )
    .expect("build library");

    let id = lib
        .ingest_with_fetch_options(Source::Url(url.clone()), IngestOptions::default(), &opts())
        .expect("trusted local URL ingest");
    let doc = lib.get_document(id).expect("stored doc");
    assert_eq!(doc.meta.source.raw_source, url);
    assert_eq!(doc.meta.title.as_deref(), Some("Fetched Page"));
    assert!(
        lib.emit(id, Format::Markdown)
            .expect("emit")
            .contains("hello from **the web**")
    );
}
