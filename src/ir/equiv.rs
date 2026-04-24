//! Structural equivalence for IR trees.
//!
//! Two documents are structurally equivalent when their block/inline trees
//! match after normalization, ignoring provenance, ingestion timestamps,
//! UUIDs, and diagnostics. Used by the roundtrip harness to assert that
//! parse → emit → parse preserves structure.

use super::normalize::normalize_document;
use super::types::{Block, BlockNode, Document, Inline, ListItem};

/// Return true if the two documents are structurally equivalent after
/// normalization.
pub fn structurally_equivalent(a: &Document, b: &Document) -> bool {
    structural_diff(a, b).is_none()
}

/// Return `None` if structurally equivalent, or a human-readable description
/// of the first difference.
pub fn structural_diff(a: &Document, b: &Document) -> Option<String> {
    let mut a = a.clone();
    let mut b = b.clone();
    normalize_document(&mut a);
    normalize_document(&mut b);

    if a.meta.title != b.meta.title {
        return Some(format!(
            "title mismatch: {:?} vs {:?}",
            a.meta.title, b.meta.title
        ));
    }
    if a.meta.format != b.meta.format {
        return Some(format!(
            "format mismatch: {:?} vs {:?}",
            a.meta.format, b.meta.format
        ));
    }
    if a.meta.frontmatter.as_ref().map(|f| &f.yaml) != b.meta.frontmatter.as_ref().map(|f| &f.yaml)
    {
        return Some("frontmatter YAML mismatch".to_string());
    }
    diff_nodes(&a.body, &b.body, "body")
}

fn diff_nodes(a: &[BlockNode], b: &[BlockNode], path: &str) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!(
            "{path}: length mismatch ({} vs {}) — left={} right={}",
            a.len(),
            b.len(),
            summarize(a),
            summarize(b)
        ));
    }
    for (idx, (left, right)) in a.iter().zip(b.iter()).enumerate() {
        if let Some(d) = diff_block(&left.block, &right.block, &format!("{path}[{idx}]")) {
            return Some(d);
        }
    }
    None
}

fn diff_block(a: &Block, b: &Block, path: &str) -> Option<String> {
    match (a, b) {
        (
            Block::Heading {
                level: la,
                content: ca,
            },
            Block::Heading {
                level: lb,
                content: cb,
            },
        ) => {
            if la != lb {
                return Some(format!("{path}: heading level {la} vs {lb}"));
            }
            diff_inlines(ca, cb, &format!("{path}.heading"))
        }
        (Block::Paragraph { content: ca }, Block::Paragraph { content: cb }) => {
            diff_inlines(ca, cb, &format!("{path}.paragraph"))
        }
        (
            Block::CodeBlock {
                language: la,
                content: ca,
            },
            Block::CodeBlock {
                language: lb,
                content: cb,
            },
        ) => {
            if la != lb {
                return Some(format!("{path}: code lang {la:?} vs {lb:?}"));
            }
            if ca != cb {
                return Some(format!("{path}: code content {ca:?} vs {cb:?}"));
            }
            None
        }
        (
            Block::Table {
                headers: ha,
                rows: ra,
            },
            Block::Table {
                headers: hb,
                rows: rb,
            },
        ) => diff_table(ha, ra, hb, rb, path),
        (
            Block::List {
                ordered: oa,
                items: ia,
            },
            Block::List {
                ordered: ob,
                items: ib,
            },
        ) => {
            if oa != ob {
                return Some(format!("{path}: list ordered {oa} vs {ob}"));
            }
            diff_list_items(ia, ib, path)
        }
        (Block::BlockQuote { children: ca }, Block::BlockQuote { children: cb }) => {
            diff_nodes(ca, cb, &format!("{path}.blockquote"))
        }
        (
            Block::GenericBlock {
                content: ca,
                hint: ha,
                ..
            },
            Block::GenericBlock {
                content: cb,
                hint: hb,
                ..
            },
        ) => {
            if ha != hb {
                return Some(format!("{path}: generic hint {ha:?} vs {hb:?}"));
            }
            diff_inlines(ca, cb, &format!("{path}.generic"))
        }
        (Block::ThematicBreak, Block::ThematicBreak) => None,
        (x, y) => Some(format!(
            "{path}: block variant mismatch — {} vs {}",
            variant_name(x),
            variant_name(y)
        )),
    }
}

