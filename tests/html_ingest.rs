//! Phase-3 HTML parser tests: DOM→IR mapping, content extraction, title
//! extraction, degradation behavior, and cross-format integrity
//! (html → IR → markdown → IR structural equivalence).

use std::path::Path;

use nucklavee::emitters::markdown::emit_markdown;
use nucklavee::ir::Source;
use nucklavee::parsers::html::{HtmlParseOptions, parse_html};
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
use nucklavee::{
    Block, DiagnosticKind, Document, Format, Inline, Library, structural_diff_bodies, validate,
};

fn opts() -> HtmlParseOptions {
    HtmlParseOptions::default()
}

fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/html")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

fn plain_text_of(doc: &Document) -> String {
    fn push_inlines(inlines: &[Inline], out: &mut String) {
        for inline in inlines {
            match inline {
                Inline::Text(s) | Inline::Code(s) => out.push_str(s),
                Inline::LineBreak => out.push(' '),
                Inline::Styled { children, .. } | Inline::Link { children, .. } => {
                    push_inlines(children, out)
                }
                Inline::Image { alt, .. } => {
                    if let Some(alt) = alt {
                        out.push_str(alt);
                    }
                }
            }
        }
    }
    fn push_block(block: &Block, out: &mut String) {
        match block {
            Block::Heading { content, .. }
            | Block::Paragraph { content }
            | Block::GenericBlock { content, .. } => {
                push_inlines(content, out);
                out.push('\n');
            }
            Block::CodeBlock { content, .. } => {
                out.push_str(content);
                out.push('\n');
            }
            Block::Table { headers, rows } => {
                for cell in headers {
                    push_inlines(cell, out);
                    out.push(' ');
                }
                for row in rows {
                    for cell in row {
                        push_inlines(cell, out);
                        out.push(' ');
                    }
                }
                out.push('\n');
            }
            Block::List { items, .. } => {
                for item in items {
                    for child in &item.content {
                        push_block(&child.block, out);
                    }
                }
            }
            Block::BlockQuote { children } => {
                for child in children {
                    push_block(&child.block, out);
                }
            }
            Block::ThematicBreak => {}
        }
    }
    let mut out = String::new();
    for node in &doc.body {
        push_block(&node.block, &mut out);
    }
    out
}

/// html → IR₁ → markdown → IR₂: bodies must be structurally equivalent and
/// both IRs valid. This is the Phase-3 cross-format integrity gate.
fn assert_cross_format_integrity(label: &str, html: &str) {
    let doc1 = parse_html(html, opts());
    validate(&doc1, None).unwrap_or_else(|e| panic!("{label}: html IR failed validation: {e}"));

    let markdown = emit_markdown(&doc1);
    let doc2 = parse_markdown(&markdown, ParseOptions::default());
    validate(&doc2, Some(markdown.len()))
        .unwrap_or_else(|e| panic!("{label}: reparsed markdown IR failed validation: {e}"));

    if let Some(diff) = structural_diff_bodies(&doc1, &doc2) {
        panic!(
            "{label}: html→IR→markdown→IR structural mismatch: {diff}\n\
             --- emitted markdown ---\n{markdown}\n\
             --- html IR ---\n{doc1:#?}\n\
             --- markdown IR ---\n{doc2:#?}"
        );
    }
}

// --- DOM→IR mapping ---------------------------------------------------------

