//! ApiEmbedder tests (audit Task 8): OpenAI-compatible request against a
//! hermetic loopback mock server, plus error and dimension handling.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use nucklavee::embedder::Embedder;
use nucklavee::embedder::api::{ApiEmbedder, ApiEmbedderConfig};

/// Serve one HTTP response, capturing the request body for assertions.
fn serve_once(
    status_line: &str,
    body: &str,
) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = std::sync::mpsc::channel();

    let mut response = format!("HTTP/1.1 {status_line}\r\n");
    response.push_str("Content-Type: application/json\r\n");
    response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    response.push_str("Connection: close\r\n\r\n");
    response.push_str(body);

    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = tx.send(request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    (format!("http://{addr}/v1/embeddings"), rx)
}

fn config(endpoint: String, dimension: usize) -> ApiEmbedderConfig {
    ApiEmbedderConfig {
        use_env_proxy: false,
        timeout: Duration::from_secs(5),
        ..ApiEmbedderConfig::new(endpoint, "test-model", dimension)
    }
}

#[test]
fn embeds_texts_and_preserves_order() {
    let body = r#"{"data":[
        {"embedding":[1.0,0.0,0.0],"index":0},
        {"embedding":[0.0,1.0,0.0],"index":1}
    ],"model":"test-model"}"#;
    let (endpoint, rx) = serve_once("200 OK", body);

    let embedder = ApiEmbedder::new(config(endpoint, 3)).expect("build embedder");
    let vectors = embedder.embed(&["hello", "world"]).expect("embed");

    assert_eq!(vectors.len(), 2);
    assert_eq!(vectors[0], vec![1.0, 0.0, 0.0]);
    assert_eq!(vectors[1], vec![0.0, 1.0, 0.0]);
    assert_eq!(embedder.dimension(), 3);

    // Request carried the model and both inputs.
    let request = rx.recv_timeout(Duration::from_secs(5)).expect("request captured");
    assert!(request.contains("test-model"), "request: {request}");
    assert!(request.contains("hello") && request.contains("world"), "request: {request}");
}

#[test]
fn out_of_order_response_is_sorted_by_index() {
    let body = r#"{"data":[
        {"embedding":[0.0,1.0],"index":1},
        {"embedding":[1.0,0.0],"index":0}
    ]}"#;
    let (endpoint, _rx) = serve_once("200 OK", body);

    let embedder = ApiEmbedder::new(config(endpoint, 2)).expect("embedder");
    let vectors = embedder.embed(&["a", "b"]).expect("embed");
    assert_eq!(vectors[0], vec![1.0, 0.0], "index 0 first");
    assert_eq!(vectors[1], vec![0.0, 1.0], "index 1 second");
}

#[test]
fn dimension_mismatch_is_an_error() {
    let body = r#"{"data":[{"embedding":[1.0,0.0,0.0],"index":0}]}"#;
    let (endpoint, _rx) = serve_once("200 OK", body);

    let embedder = ApiEmbedder::new(config(endpoint, 5)).expect("embedder");
    let err = embedder.embed(&["x"]).expect_err("dimension mismatch");
    assert!(matches!(err, nucklavee::Error::Embedding(_)), "got {err}");
}

#[test]
fn http_error_status_surfaces_as_embedding_error() {
    let (endpoint, _rx) = serve_once("500 Internal Server Error", "upstream boom");
    let embedder = ApiEmbedder::new(config(endpoint, 3)).expect("embedder");
    let err = embedder.embed(&["x"]).expect_err("500 should error");
    assert!(
        matches!(err, nucklavee::Error::Embedding(ref m) if m.contains("HTTP 500")),
        "got {err}"
    );
}

#[test]
fn empty_input_makes_no_request() {
    // Endpoint intentionally unreachable; empty input must short-circuit.
    let embedder = ApiEmbedder::new(config("http://192.0.2.1:9/v1/embeddings".into(), 3))
        .expect("embedder");
    let vectors = embedder.embed(&[]).expect("empty embed");
    assert!(vectors.is_empty());
}
