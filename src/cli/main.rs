use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nucklavee::chunking::ChunkId;
use nucklavee::embedder::Embedder;
use nucklavee::ir::{DocumentId, Source};
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::vector::VectorIndex;
use nucklavee::{Format, IngestOptions, Library, Result};

#[derive(Debug, Default)]
struct NoopVectorIndex;

impl VectorIndex for NoopVectorIndex {
    fn add(&mut self, _id: ChunkId, _vector: Vec<f32>) -> Result<()> {
        Ok(())
    }

    fn search(&self, _query: &[f32], _limit: usize) -> Result<Vec<(ChunkId, f32)>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
struct NoopEmbedder;

impl Embedder for NoopEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(vec![vec![0.0; self.dimension()]; texts.len()])
    }

    fn dimension(&self) -> usize {
        8
    }
}

fn main() -> ExitCode {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );

    let cli = Cli::parse();
    let result = run_phase2_service(&mut lib, cli.command);

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "nucklavee")]
#[command(
    about = "nucklavee Phase-2 CLI (ingest-emit retrieval only)",
    long_about = "Phase 2 supports retrieval through `ingest-emit` only. The CLI uses an in-memory document store, and document IDs are process-local and valid only in the process that created them. Standalone `emit --id` is disabled in Phase 2."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(
        about = "Ingest a markdown file and print its in-memory document ID (same process only)"
    )]
    Ingest {
        #[arg(help = "Path to a local .md file")]
        path: String,
        #[arg(long, help = "Normalize bare callouts like `[!tip]` into blockquotes")]
        normalize_bare_callouts: bool,
    },
    #[command(
        hide = true,
        about = "Legacy Phase-2 command: emit by in-memory document ID (disabled)"
    )]
    Emit {
        #[arg(long)]
        id: DocumentId,
        #[arg(long, value_parser = parse_format)]
        format: Format,
    },
    #[command(about = "Ingest and immediately emit in one process (recommended for Phase 2)")]
    IngestEmit {
        #[arg(help = "Path to a local .md file")]
        path: String,
        #[arg(long, value_parser = parse_format, default_value = "markdown")]
        format: Format,
        #[arg(long, help = "Normalize bare callouts like `[!tip]` into blockquotes")]
        normalize_bare_callouts: bool,
    },
    Query,
    ContextWindow,
    Html,
    Pdf,
}

fn run_phase2_service(
    lib: &mut Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder>,
    command: Commands,
) -> Result<()> {
    match command {
        Commands::Ingest {
            path,
            normalize_bare_callouts,
        } => run_ingest(lib, &path, normalize_bare_callouts),
        Commands::Emit { id: _, format: _ } => Err(nucklavee::Error::InvalidInput(
            "emit --id is disabled in Phase 2 because document IDs are process-local. use `ingest-emit <path> --format markdown`".into(),
        )),
        Commands::IngestEmit {
            path,
            format,
            normalize_bare_callouts,
        } => run_ingest_emit(lib, &path, format, normalize_bare_callouts),
        Commands::Query => Err(nucklavee::Error::NotImplemented(
            "query is not implemented in Phase 2 (markdown ingest/emit only)",
        )),
        Commands::ContextWindow => Err(nucklavee::Error::NotImplemented(
            "context_window is not implemented in Phase 2 (markdown ingest/emit only)",
        )),
        Commands::Html => Err(nucklavee::Error::NotImplemented(
            "html pipeline is not implemented in Phase 2; markdown only",
        )),
        Commands::Pdf => Err(nucklavee::Error::NotImplemented(
            "pdf pipeline is not implemented in Phase 2; markdown only",
        )),
    }
}

fn run_ingest(
    lib: &mut Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder>,
    path: &str,
    normalize_bare_callouts: bool,
) -> Result<()> {
    let id = ingest_doc_id(lib, path, normalize_bare_callouts)?;
    println!("{id}");
    Ok(())
}

fn run_ingest_emit(
    lib: &mut Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder>,
    path: &str,
    format: Format,
    normalize_bare_callouts: bool,
) -> Result<()> {
    let id = ingest_doc_id(lib, path, normalize_bare_callouts)?;
    let output = lib.emit(id, format)?;
    println!("{output}");
    Ok(())
}

fn ingest_doc_id(
    lib: &mut Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder>,
    path: &str,
    normalize_bare_callouts: bool,
) -> Result<DocumentId> {
    lib.ingest_with_options(
        Source::File(path.into()),
        IngestOptions {
            normalize_bare_callouts,
        },
    )
}

fn parse_format(raw: &str) -> Result<Format> {
    match raw {
        "markdown" => Ok(Format::Markdown),
        _ => Err(nucklavee::Error::InvalidInput(format!(
            "unsupported format '{raw}'. supported: markdown"
        ))),
    }
}