#[test]
fn maps_core_block_and_inline_elements() {
    let html = r#"
    <html><body><main>
      <h1>Title</h1>
      <p>Text with <strong>bold</strong>, <em>italic</em>, <del>gone</del>,
         <code>span()</code>, a <a href="https://e.com/">link</a>, and
         an image <img src="https://e.com/i.png" alt="alt text">.</p>
      <pre><code class="language-rust">fn main() {}</code></pre>
      <ul><li>one</li><li>two</li></ul>
      <ol><li>first</li></ol>
      <blockquote><p>quoted</p></blockquote>
      <hr>
    </main></body></html>"#;
    let doc = parse_html(html, opts());
    validate(&doc, None).expect("valid IR");

    let kinds: Vec<&str> = doc
        .body
        .iter()
        .map(|n| match &n.block {
            Block::Heading { .. } => "heading",
            Block::Paragraph { .. } => "paragraph",
            Block::CodeBlock { .. } => "code",
            Block::List { ordered: false, .. } => "ul",
            Block::List { ordered: true, .. } => "ol",
            Block::BlockQuote { .. } => "quote",
            Block::ThematicBreak => "hr",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["heading", "paragraph", "code", "ul", "ol", "quote", "hr"],
        "unexpected block sequence: {doc:#?}"
    );

    let Block::CodeBlock { language, content } = &doc.body[2].block else {
        panic!("expected code block");
    };
    assert_eq!(language.as_deref(), Some("rust"));
    assert_eq!(content, "fn main() {}");

    let text = plain_text_of(&doc);
    assert!(text.contains("bold") && text.contains("italic") && text.contains("alt text"));
}

#[test]
fn maps_table_with_thead_and_inline_formatting() {
    let html = r#"<body><table>
      <thead><tr><th>Name</th><th>Value</th></tr></thead>
      <tbody>
        <tr><td><code>alpha</code></td><td><strong>1</strong></td></tr>
        <tr><td>beta</td><td>2</td></tr>
      </tbody>
    </table></body>"#;
    let doc = parse_html(html, opts());
    validate(&doc, None).expect("valid IR");

    let Block::Table { headers, rows } = &doc.body[0].block else {
        panic!("expected table, got {:?}", doc.body[0].block);
    };
    assert_eq!(headers.len(), 2);
    assert_eq!(rows.len(), 2);
    assert!(matches!(&rows[0][0][0], Inline::Code(s) if s == "alpha"));
    assert!(matches!(&rows[0][1][0], Inline::Styled { .. }));
}

#[test]
fn headerless_table_promotes_first_row_with_diagnostic() {
    let html =
        "<body><table><tr><td>a</td><td>b</td></tr><tr><td>1</td><td>2</td></tr></table></body>";
    let doc = parse_html(html, opts());
    validate(&doc, None).expect("valid IR");

    let Block::Table { headers, rows } = &doc.body[0].block else {
        panic!("expected table");
    };
    assert_eq!(headers.len(), 2);
    assert_eq!(rows.len(), 1);
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Normalized
                && d.message.contains("first row was promoted to header")),
        "expected header-promotion diagnostic: {:?}",
        doc.diagnostics
    );
}

#[test]
fn ragged_table_rows_are_padded_with_diagnostic() {
    let html = "<body><table><thead><tr><th>a</th><th>b</th><th>c</th></tr></thead><tr><td>1</td></tr></table></body>";
    let doc = parse_html(html, opts());
    validate(&doc, None).expect("valid IR despite ragged input");
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.message.contains("padded with empty cells")),
        "expected padding diagnostic: {:?}",
        doc.diagnostics
    );
}

#[test]
fn nested_lists_and_loose_li_text_become_paragraph_plus_list() {
    let html = "<body><ul><li>outer<ul><li>inner</li></ul></li></ul></body>";
    let doc = parse_html(html, opts());
    validate(&doc, None).expect("valid IR");

    let Block::List { items, .. } = &doc.body[0].block else {
        panic!("expected list");
    };
    assert_eq!(items[0].content.len(), 2, "li should hold paragraph + list");
    assert!(matches!(items[0].content[0].block, Block::Paragraph { .. }));
    assert!(matches!(items[0].content[1].block, Block::List { .. }));
}

#[test]
fn unknown_element_degrades_to_generic_block_with_class_hint() {
    let html = r#"<body><main><p>before</p><widget-panel class="pinout sidebar-ish">PIN 1: VCC</widget-panel></main></body>"#;
    let doc = parse_html(html, opts());
    validate(&doc, None).expect("valid IR");

    let generic = doc
        .body
        .iter()
        .find_map(|n| match &n.block {
            Block::GenericBlock {
                content,
                hint,
                confidence,
            } => Some((content.clone(), hint.clone(), *confidence)),
            _ => None,
        })
        .expect("expected a GenericBlock");
    assert_eq!(generic.1.as_deref(), Some("pinout sidebar-ish"));
    assert!((0.0..=1.0).contains(&generic.2));
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Unsupported && d.message.contains("widget-panel")),
        "expected unsupported-element diagnostic: {:?}",
        doc.diagnostics
    );
}

#[test]
fn anchor_without_href_flattens_and_empty_img_is_dropped() {
    let html = r#"<body><p>see <a>bare anchor</a> and <img alt="no src"> end</p></body>"#;
    let doc = parse_html(html, opts());
    validate(&doc, None).expect("valid IR — no empty-url nodes may exist");

    let text = plain_text_of(&doc);
    assert!(text.contains("bare anchor"));
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Lossy),
        "expected lossy diagnostics: {:?}",
        doc.diagnostics
    );
}

