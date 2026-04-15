use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser as ClapParser, Subcommand};

use nucklavee::emitters::Emitter;
use nucklavee::emitters::markdown::MarkdownEmitter;
use nucklavee::ir::Source;
use nucklavee::parsers::markdown::MarkdownParser;

#[derive(Debug, ClapParser)]
#[command(name = "nucklavee", about = "Universal document transformation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse a markdown file and print the intermediate representation as JSON.
    Parse {
        /// Path to a markdown file. Use `-` to read from stdin.
        path: PathBuf,
    },
    /// Parse a markdown file and re-emit it as markdown. Useful for roundtrip inspection.
    Emit {
        /// Path to a markdown file. Use `-` to read from stdin.
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Parse { path } => run_parse(path),
        Command::Emit { path } => run_emit(path),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_parse(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let (input, source) = read_input(&path)?;
    let mut document = MarkdownParser.parse_with_source(&input, source)?;
    document.validate_strict().map_err(|e| format!("invalid IR produced: {e}"))?;
    // Stable identity for debug output so the printed JSON is reproducible.
    document.meta.id = uuid::Uuid::nil();
    let json = serde_json::to_string_pretty(&document)?;
    println!("{json}");
    Ok(())
}

fn run_emit(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let (input, source) = read_input(&path)?;
    let document = MarkdownParser.parse_with_source(&input, source)?;
    let output = MarkdownEmitter.emit(&document)?;
    print!("{output}");
    Ok(())
}

fn read_input(path: &PathBuf) -> Result<(String, Source), Box<dyn std::error::Error>> {
    if path.as_os_str() == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        Ok((buf, Source::RawMarkdown(String::new())))
    } else {
        let input = std::fs::read_to_string(path)?;
        Ok((input, Source::File(path.clone())))
    }
}
