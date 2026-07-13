//! CLI smoke tests (audit Task 10): every subcommand end to end against a
//! temp SQLite db + usearch index + a hermetic loopback mock embedding server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::thread;

use uuid::Uuid;

const DIM: usize = 8;

/// A loopback OpenAI-compatible embedding server that returns one fixed unit
/// vector per input (so every chunk and every query embed identically —
/// retrieval is guaranteed, which is all a smoke test needs). Runs until the
/// test process exits.
fn spawn_embedding_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = handle(stream);
        }
    });
    format!("http://{addr}/v1/embeddings")
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;

    let n = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("input").and_then(|i| i.as_array()).map(|a| a.len()))
        .unwrap_or(1);

    let mut embedding = vec![0.0f32; DIM];
    embedding[0] = 1.0;
    let data: Vec<serde_json::Value> = (0..n)
        .map(|i| serde_json::json!({ "embedding": embedding, "index": i }))
        .collect();
    let payload = serde_json::json!({ "data": data }).to_string();

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

struct Fixture {
    dir: PathBuf,
    config: PathBuf,
    doc: PathBuf,
    index: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let endpoint = spawn_embedding_server();
        let dir = std::env::temp_dir().join(format!("nucklavee_cli_{}_{}", std::process::id(), Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");

        let db = dir.join("library.sqlite");
        let index = dir.join("library.usearch");
        let doc = dir.join("guide.md");
        std::fs::write(
            &doc,
            "# Field Guide\n\n## Apples\n\nApples are a crunchy red fruit.\n\n## Oceans\n\nThe ocean is salt water.\n",
        )
        .expect("write doc");

        let config = dir.join("config.toml");
        std::fs::write(
            &config,
            format!(
                "[storage]\ndatabase = \"{}\"\nvector_index = \"{}\"\n\n[embedding]\nendpoint = \"{}\"\nmodel = \"test-model\"\ndimension = {}\nuse_env_proxy = false\n",
                db.display(),
                index.display(),
                endpoint,
                DIM,
            ),
        )
        .expect("write config");

        Self {
            dir,
            config,
            doc,
            index,
        }
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_nucklavee"))
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .output()
            .expect("run cli");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn full_lifecycle_ingest_list_info_search_context_emit_remove() {
    let fx = Fixture::new();
    let doc = fx.doc.to_string_lossy().to_string();

    // ingest → prints a UUID
    let (code, out, err) = fx.run(&["ingest", &doc]);
    assert_eq!(code, 0, "ingest failed: {err}");
    let id = out.trim().to_string();
    assert!(Uuid::parse_str(&id).is_ok(), "ingest should print a UUID, got: {out:?}");

    // list → shows the title
    let (code, out, _) = fx.run(&["list"]);
    assert_eq!(code, 0);
    assert!(out.contains(&id) && out.contains("Field Guide"), "list output: {out}");

    // info → metadata + chunk count
    let (code, out, err) = fx.run(&["info", &id]);
    assert_eq!(code, 0, "info failed: {err}");
    assert!(out.contains("Field Guide"), "info: {out}");
    assert!(out.contains("chunks:"), "info should report chunk count: {out}");

    // search → retrieves chunks with provenance
    let (code, out, err) = fx.run(&["search", "crunchy red apples", "--limit", "5"]);
    assert_eq!(code, 0, "search failed: {err}");
    assert!(out.contains("Apples") || out.contains("Oceans"), "search should return results: {out}");

    // context → provenance-headed window
    let (code, out, err) = fx.run(&["context", "salt water ocean", "--budget", "500"]);
    assert_eq!(code, 0, "context failed: {err}");
    assert!(out.contains("[Source: Field Guide"), "context header missing: {out}");

    // emit → markdown
    let (code, out, err) = fx.run(&["emit", &id, "markdown"]);
    assert_eq!(code, 0, "emit failed: {err}");
    assert!(out.contains("# Field Guide"), "emit markdown: {out}");

    // emit → html and text also work
    assert!(fx.run(&["emit", &id, "html"]).1.contains("<h1>Field Guide</h1>"));
    assert!(fx.run(&["emit", &id, "text"]).1.contains("FIELD GUIDE"));

    // remove → gone from the listing
    let (code, out, err) = fx.run(&["remove", &id]);
    assert_eq!(code, 0, "remove failed: {err}");
    assert!(out.contains("removed"), "remove output: {out}");

    let (_, out, _) = fx.run(&["list"]);
    assert!(!out.contains(&id), "document should be gone after remove: {out}");
}

#[test]
fn json_output_is_machine_readable() {
    let fx = Fixture::new();
    let doc = fx.doc.to_string_lossy().to_string();

    let (code, out, err) = fx.run(&["--json", "ingest", &doc]);
    assert_eq!(code, 0, "ingest failed: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("ingest --json is valid JSON");
    let id = v["document_id"].as_str().expect("document_id").to_string();

    let (_, out, _) = fx.run(&["--json", "list"]);
    let list: serde_json::Value = serde_json::from_str(&out).expect("list --json");
    assert!(list.as_array().map(|a| !a.is_empty()).unwrap_or(false), "list json: {out}");

    let (_, out, _) = fx.run(&["--json", "info", &id]);
    let info: serde_json::Value = serde_json::from_str(&out).expect("info --json");
    assert_eq!(info["id"].as_str(), Some(id.as_str()));
    assert!(info["chunk_count"].as_u64().unwrap_or(0) >= 1);

    let (_, out, _) = fx.run(&["--json", "search", "apples", "--limit", "3"]);
    let _results: serde_json::Value = serde_json::from_str(&out).expect("search --json");
}

#[test]
fn unknown_emit_format_is_rejected() {
    let fx = Fixture::new();
    let doc = fx.doc.to_string_lossy().to_string();
    let (_, out, _) = fx.run(&["ingest", &doc]);
    let id = out.trim();

    let (code, _, err) = fx.run(&["emit", id, "rtf"]);
    assert_eq!(code, 2, "clap value_parser error should exit 2");
    assert!(
        err.contains("unsupported format 'rtf'. supported: markdown, html, text"),
        "stderr: {err}"
    );
}

#[test]
fn rebuild_index_recovers_from_a_corrupt_index_file() {
    let fx = Fixture::new();
    let doc = fx.doc.to_string_lossy().to_string();

    let (code, _, err) = fx.run(&["ingest", &doc]);
    assert_eq!(code, 0, "ingest failed: {err}");

    // Corrupt the index manifest. Ordinary commands must now fail with a
    // pointer at the recovery command rather than an opaque load error.
    std::fs::write(&fx.index, b"\x00corrupted\xff").expect("corrupt index");
    let (code, _, err) = fx.run(&["search", "apples", "--limit", "3"]);
    assert_eq!(code, 1, "search against a corrupt index should fail");
    assert!(
        err.contains("rebuild-index"),
        "failure must point at the recovery path: {err}"
    );

    // The recovery command itself must NOT try to load the corrupt index —
    // it starts empty and repopulates from the store.
    let (code, out, err) = fx.run(&["rebuild-index"]);
    assert_eq!(code, 0, "rebuild-index must work with a corrupt index: {err}");
    assert!(out.contains("rebuilt index"), "rebuild output: {out}");

    // Search works again, against the rebuilt index.
    let (code, out, err) = fx.run(&["search", "crunchy red apples", "--limit", "3"]);
    assert_eq!(code, 0, "search after rebuild failed: {err}");
    assert!(
        out.contains("Apples") || out.contains("Oceans"),
        "search should return results after rebuild: {out}"
    );
}

#[test]
fn rebuild_index_works_when_index_files_are_missing_entirely() {
    let fx = Fixture::new();
    let doc = fx.doc.to_string_lossy().to_string();

    let (code, _, err) = fx.run(&["ingest", &doc]);
    assert_eq!(code, 0, "ingest failed: {err}");

    // Delete every index artifact (manifest + generation files).
    for entry in std::fs::read_dir(&fx.dir).expect("read dir").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("library.usearch") {
            std::fs::remove_file(entry.path()).expect("remove index artifact");
        }
    }

    let (code, out, err) = fx.run(&["--json", "rebuild-index"]);
    assert_eq!(code, 0, "rebuild-index with no index files failed: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("rebuild --json");
    assert!(
        v["vectors_indexed"].as_u64().unwrap_or(0) >= 1,
        "rebuild should re-index stored embeddings: {out}"
    );

    let (code, out, err) = fx.run(&["search", "salt water ocean", "--limit", "3"]);
    assert_eq!(code, 0, "search after rebuild failed: {err}");
    assert!(!out.contains("(no results)"), "results expected: {out}");
}

#[test]
fn missing_config_is_a_clear_error() {
    // No --config, and HOME pointed somewhere without a config file.
    let empty_home = std::env::temp_dir().join(format!("nucklavee_nohome_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&empty_home).expect("mkdir");
    let output = Command::new(env!("CARGO_BIN_EXE_nucklavee"))
        .args(["list"])
        .env("HOME", &empty_home)
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("could not read config"), "stderr: {err}");
    let _ = std::fs::remove_dir_all(&empty_home);
}
