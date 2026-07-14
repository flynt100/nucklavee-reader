//! Nucklavee CLI (spec §8): ingest / search / emit / list / info / context /
//! remove over a persistent SQLite + usearch library configured by a TOML
//! file. Pass `--json` for machine-readable output.

mod config;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;

use config::Config;
use nucklavee::embedder::api::ApiEmbedder;
use nucklavee::ir::{DocumentId, Source};
use nucklavee::storage::DocumentStore;
use nucklavee::storage::sqlite::SqliteDocumentStore;
use nucklavee::vector::VectorIndex;
use nucklavee::vector::usearch::UsearchIndex;
use nucklavee::{Error, Format, IngestOptions, Library, Result, contract};

type Lib = Library<SqliteDocumentStore, UsearchIndex, ApiEmbedder>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(cli.config.as_deref())?;
    let json = cli.json;

    // `rebuild-index` is the recovery path for an index that cannot load, so
    // it must never itself load the existing index files: it starts from an
    // empty index and repopulates it from the authoritative store.
    if matches!(cli.command, Command::RebuildIndex) {
        let mut lib = build_library(&cfg, false)?;
        return cmd_rebuild_index(&mut lib, &cfg, json);
    }

    let mut lib = build_library(&cfg, true)?;
    match cli.command {
        Command::Ingest {
            path,
            normalize_bare_callouts,
        } => cmd_ingest(&mut lib, &cfg, &path, normalize_bare_callouts, json),
        Command::Search { query, limit } => cmd_search(&lib, &query, limit, json),
        Command::Emit { id, format } => cmd_emit(&lib, id, format),
        Command::List => cmd_list(&lib, json),
        Command::Info { id } => cmd_info(&lib, id, json),
        Command::Context { query, budget } => cmd_context(&lib, &query, budget),
        Command::Remove { id } => cmd_remove(&mut lib, &cfg, id),
        Command::RebuildIndex => unreachable!("handled before the index is loaded"),
    }
}

fn build_library(cfg: &Config, load_existing_index: bool) -> Result<Lib> {
    // Construction order matters: the embedder is built first because its
    // embedding space configures the index; the persisted index is then
    // loaded against that expected space (never adopting whatever is on
    // disk), and `Library::new` finally cross-checks the store's binding.
    let embedder = ApiEmbedder::new(cfg.embedder_config())?;
    let mut index = UsearchIndex::for_embedder(&embedder)?;
    if load_existing_index && cfg.storage.vector_index.exists() {
        index.load(&cfg.storage.vector_index).map_err(|e| match e {
            // A wrong-space index is not corruption; `rebuild-index` cannot
            // convert models, so don't point at it.
            Error::EmbeddingSpaceMismatch(_) => e,
            other => Error::VectorIndex(format!(
                "vector index at '{}' failed to load: {other}; run `nucklavee \
                 rebuild-index` to rebuild it from the document store",
                cfg.storage.vector_index.display()
            )),
        })?;
    }
    let store = SqliteDocumentStore::open(&cfg.storage.database)?;
    Library::new(store, index, embedder)
}

#[derive(Debug, Parser)]
#[command(name = "nucklavee")]
#[command(about = "Universal document ingestion, conversion, and semantic search")]
struct Cli {
    /// Path to config TOML (default: ~/.config/forge/config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Emit machine-readable JSON where applicable.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Ingest a local file (.md/.html/.htm) or an http(s):// URL.
    Ingest {
        path: String,
        /// Normalize bare callouts like `[!tip]` into blockquotes (markdown).
        #[arg(long)]
        normalize_bare_callouts: bool,
    },
    /// Semantic search; print ranked chunks with provenance.
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Emit a stored document as markdown | html | text.
    Emit {
        id: DocumentId,
        #[arg(value_parser = parse_format_arg)]
        format: Format,
    },
    /// List all ingested documents with IDs and titles.
    List,
    /// Show a document's metadata and chunk count.
    Info { id: DocumentId },
    /// Assemble and print a context window for a query.
    Context {
        query: String,
        #[arg(long, default_value_t = 2048)]
        budget: usize,
    },
    /// Remove a document and its chunks from the library.
    Remove { id: DocumentId },
    /// Rebuild the vector index from stored embeddings (no re-embedding).
    /// Use after index corruption or loss — the store is authoritative.
    RebuildIndex,
}

// --- command handlers -------------------------------------------------------