fn diff_table(
    ha: &[Vec<Inline>],
    ra: &[Vec<Vec<Inline>>],
    hb: &[Vec<Inline>],
    rb: &[Vec<Vec<Inline>>],
    path: &str,
) -> Option<String> {
    if ha.len() != hb.len() {
        return Some(format!(
            "{path}: table header col count {} vs {}",
            ha.len(),
            hb.len()
        ));
    }
    for (i, (ca, cb)) in ha.iter().zip(hb.iter()).enumerate() {
        if let Some(d) = diff_inlines(ca, cb, &format!("{path}.header[{i}]")) {
            return Some(d);
        }
    }
    if ra.len() != rb.len() {
        return Some(format!(
            "{path}: table row count {} vs {}",
            ra.len(),
            rb.len()
        ));
    }
    for (ri, (rowa, rowb)) in ra.iter().zip(rb.iter()).enumerate() {
        if rowa.len() != rowb.len() {
            return Some(format!(
                "{path}: table row {ri} col count {} vs {}",
                rowa.len(),
                rowb.len()
            ));
        }
        for (ci, (ca, cb)) in rowa.iter().zip(rowb.iter()).enumerate() {
            if let Some(d) = diff_inlines(ca, cb, &format!("{path}.row[{ri}].cell[{ci}]")) {
                return Some(d);
            }
        }
    }
    None
}

fn diff_list_items(a: &[ListItem], b: &[ListItem], path: &str) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!(
            "{path}: list item count {} vs {}",
            a.len(),
            b.len()
        ));
    }
    for (idx, (la, lb)) in a.iter().zip(b.iter()).enumerate() {
        if let Some(d) = diff_nodes(&la.content, &lb.content, &format!("{path}.item[{idx}]")) {
            return Some(d);
        }
    }
    None
}

fn diff_inlines(a: &[Inline], b: &[Inline], path: &str) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!(
            "{path}: inline count {} vs {} — left={:?} right={:?}",
            a.len(),
            b.len(),
            a,
            b
        ));
    }
    for (idx, (la, lb)) in a.iter().zip(b.iter()).enumerate() {
        if let Some(d) = diff_inline(la, lb, &format!("{path}[{idx}]")) {
            return Some(d);
        }
    }
    None
}

fn diff_inline(a: &Inline, b: &Inline, path: &str) -> Option<String> {
    match (a, b) {
        (Inline::Text(sa), Inline::Text(sb)) => {
            if sa != sb {
                Some(format!("{path}: text {sa:?} vs {sb:?}"))
            } else {
                None
            }
        }
        (Inline::Code(sa), Inline::Code(sb)) => {
            if sa != sb {
                Some(format!("{path}: code {sa:?} vs {sb:?}"))
            } else {
                None
            }
        }
        (Inline::LineBreak, Inline::LineBreak) => None,
        (
            Inline::Styled {
                style: sa,
                children: ca,
            },
            Inline::Styled {
                style: sb,
                children: cb,
            },
        ) => {
            if sa != sb {
                return Some(format!("{path}: style {sa:?} vs {sb:?}"));
            }
            diff_inlines(ca, cb, &format!("{path}.styled"))
        }
        (
            Inline::Link {
                url: ua,
                children: ca,
            },
            Inline::Link {
                url: ub,
                children: cb,
            },
        ) => {
            if ua != ub {
                return Some(format!("{path}: link url {ua:?} vs {ub:?}"));
            }
            diff_inlines(ca, cb, &format!("{path}.link"))
        }
        (Inline::Image { url: ua, alt: aa }, Inline::Image { url: ub, alt: ab }) => {
            if ua != ub {
                return Some(format!("{path}: image url {ua:?} vs {ub:?}"));
            }
            if aa != ab {
                return Some(format!("{path}: image alt {aa:?} vs {ab:?}"));
            }
            None
        }
        (x, y) => Some(format!(
            "{path}: inline variant mismatch — {} vs {}",
            inline_name(x),
            inline_name(y)
        )),
    }
}

fn variant_name(b: &Block) -> &'static str {
    match b {
        Block::Heading { .. } => "Heading",
        Block::Paragraph { .. } => "Paragraph",
        Block::CodeBlock { .. } => "CodeBlock",
        Block::Table { .. } => "Table",
        Block::List { .. } => "List",
        Block::BlockQuote { .. } => "BlockQuote",
        Block::GenericBlock { .. } => "GenericBlock",
        Block::ThematicBreak => "ThematicBreak",
    }
}

fn inline_name(i: &Inline) -> &'static str {
    match i {
        Inline::Text(_) => "Text",
        Inline::Styled { .. } => "Styled",
        Inline::Code(_) => "Code",
        Inline::Link { .. } => "Link",
        Inline::Image { .. } => "Image",
        Inline::LineBreak => "LineBreak",
    }
}

fn summarize(nodes: &[BlockNode]) -> String {
    let names: Vec<&str> = nodes.iter().map(|n| variant_name(&n.block)).collect();
    format!("[{}]", names.join(", "))
}
