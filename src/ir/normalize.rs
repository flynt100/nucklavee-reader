use super::types::{Block, BlockNode, Document, Inline};

/// Normalize a document in place: merges adjacent `Inline::Text` nodes and
/// strips empty `Inline::Text("")` values. Recurses through all block/inline
/// structure. Provenance is not modified.
pub fn normalize_document(doc: &mut Document) {
    for node in &mut doc.body {
        normalize_block_node(node);
    }
}

fn normalize_block_node(node: &mut BlockNode) {
    normalize_block(&mut node.block);
}

fn normalize_block(block: &mut Block) {
    match block {
        Block::Heading { content, .. } | Block::Paragraph { content } => {
            normalize_inlines(content);
        }
        Block::CodeBlock { .. } | Block::ThematicBreak => {}
        Block::Table { headers, rows } => {
            for cell in headers {
                normalize_inlines(cell);
            }
            for row in rows {
                for cell in row {
                    normalize_inlines(cell);
                }
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for child in &mut item.content {
                    normalize_block_node(child);
                }
            }
        }
        Block::BlockQuote { children } => {
            for child in children {
                normalize_block_node(child);
            }
        }
        Block::GenericBlock { content, .. } => {
            normalize_inlines(content);
        }
    }
}

fn normalize_inlines(inlines: &mut Vec<Inline>) {
    // First recurse.
    for inline in inlines.iter_mut() {
        normalize_inline(inline);
    }

    // Merge adjacent text nodes and drop empty text nodes.
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines.drain(..) {
        match inline {
            Inline::Text(ref s) if s.is_empty() => {}
            Inline::Text(s) => match out.last_mut() {
                Some(Inline::Text(prev)) => prev.push_str(&s),
                _ => out.push(Inline::Text(s)),
            },
            other => out.push(other),
        }
    }
    *inlines = out;
}

fn normalize_inline(inline: &mut Inline) {
    match inline {
        Inline::Text(_) | Inline::Code(_) | Inline::LineBreak | Inline::Image { .. } => {}
        Inline::Styled { children, .. } | Inline::Link { children, .. } => {
            normalize_inlines(children);
        }
    }
}