fn cmd_ingest(
    lib: &mut Lib,
    cfg: &Config,
    path: &str,
    normalize_bare_callouts: bool,
    json: bool,
) -> Result<()> {
    let source = if is_url(path) {
        Source::Url(path.to_string())
    } else {
        Source::File(path.into())
    };
    let id = lib.ingest_with_options(
        source,
        IngestOptions {
            normalize_bare_callouts,
            ..Default::default()
        },
    )?;
    lib.save_index(&cfg.storage.vector_index)?;

    if json {
        print_json(&serde_json::json!({ "document_id": id.to_string() }))?;
    } else {
        println!("{id}");
    }
    Ok(())
}

fn cmd_search(lib: &Lib, query: &str, limit: usize, json: bool) -> Result<()> {
    let chunks = lib.query(query, limit)?;
    if json {
        print_json(&chunks)?;
        return Ok(());
    }
    if chunks.is_empty() {
        println!("(no results)");
    }
    for chunk in &chunks {
        let path = if chunk.section_path.is_empty() {
            "(root)".to_string()
        } else {
            chunk.section_path.join(" > ")
        };
        println!("[{path}] {}", snippet(&chunk.content));
    }
    Ok(())
}

fn cmd_emit(lib: &Lib, id: DocumentId, format: Format) -> Result<()> {
    let output = lib.emit(id, format)?;
    println!("{output}");
    Ok(())
}

fn cmd_list(lib: &Lib, json: bool) -> Result<()> {
    let docs = lib.store().list_documents()?;
    if json {
        print_json(&docs)?;
        return Ok(());
    }
    if docs.is_empty() {
        println!("(no documents)");
    }
    for meta in &docs {
        println!(
            "{}\t{}",
            meta.id,
            meta.title.as_deref().unwrap_or("(untitled)")
        );
    }
    Ok(())
}

fn cmd_info(lib: &Lib, id: DocumentId, json: bool) -> Result<()> {
    let doc = lib.get_document(id)?;
    let chunk_count = lib.store().get_chunks_by_document(id)?.len();

    if json {
        print_json(&serde_json::json!({
            "id": doc.meta.id.to_string(),
            "title": doc.meta.title,
            "source": doc.meta.source.raw_source,
            "format": format!("{:?}", doc.meta.format),
            "ingested_at": doc.meta.ingested_at.to_rfc3339(),
            "content_hash": doc.meta.content_hash,
            "chunk_count": chunk_count,
        }))?;
        return Ok(());
    }
    println!("id:          {}", doc.meta.id);
    println!(
        "title:       {}",
        doc.meta.title.as_deref().unwrap_or("(untitled)")
    );
    println!("source:      {}", doc.meta.source.raw_source);
    println!("format:      {:?}", doc.meta.format);
    println!("ingested_at: {}", doc.meta.ingested_at.to_rfc3339());
    println!("chunks:      {chunk_count}");
    Ok(())
}

fn cmd_context(lib: &Lib, query: &str, budget: usize) -> Result<()> {
    let ctx = lib.context_window(query, budget)?;
    print!("{ctx}");
    if !ctx.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn cmd_remove(lib: &mut Lib, cfg: &Config, id: DocumentId) -> Result<()> {
    lib.remove_document(id)?;
    lib.save_index(&cfg.storage.vector_index)?;
    println!("removed {id}");
    Ok(())
}

fn cmd_rebuild_index(lib: &mut Lib, cfg: &Config, json: bool) -> Result<()> {
    let count = lib.rebuild_index()?;
    lib.save_index(&cfg.storage.vector_index)?;
    if json {
        print_json(&serde_json::json!({ "vectors_indexed": count }))?;
    } else {
        println!("rebuilt index with {count} vectors");
    }
    Ok(())
}

// --- helpers ----------------------------------------------------------------

fn is_url(arg: &str) -> bool {
    arg.starts_with("http://") || arg.starts_with("https://")
}

fn parse_format_arg(raw: &str) -> std::result::Result<Format, String> {
    match raw {
        "markdown" => Ok(Format::Markdown),
        "html" => Ok(Format::Html),
        "text" => Ok(Format::PlainText),
        _ => Err(contract::unsupported_format_message(raw)),
    }
}

fn snippet(content: &str) -> String {
    let one_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 160;
    if one_line.chars().count() > MAX {
        let truncated: String = one_line.chars().take(MAX).collect();
        format!("{truncated}…")
    } else {
        one_line
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| Error::InvalidInput(format!("json serialization failed: {e}")))?;
    println!("{s}");
    Ok(())
}
