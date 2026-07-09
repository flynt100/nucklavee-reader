//! Structure-aware chunker (spec §6).
//!
//! Walks a document's IR and emits embedding-ready [`Chunk`]s, reusing the
//! `section_path` provenance the parser already computed (it never re-derives
//! heading hierarchies). Design:
//!
//! - Consecutive non-heading blocks with the same `section_path` form a
//!   section; a chunk never spans two sections (so it never crosses an H2
//!   boundary).
//! - Tables and code blocks become their own chunks (tagged `Table` / `Code`).
//!   Everything else (paragraphs, lists, quotes, generic blocks) accumulates
//!   into `Prose` chunks.
//! - Oversized content is split within budget: prose at block then sentence
//!   boundaries, code at blank lines, tables by rows with the header
//!   re-prepended. A token-window fallback guarantees every chunk fits.
//!
//! Token counting uses `tiktoken-rs` cl100k_base (spec §6.3).

use tiktoken_rs::CoreBPE;
use uuid::Uuid;

use crate::Result;
use crate::chunking::{Chunk, ChunkBlockType, ChunkOptions, Chunker};
use crate::emitters::text::{emit_text_blocks, render_inlines_plain};
use crate::ir::{Block, BlockNode, DocumentId, Inline};

pub struct StructuralChunker {
    bpe: CoreBPE,
}

impl std::fmt::Debug for StructuralChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructuralChunker").finish_non_exhaustive()
    }
}

impl StructuralChunker {
    /// Build a chunker using the cl100k_base tokenizer (matches OpenAI
    /// embeddings).
    pub fn new() -> Result<Self> {
        let bpe = tiktoken_rs::cl100k_base()
            .map_err(|e| crate::Error::Chunking(format!("failed to load tokenizer: {e}")))?;
        Ok(Self { bpe })
    }

    fn count(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }

    /// Public token count using the same cl100k_base tokenizer the chunker
    /// uses (so `context_window` budgets match chunk budgets).
    pub fn count_tokens(&self, text: &str) -> usize {
        self.count(text)
    }

    /// Split `text` into pieces each within `budget` tokens by decoding fixed
    /// token windows. Exact last-resort fallback (may cut mid-word).
    fn token_window_split(&self, text: &str, budget: usize) -> Vec<String> {
        let tokens = self.bpe.encode_with_special_tokens(text);
        if tokens.len() <= budget || budget == 0 {
            return vec![text.to_string()];
        }
        let mut pieces = Vec::new();
        for window in tokens.chunks(budget) {
            if let Ok(s) = self.bpe.decode(window) {
                pieces.push(s);
            }
        }
        if pieces.is_empty() {
            vec![text.to_string()]
        } else {
            pieces
        }
    }

    /// Greedily pack `units` into chunks joined by `joiner`, keeping each within
    /// `budget`. Units larger than budget are pre-split with `presplit`.
    fn pack(
        &self,
        units: Vec<String>,
        budget: usize,
        joiner: &str,
        presplit: impl Fn(&str) -> Vec<String>,
    ) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut current = String::new();
        for unit in units {
            let pieces = if self.count(&unit) > budget {
                presplit(&unit)
            } else {
                vec![unit]
            };
            for piece in pieces {
                if piece.trim().is_empty() {
                    continue;
                }
                if current.is_empty() {
                    current = piece;
                } else if self.count(&current) + self.count(joiner) + self.count(&piece) <= budget {
                    current.push_str(joiner);
                    current.push_str(&piece);
                } else {
                    out.push(std::mem::take(&mut current));
                    current = piece;
                }
            }
        }
        if !current.trim().is_empty() {
            out.push(current);
        }
        out
    }
}

/// Split prose text at sentence boundaries (`. `, `? `, `! `, or a newline).
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        current.push(ch);
        let is_terminator = matches!(ch, '.' | '?' | '!');
        let next_is_boundary = chars
            .get(i + 1)
            .map(|n| n.is_whitespace())
            .unwrap_or(true);
        if (is_terminator && next_is_boundary) || ch == '\n' {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_string());
            }
            current.clear();
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        sentences.push(trimmed.to_string());
    }
    if sentences.is_empty() {
        vec![text.trim().to_string()]
    } else {
        sentences
    }
}

fn row_text(cells: &[Vec<Inline>]) -> String {
    cells
        .iter()
        .map(|cell| render_inlines_plain(cell))
        .collect::<Vec<_>>()
        .join(" | ")
}

struct Builder<'a> {
    chunker: &'a StructuralChunker,
    document_id: DocumentId,
    budget: usize,
    out: Vec<Chunk>,
    seq: usize,
    current_path: Vec<String>,
    prose_buf: Vec<BlockNode>,
}

impl<'a> Builder<'a> {
    fn new(chunker: &'a StructuralChunker, document_id: DocumentId, budget: usize) -> Self {
        Self {
            chunker,
            document_id,
            budget,
            out: Vec::new(),
            seq: 0,
            current_path: Vec::new(),
            prose_buf: Vec::new(),
        }
    }

