use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nucklavee::ir::{DocumentId, Source};
use nucklavee::phase2_contract;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
use nucklavee::{Format, IngestOptions, Library, Result};

fn main() -> ExitCode {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    )
    .expect("build library");

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
    about = "nucklavee CLI (markdown/html ingest, markdown emit)",
    long_about = "Ingest markdown (.md) or html (.html/.htm) files and emit canonical markdown via `ingest-emit`. The CLI uses an in-memory document store, and document IDs are process-local and valid only in the process that created them. Standalone `emit --id` is disabled."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(
        about = "Ingest a markdown or html file and print its in-memory document ID (same process only)"
    )]
    Ingest {
        #[arg(help = "Path to a local .md/.html file, or an http(s):// URL")]
        path: String,
        #[arg(long, help = "Normalize bare callouts like `[!tip]` into blockquotes")]
        normalize_bare_callouts: bool,
    },
    #[command(
        hide = true,
        about = "Legacy command: emit by in-memory document ID (disabled)"
    )]
    Emit {
        #[arg(long)]
        id: DocumentId,
        #[arg(long, value_parser = parse_format)]
        format: Format,
    },
    #[command(about = "Ingest and immediately emit in one process (recommended)")]
    IngestEmit {
        #[arg(help = "Path to a local .md/.html file, or an http(s):// URL")]
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
        Commands::Emit { id: _, format: _ } => Err(phase2_contract::invalid_input(
            phase2_contract::EMIT_BY_ID_DISABLED,
        )),
        Commands::IngestEmit {
            path,
            format,
            normalize_bare_callouts,
        } => run_ingest_emit(lib, &path, format, normalize_bare_callouts),
        Commands::Query => Err(phase2_contract::not_implemented(
            phase2_contract::QUERY_NOT_IMPLEMENTED,
        )),
        Commands::ContextWindow => Err(phase2_contract::not_implemented(
            phase2_contract::CONTEXT_WINDOW_NOT_IMPLEMENTED,
        )),
        Commands::Html => Err(phase2_contract::not_implemented(
            phase2_contract::HTML_PIPELINE_NOT_IMPLEMENTED,
        )),
        Commands::Pdf => Err(phase2_contract::not_implemented(
            phase2_contract::PDF_PIPELINE_NOT_IMPLEMENTED,
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
    let source = if is_url(path) {
        Source::Url(path.to_string())
    } else {
        Source::File(path.into())
    };
    lib.ingest_with_options(
        source,
        IngestOptions {
            normalize_bare_callouts,
            ..Default::default()
        },
    )
}

fn is_url(arg: &str) -> bool {
    arg.starts_with("http://") || arg.starts_with("https://")
}

fn parse_format(raw: &str) -> Result<Format> {
    match raw {
        "markdown" => Ok(Format::Markdown),
        "html" => Ok(Format::Html),
        "text" => Ok(Format::PlainText),
        _ => Err(phase2_contract::invalid_input(
            phase2_contract::unsupported_format_message(raw),
        )),
    }
}