#[test]
fn section_paths_follow_heading_hierarchy() {
    let html = r#"<body><main>
      <h1>A</h1><p>under a</p>
      <h2>B</h2><p>under b</p>
      <h2>C</h2><p>under c</p>
    </main></body>"#;
    let doc = parse_html(html, opts());

    let paths: Vec<Vec<String>> = doc
        .body
        .iter()
        .filter_map(|n| match &n.block {
            Block::Paragraph { .. } => Some(n.prov.section_path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        paths,
        vec![
            vec!["A".to_string()],
            vec!["A".to_string(), "B".to_string()],
            vec!["A".to_string(), "C".to_string()],
        ]
    );

    // Headings carry their parent path.
    let heading_paths: Vec<Vec<String>> = doc
        .body
        .iter()
        .filter_map(|n| match &n.block {
            Block::Heading { .. } => Some(n.prov.section_path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        heading_paths,
        vec![vec![], vec!["A".to_string()], vec!["A".to_string()]]
    );
}

// --- title extraction --------------------------------------------------------

#[test]
fn title_prefers_title_tag_then_h1_then_og_title() {
    let with_title =
        "<html><head><title>From Title</title></head><body><h1>From H1</h1></body></html>";
    assert_eq!(
        parse_html(with_title, opts()).meta.title.as_deref(),
        Some("From Title")
    );

    let with_h1 = "<html><body><h1>From H1</h1></body></html>";
    assert_eq!(
        parse_html(with_h1, opts()).meta.title.as_deref(),
        Some("From H1")
    );

    let with_og = r#"<html><head><meta property="og:title" content="From OG"></head><body><p>x</p></body></html>"#;
    assert_eq!(
        parse_html(with_og, opts()).meta.title.as_deref(),
        Some("From OG")
    );
}

// --- content extraction --------------------------------------------------------

#[test]
fn content_extraction_strips_chrome_on_all_fixture_pages() {
    for (fixture, chrome_markers, content_marker) in [
        (
            "docs_site.html",
            vec!["NAVLINK", "SIDEBAR", "FOOTER", "TRACKING-BEACON"],
            "widget engine reads its settings",
        ),
        (
            "wiki_article.html",
            vec!["WIKIHEADER", "WIKINAV", "WIKIFOOTER"],
            "horse-like demon from Orcadian folklore",
        ),
        (
            "blog_post.html",
            vec!["BLOGNAV", "COMMENTS"],
            "finally shipped",
        ),
    ] {
        let doc = parse_html(&load_fixture(fixture), opts());
        validate(&doc, None).unwrap_or_else(|e| panic!("{fixture}: invalid IR: {e}"));
        let text = plain_text_of(&doc);
        for marker in chrome_markers {
            assert!(
                !text.contains(marker),
                "{fixture}: chrome marker {marker:?} leaked into content:\n{text}"
            );
        }
        assert!(
            text.contains(content_marker),
            "{fixture}: expected content missing:\n{text}"
        );
    }
}

#[test]
fn density_descent_selects_wrapped_content_div() {
    let doc = parse_html(&load_fixture("wiki_article.html"), opts());
    assert!(
        doc.diagnostics.iter().any(|d| {
            d.kind == DiagnosticKind::Normalized
                && d.message.contains("content extraction selected")
                && d.message.contains("id=\"content\"")
        }),
        "expected extraction diagnostic naming #content: {:?}",
        doc.diagnostics
    );
}

// --- cross-format integrity -----------------------------------------------------

#[test]
fn cross_format_integrity_docs_site() {
    assert_cross_format_integrity("docs_site.html", &load_fixture("docs_site.html"));
}

#[test]
fn cross_format_integrity_wiki_article() {
    assert_cross_format_integrity("wiki_article.html", &load_fixture("wiki_article.html"));
}

#[test]
fn cross_format_integrity_blog_post() {
    assert_cross_format_integrity("blog_post.html", &load_fixture("blog_post.html"));
}

// --- Library wiring ----------------------------------------------------------

#[test]
fn library_ingests_raw_html_and_emits_markdown() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex::default(),
        NoopEmbedder,
    )
    .expect("build library");
    let id = lib
        .ingest(Source::RawHtml(
            "<html><head><title>T</title></head><body><main><h1>T</h1><p>hello <strong>world</strong></p></main></body></html>".into(),
        ))
        .expect("raw html ingest should succeed in Phase 3");

    let markdown = lib.emit(id, Format::Markdown).expect("markdown emit");
    assert!(markdown.contains("# T"));
    assert!(markdown.contains("hello **world**"));
}

#[test]
fn library_ingests_html_file_by_extension() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex::default(),
        NoopEmbedder,
    )
    .expect("build library");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/html/blog_post.html");
    let id = lib.ingest(Source::File(path)).expect(".html file ingest");
    let markdown = lib.emit(id, Format::Markdown).expect("markdown emit");
    assert!(markdown.contains("# Shipping the Reader"));
    assert!(!markdown.contains("BLOGNAV"));
}

#[test]
fn library_rejects_unsupported_extension_with_contract_message() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex::default(),
        NoopEmbedder,
    )
    .expect("build library");
    let err = lib
        .ingest(Source::File("notes.docx".into()))
        .expect_err("docx must be rejected");
    assert_eq!(
        err.to_string(),
        format!(
            "invalid input: {}",
            nucklavee::contract::unsupported_extension_message("docx")
        )
    );
}
