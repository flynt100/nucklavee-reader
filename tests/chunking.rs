//! Structure-aware chunker tests (spec §6 / §11.2, audit Task 7):
//! section boundaries, token budget, provenance paths, and block-type tags.

use nucklavee::chunking::{ChunkBlockType, ChunkOptions, Chunker, StructuralChunker};
use nucklavee::normalize_document;
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::{Chunk, Document};

fn doc(markdown: &str) -> Document {
    // Mirror the ingest path: parse then canonicalize.
    let mut d = parse_markdown(markdown, ParseOptions::default());
    normalize_document(&mut d);
    d
}

fn chunk(markdown: &str, budget: usize) -> Vec<Chunk> {
    let chunker = StructuralChunker::new().expect("tokenizer loads offline");
    chunker
        .chunk(
            &doc(markdown),
            &ChunkOptions {
                token_budget: budget,
            },
        )
        .expect("chunk")
}

const SAMPLE: &str = "\
# Guide

Intro paragraph under the title.

## Install

Install step one here. INSTALLMARKER present.

```bash
apt install thing
run --now
```

## Usage

Usage paragraph. USAGEMARKER present.

| Cmd | Effect |
| --- | --- |
| run | does a thing |
| stop | halts it |
";

#[test]
fn no_chunk_spans_two_h2_sections() {
    let chunks = chunk(SAMPLE, 512);
    for c in &chunks {
        assert!(
            !(c.content.contains("INSTALLMARKER") && c.content.contains("USAGEMARKER")),
            "a chunk spanned two H2 sections:\n{}",
            c.content
        );
    }
    // The two sections' prose live under different section paths.
    let paths: std::collections::HashSet<Vec<String>> =
        chunks.iter().map(|c| c.section_path.clone()).collect();
    assert!(paths.contains(&vec!["Guide".to_string(), "Install".to_string()]));
    assert!(paths.contains(&vec!["Guide".to_string(), "Usage".to_string()]));
}

#[test]
fn section_paths_match_heading_hierarchy() {
    let chunks = chunk(SAMPLE, 512);

    let intro = chunks
        .iter()
        .find(|c| c.content.contains("Intro paragraph"))
        .expect("intro chunk");
    assert_eq!(intro.section_path, vec!["Guide".to_string()]);

    let code = chunks
        .iter()
        .find(|c| matches!(c.block_type, ChunkBlockType::Code))
        .expect("code chunk");
    assert_eq!(
        code.section_path,
        vec!["Guide".to_string(), "Install".to_string()]
    );

    let table = chunks
        .iter()
        .find(|c| matches!(c.block_type, ChunkBlockType::Table))
        .expect("table chunk");
    assert_eq!(
        table.section_path,
        vec!["Guide".to_string(), "Usage".to_string()]
    );
}

#[test]
fn block_types_are_tagged() {
    let chunks = chunk(SAMPLE, 512);
    assert!(
        chunks
            .iter()
            .any(|c| matches!(c.block_type, ChunkBlockType::Prose))
    );
    assert!(
        chunks
            .iter()
            .any(|c| matches!(c.block_type, ChunkBlockType::Code))
    );
    assert!(
        chunks
            .iter()
            .any(|c| matches!(c.block_type, ChunkBlockType::Table))
    );

    // The code chunk carries the code text, not the language marker.
    let code = chunks
        .iter()
        .find(|c| matches!(c.block_type, ChunkBlockType::Code))
        .unwrap();
    assert!(code.content.contains("apt install thing"));

    // The table chunk keeps the header row plus data rows.
    let table = chunks
        .iter()
        .find(|c| matches!(c.block_type, ChunkBlockType::Table))
        .unwrap();
    assert!(table.content.contains("Cmd | Effect"));
    assert!(table.content.contains("run | does a thing"));
}

#[test]
fn sequence_indices_are_contiguous_from_zero() {
    let chunks = chunk(SAMPLE, 512);
    let seqs: Vec<usize> = chunks.iter().map(|c| c.sequence_index).collect();
    assert_eq!(seqs, (0..chunks.len()).collect::<Vec<_>>());
}

#[test]
fn token_counts_are_recorded_and_within_budget() {
    let chunks = chunk(SAMPLE, 512);
    for c in &chunks {
        assert!(c.token_count > 0, "empty chunk emitted");
        assert!(
            c.token_count <= 512,
            "chunk exceeds budget: {} tokens\n{}",
            c.token_count,
            c.content
        );
    }
}

#[test]
fn oversized_prose_is_split_within_a_tiny_budget() {
    let long = "\
# Doc

Sentence one is here. Sentence two follows it. Sentence three keeps going. \
Sentence four is also present. Sentence five ends the paragraph.
";
    let budget = 8;
    let chunks = chunk(long, budget);
    assert!(chunks.len() > 1, "expected the paragraph to split");
    for c in &chunks {
        assert!(
            c.token_count <= budget,
            "chunk over tiny budget: {} tokens\n{}",
            c.token_count,
            c.content
        );
    }
}

#[test]
fn oversized_table_splits_by_rows_reprepending_header() {
    let table = "\
# T

| Key | Value |
| --- | --- |
| alpha | one |
| beta | two |
| gamma | three |
| delta | four |
";
    // Budget large enough for header + a row or two, small enough to force
    // multiple table chunks.
    let chunks = chunk(table, 12);
    let table_chunks: Vec<&Chunk> = chunks
        .iter()
        .filter(|c| matches!(c.block_type, ChunkBlockType::Table))
        .collect();
    assert!(
        table_chunks.len() > 1,
        "expected the table to split by rows"
    );
    for c in &table_chunks {
        assert!(
            c.content.starts_with("Key | Value"),
            "each table chunk must re-prepend the header:\n{}",
            c.content
        );
        assert!(
            c.token_count <= 12,
            "table chunk over budget:\n{}",
            c.content
        );
    }
}

#[test]
fn empty_and_heading_only_documents_produce_no_chunks() {
    assert!(chunk("", 512).is_empty());
    assert!(
        chunk("# Just A Heading\n", 512).is_empty(),
        "a heading with no body has no embeddable content"
    );
}