    fn push_chunk(&mut self, content: String, block_type: ChunkBlockType, path: &[String]) {
        if content.trim().is_empty() {
            return;
        }
        let token_count = self.chunker.count(&content);
        self.out.push(Chunk {
            id: Uuid::new_v4(),
            document_id: self.document_id,
            section_path: path.to_vec(),
            content,
            block_type,
            sequence_index: self.seq,
            token_count,
        });
        self.seq += 1;
    }

    fn flush_prose(&mut self) {
        if self.prose_buf.is_empty() {
            return;
        }
        let blocks = std::mem::take(&mut self.prose_buf);
        let path = self.current_path.clone();
        let text = emit_text_blocks(&blocks);
        if text.trim().is_empty() {
            return;
        }
        if self.chunker.count(&text) <= self.budget {
            self.push_chunk(text, ChunkBlockType::Prose, &path);
            return;
        }
        // Oversized: split at block boundaries, then sentences, then tokens.
        let block_units: Vec<String> = blocks
            .iter()
            .map(|b| emit_text_blocks(std::slice::from_ref(b)))
            .collect();
        let budget = self.budget;
        let pieces = self.chunker.pack(block_units, budget, "\n\n", |unit| {
            let sentences = split_sentences(unit);
            self.chunker
                .pack(sentences, budget, " ", |s| self.chunker.token_window_split(s, budget))
        });
        for piece in pieces {
            self.push_chunk(piece, ChunkBlockType::Prose, &path);
        }
    }

    fn emit_code(&mut self, language: &Option<String>, content: &str) {
        let path = self.current_path.clone();
        let _ = language; // language is metadata; chunk content is the code text
        if self.chunker.count(content) <= self.budget {
            self.push_chunk(content.to_string(), ChunkBlockType::Code, &path);
            return;
        }
        let segments: Vec<String> = content.split("\n\n").map(str::to_string).collect();
        let budget = self.budget;
        let pieces = self
            .chunker
            .pack(segments, budget, "\n\n", |s| self.chunker.token_window_split(s, budget));
        for piece in pieces {
            self.push_chunk(piece, ChunkBlockType::Code, &path);
        }
    }

    fn emit_table(&mut self, headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) {
        let path = self.current_path.clone();
        let header_line = row_text(headers);
        let row_lines: Vec<String> = rows.iter().map(|r| row_text(r)).collect();

        // A table with no body rows is a single chunk (just the header).
        if row_lines.is_empty() {
            self.push_chunk(header_line, ChunkBlockType::Table, &path);
            return;
        }

        let mut current_rows: Vec<String> = Vec::new();
        for row in row_lines {
            let candidate = table_content(&header_line, &current_rows, Some(&row));
            if current_rows.is_empty() {
                // Header + a single row may itself exceed budget: token-split.
                if self.chunker.count(&candidate) > self.budget {
                    for piece in self.chunker.token_window_split(&candidate, self.budget) {
                        self.push_chunk(piece, ChunkBlockType::Table, &path);
                    }
                    continue;
                }
                current_rows.push(row);
            } else if self.chunker.count(&candidate) <= self.budget {
                current_rows.push(row);
            } else {
                let content = table_content(&header_line, &current_rows, None);
                self.push_chunk(content, ChunkBlockType::Table, &path);
                current_rows = vec![row];
            }
        }
        if !current_rows.is_empty() {
            let content = table_content(&header_line, &current_rows, None);
            self.push_chunk(content, ChunkBlockType::Table, &path);
        }
    }

    fn process(&mut self, node: &BlockNode) {
        if matches!(node.block, Block::Heading { .. }) {
            // Headings are section boundaries; their text lives in the
            // section_path of the blocks beneath them.
            return;
        }
        if node.prov.section_path != self.current_path {
            self.flush_prose();
            self.current_path = node.prov.section_path.clone();
        }
        match &node.block {
            Block::Table { headers, rows } => {
                self.flush_prose();
                self.emit_table(headers, rows);
            }
            Block::CodeBlock { language, content } => {
                self.flush_prose();
                self.emit_code(language, content);
            }
            _ => self.prose_buf.push(node.clone()),
        }
    }
}

fn table_content(header: &str, rows: &[String], extra: Option<&str>) -> String {
    let mut lines = vec![header.to_string()];
    lines.extend(rows.iter().cloned());
    if let Some(extra) = extra {
        lines.push(extra.to_string());
    }
    lines.join("\n")
}

impl Chunker for StructuralChunker {
    fn chunk(
        &self,
        document: &crate::ir::Document,
        opts: &ChunkOptions,
    ) -> Result<Vec<Chunk>> {
        let budget = opts.token_budget.max(1);
        let mut builder = Builder::new(self, document.meta.id, budget);
        for node in &document.body {
            builder.process(node);
        }
        builder.flush_prose();
        Ok(builder.out)
    }
}
