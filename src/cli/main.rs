use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nucklavee::chunking::ChunkId;
use nucklavee::embedder::Embedder;
use nucklavee::ir::{DocumentId, Source};
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::vector::VectorIndex;
use nucklavee::{Format, Library, Result};

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
#[command(about = "nucklavee Phase-2 CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Ingest {
        path: String,
    },
    Emit {
        #[arg(long)]
        id: DocumentId,
        #[arg(long, value_parser = parse_format)]
        format: Format,
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
        Commands::Ingest { path } => run_ingest(lib, &path),
        Commands::Emit { id, format } => run_emit(lib, id, format),
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
) -> Result<()> {
    let id = lib.ingest(Source::File(path.into()))?;
    println!("{id}");
    Ok(())
}

fn run_emit(
    lib: &mut Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder>,
    id: DocumentId,
    format: Format,
) -> Result<()> {
    let output = lib.emit(id, format)?;
    println!("{output}");
    Ok(())
}

fn parse_format(raw: &str) -> Result<Format> {
    match raw {
        "markdown" => Ok(Format::Markdown),
        "html" => Ok(Format::Html),
        "text" | "plain" | "plaintext" => Ok(Format::PlainText),
        _ => Err(nucklavee::Error::InvalidInput(format!(
            "unsupported format '{raw}'. supported: markdown"
        ))),
    }
}
