use std::env;
use std::process::ExitCode;

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
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );

    let result = match args[0].as_str() {
        "ingest" => run_ingest(&mut lib, &args[1..]),
        "emit" => run_emit(&lib, &args[1..]),
        "query" => Err(nucklavee::Error::NotImplemented(
            "query is not implemented in Phase 2 (markdown ingest/emit only)",
        )),
        "context_window" => Err(nucklavee::Error::NotImplemented(
            "context_window is not implemented in Phase 2 (markdown ingest/emit only)",
        )),
        "html" => Err(nucklavee::Error::NotImplemented(
            "html pipeline is not implemented in Phase 2; markdown only",
        )),
        "pdf" => Err(nucklavee::Error::NotImplemented(
            "pdf pipeline is not implemented in Phase 2; markdown only",
        )),
        _ => {
            print_usage();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run_ingest(
    lib: &mut Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder>,
    args: &[String],
) -> Result<()> {
    if args.len() != 1 {
        return Err(nucklavee::Error::InvalidInput(
            "usage: nucklavee ingest <path.md>".to_string(),
        ));
    }

    let id = lib.ingest(Source::File(args[0].clone().into()))?;
    println!("{id}");
    Ok(())
}

fn run_emit(
    lib: &Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder>,
    args: &[String],
) -> Result<()> {
    let mut id: Option<DocumentId> = None;
    let mut format: Option<Format> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--id" => {
                let Some(raw_id) = args.get(i + 1) else {
                    return Err(nucklavee::Error::InvalidInput(
                        "missing value for --id".to_string(),
                    ));
                };
                id = Some(raw_id.parse().map_err(|err| {
                    nucklavee::Error::InvalidInput(format!("invalid document id '{raw_id}': {err}"))
                })?);
                i += 2;
            }
            "--format" => {
                let Some(raw_format) = args.get(i + 1) else {
                    return Err(nucklavee::Error::InvalidInput(
                        "missing value for --format".to_string(),
                    ));
                };
                format = Some(parse_format(raw_format)?);
                i += 2;
            }
            unexpected => {
                return Err(nucklavee::Error::InvalidInput(format!(
                    "unexpected argument '{unexpected}'. usage: nucklavee emit --id <doc_id> --format markdown"
                )));
            }
        }
    }

    let id = id.ok_or_else(|| {
        nucklavee::Error::InvalidInput(
            "missing --id. usage: nucklavee emit --id <doc_id> --format markdown".to_string(),
        )
    })?;
    let format = format.ok_or_else(|| {
        nucklavee::Error::InvalidInput(
            "missing --format. usage: nucklavee emit --id <doc_id> --format markdown".to_string(),
        )
    })?;

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

fn print_usage() {
    eprintln!("nucklavee Phase-2 CLI");
    eprintln!("usage:");
    eprintln!("  nucklavee ingest <path.md>");
    eprintln!("  nucklavee emit --id <doc_id> --format markdown");
}
