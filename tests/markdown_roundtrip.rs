//! Markdown roundtrip harness.
//!
//! For every fixture:
//! 1. parse the source into IR
//! 2. validate IR
//! 3. emit the IR back to markdown
//! 4. re-parse the emitted markdown
//! 5. compare the two IR trees by structural equivalence
//!
//! Provenance ranges on the first parse are additionally checked for
//! in-bounds correctness against the source length.
//!
//! On failure, the test prints:
//! - the original fixture
//! - the first IR (debug)
//! - the emitted markdown
//! - the second IR (debug)
//! - the structural diff
//! - a diagnostic summary
//!
//! Policy-area grouping for normalization-delta coverage:
//! - A) Core structural equivalence gate: `roundtrip_*` fixture tests
//! - B) Canonicalization/determinism: `parse_emit_is_deterministic_*`
//! - C) Syntax-preservation contracts: explicit emitted-text/golden tests
//! - D) Frontmatter boundary integrity: frontmatter roundtrip + boundary checks
//! - E) Provenance/validation invariants: validation assertions + range bounds test

use std::path::{Path, PathBuf};

use nucklavee::emitters::markdown::emit_markdown;
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::{Document, structural_diff, validate};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixture(name: &str) -> (String, String) {
    let path = fixtures_dir().join(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    (name.to_string(), content)
}

fn opts() -> ParseOptions {
    ParseOptions::default()
}

fn opts_with_callout_normalization() -> ParseOptions {
    ParseOptions {
        normalize_bare_callouts: true,
        ..ParseOptions::default()
    }
}

fn roundtrip(name: &str) {
    let (label, source) = load_fixture(name);
    let doc1 = parse_markdown(&source, opts());
    if let Err(e) = validate(&doc1, Some(source.len())) {
        panic!(
            "fixture {label}: initial IR failed validation: {e}\n--- source ---\n{source}\n--- ir1 ---\n{:#?}",
            doc1
        );
    }

    let emitted = emit_markdown(&doc1);
    let doc2 = parse_markdown(&emitted, opts());
    if let Err(e) = validate(&doc2, Some(emitted.len())) {
        panic!(
            "fixture {label}: re-emitted IR failed validation: {e}\n--- source ---\n{source}\n--- ir1 ---\n{:#?}\n--- emitted ---\n{emitted}\n--- ir2 ---\n{:#?}",
            doc1, doc2
        );
    }

    if let Some(diff) = structural_diff(&doc1, &doc2) {
        panic!(
            "fixture {label}: roundtrip structural mismatch: {diff}\n\
             --- source ---\n{source}\n\
             --- ir1 ---\n{:#?}\n\
             --- emitted ---\n{emitted}\n\
             --- ir2 ---\n{:#?}\n\
             --- diagnostics (ir1) ---\n{:?}\n\
             --- diagnostics (ir2) ---\n{:?}\n",
            doc1, doc2, doc1.diagnostics, doc2.diagnostics
        );
    }
}

fn roundtrip_expected_stable(name: &str) {
    let (label, source) = load_fixture(name);
    let doc = parse_markdown(&source, opts());
    assert!(
        doc.diagnostics.is_empty(),
        "fixture {label}: expected stable fixture with no diagnostics, got {:?}",
        doc.diagnostics
    );
    roundtrip(name);
}

fn roundtrip_expected_diagnostic(name: &str) {
    let (label, source) = load_fixture(name);
    let doc = parse_markdown(&source, opts());
    assert!(
        !doc.diagnostics.is_empty(),
        "fixture {label}: expected diagnostics, got none"
    );
    roundtrip(name);
}

const FIXTURE_EXPANSION_EXPECTED_STABLE: &[&str] = &[
    "12_callout_blockquote.md",
    "13_frontmatter_nested_yaml.md",
    "13_syntax_preservation.md",
    "14_math_delimiters.md",
    "16_escape_boundaries.md",
    "15_nested_mixed_structures.md",
    "20_styled_leading_list_items.md",
];

const FIXTURE_EXPANSION_EXPECTED_DIAGNOSTIC: &[&str] = &[
    "09_diagnostics.md",
    "17_common_malformed_variants.md",
    "21_math_inline_underscore_asterisk.md",
    "20_real_world_obsidian.md",
];

#[test]
// Phase 2B fixture-expansion classification: stable fixtures must stay diagnostic-free.
fn fixture_expansion_stable_classification_is_enforced() {
    for fixture in FIXTURE_EXPANSION_EXPECTED_STABLE {
        roundtrip_expected_stable(fixture);
    }
}

#[test]
// Phase 2B fixture-expansion classification: malformed/lossy fixtures must emit diagnostics.
fn fixture_expansion_expected_diagnostic_classification_is_enforced() {
    for fixture in FIXTURE_EXPANSION_EXPECTED_DIAGNOSTIC {
        roundtrip_expected_diagnostic(fixture);
    }
}

#[test]
// Policy A: core structural equivalence gate
fn roundtrip_basic() {
    roundtrip("01_basic.md");
}

#[test]
// Policy B: list marker canonicalization accepted if structure is stable
fn roundtrip_lists() {
    roundtrip("02_lists.md");
}

#[test]
// Policy A: core structural equivalence gate
fn roundtrip_code_and_quotes() {
    roundtrip("03_code_and_quotes.md");
}

#[test]
// Policy A: core structural equivalence gate
fn roundtrip_links_and_images() {
    roundtrip("04_links_and_images.md");
}

#[test]
// Policy A: core structural equivalence gate
fn roundtrip_tables() {
    roundtrip("05_tables.md");
}

#[test]
// Policy A: core structural equivalence gate
fn roundtrip_headings() {
    roundtrip("06_headings.md");
}

#[test]
// Policy B: whitespace/edge canonicalization accepted if structure is stable
fn roundtrip_edges() {
    roundtrip("07_edge.md");
}

#[test]
// Policy B: hard nested-list/table normalization accepted if structure is stable
fn roundtrip_hard() {
    roundtrip("08_hard.md");
}

#[test]
// Policy D: frontmatter/body boundary must remain intact
fn roundtrip_frontmatter() {
    roundtrip("10_frontmatter.md");
}

#[test]
// Policy C: literal syntax protection gate
fn roundtrip_literal_markdown_syntax() {
    roundtrip("11_literal_syntax.md");
}

#[test]
// Policy C: callout-like blockquote semantics must survive canonicalization
fn roundtrip_callout_blockquote_fixture() {
    roundtrip("12_callout_blockquote.md");
}

#[test]
// Policy D: nested YAML frontmatter integrity
fn roundtrip_frontmatter_nested_yaml() {
    roundtrip("13_frontmatter_nested_yaml.md");
}

#[test]
// Policy C: syntax-preservation fixture broad gate
fn roundtrip_syntax_preservation_fixture() {
    roundtrip("13_syntax_preservation.md");
}

#[test]
// Policy C: math delimiter preservation gate
fn roundtrip_math_delimiters_fixture() {
    roundtrip("14_math_delimiters.md");
}

#[test]
// Policy E: nested mixed list/blockquote/table interactions should remain structurally stable.
fn roundtrip_nested_mixed_structures_fixture() {
    roundtrip("15_nested_mixed_structures.md");
}

#[test]
// Policy C: expected stable escaping/literal-boundary behavior
fn roundtrip_escape_boundaries_fixture_expected_stable() {
    roundtrip_expected_stable("16_escape_boundaries.md");
}

#[test]
// Policy E: expected diagnostic behavior for malformed-but-common source variants
fn roundtrip_common_malformed_variants_fixture_expected_diagnostic() {
    roundtrip_expected_diagnostic("17_common_malformed_variants.md");
}

#[test]
// Policy E: phase-2B pseudo-table conversion should roundtrip deterministically
fn roundtrip_phase2b_tsv_like_fixture() {
    roundtrip("18_phase2b_tsv_like.md");
}

#[test]
// Policy E: ambiguous tabular shape should fall back with diagnostics and remain deterministic
fn roundtrip_phase2b_malformed_tsv_fixture_expected_diagnostic() {
    roundtrip_expected_diagnostic("19_phase2b_tsv_malformed.md");
}

#[test]
// Policy A: list items with inline-leading content must survive roundtrip structurally.
fn roundtrip_styled_leading_list_items_fixture() {
    roundtrip("20_styled_leading_list_items.md");
}

#[test]
// Policy C/E: inline math shielding must preserve structure and deterministic emission.
fn roundtrip_math_inline_underscore_asterisk_fixture() {
    roundtrip("21_math_inline_underscore_asterisk.md");
}


#[test]
// Policy E: real-world Obsidian sample with raw HTML must remain expected-diagnostic and structurally deterministic.
fn roundtrip_real_world_obsidian_fixture_expected_diagnostic() {
    roundtrip_expected_diagnostic("20_real_world_obsidian.md");
}

#[test]
fn title_extraction_uses_first_h1() {
    let (_, source) = load_fixture("06_headings.md");
    let doc = parse_markdown(&source, opts());
    assert_eq!(doc.meta.title.as_deref(), Some("Root H1"));
}

#[test]
fn title_is_none_without_h1() {
    let src = "## Only an h2\n\ntext";
    let doc = parse_markdown(src, opts());
    assert!(doc.meta.title.is_none(), "title was {:?}", doc.meta.title);
}

#[test]
fn provenance_ranges_are_in_bounds() {
    let (_, source) = load_fixture("01_basic.md");
    let doc = parse_markdown(&source, opts());
    walk_ranges(&doc, source.len());
}


fn assert_resolved_idempotent_roundtrip(label: &str, source: &str, iterations: usize) {
    assert!(
        iterations >= 2,
        "{label}: iterations must be >= 2 to prove idempotent convergence"
    );

    let source_doc = parse_markdown(source, opts());
    let first_emitted = emit_markdown(&source_doc);
    let mut current_doc = parse_markdown(&first_emitted, opts());
    let mut previous_emitted: Option<String> = None;

    for i in 1..=iterations {
        let emitted = emit_markdown(&current_doc);
        let reparsed = parse_markdown(&emitted, opts());
        let diff = structural_diff(&current_doc, &reparsed);

        assert!(
            diff.is_none(),
            "{label}: expected no structural drift after convergence baseline; found diff at iteration {i}: {diff:?}"
        );
        assert!(
            validate(&reparsed, Some(emitted.len())).is_ok(),
            "{label}: reparsed document failed validation at iteration {i}"
        );

        if let Some(prev) = &previous_emitted {
            assert_eq!(
                &emitted, prev,
                "{label}: emission changed after convergence baseline at iteration {i}"
            );
        }
        previous_emitted = Some(emitted);
        current_doc = reparsed;
    }
}
fn assert_deterministic_parse_emit(label: &str, source: &str, iterations: usize) {
    assert!(iterations > 0, "iterations must be > 0");

    let mut current_doc = parse_markdown(source, opts());
    let mut baseline_emitted: Option<String> = None;
    let mut baseline_diff: Option<Option<String>> = None;

    for i in 0..iterations {
        let emitted = emit_markdown(&current_doc);
        let reparsed = parse_markdown(&emitted, opts());
        let diff = structural_diff(&current_doc, &reparsed);

        if let Some(expected) = &baseline_emitted {
            assert_eq!(
                &emitted, expected,
                "{label}: emitted markdown diverged at iteration {i}"
            );
        } else {
            baseline_emitted = Some(emitted.clone());
        }

        if let Some(expected) = &baseline_diff {
            assert_eq!(
                &diff, expected,
                "{label}: structural_diff diverged at iteration {i}"
            );
        } else {
            baseline_diff = Some(diff.clone());
        }

        assert!(
            validate(&reparsed, Some(emitted.len())).is_ok(),
            "{label}: reparsed document failed validation at iteration {i}"
        );

        current_doc = reparsed;
    }
}

fn assert_display_math_blocks_do_not_contain_bad_closer(label: &str, emitted: &str) {
    let mut in_dollar_math_block = false;
    let mut in_bracket_math_block = false;
    let mut current_block = String::new();

    let assert_current_block = |block: &str, context: &str| {
        assert!(
            !block.contains("\\ \\]"),
            "{label}: emitted display math block contains invalid `\\ \\]` sequence ({context}):\n{block}"
        );
    };

    for line in emitted.lines() {
        if in_dollar_math_block {
            if line.trim() == "$$" {
                assert_current_block(&current_block, "dollar-delimited");
                current_block.clear();
                in_dollar_math_block = false;
            } else {
                current_block.push_str(line);
                current_block.push('\n');
            }
            continue;
        }

        if in_bracket_math_block {
            if line.trim() == "\\]" {
                assert_current_block(&current_block, "bracket-delimited");
                current_block.clear();
                in_bracket_math_block = false;
            } else {
                current_block.push_str(line);
                current_block.push('\n');
            }
            continue;
        }

        if line.trim() == "$$" {
            in_dollar_math_block = true;
            continue;
        }

        if line.trim() == "\\[" {
            in_bracket_math_block = true;
        }
    }
}

#[test]
// Policy E: parse->emit->parse must converge to deterministic output
fn parse_emit_is_deterministic_for_nested_lists_and_tables_fixture() {
    let (label, source) = load_fixture("08_hard.md");
    assert_deterministic_parse_emit(&label, &source, 5);
}

#[test]
// Policy E: determinism holds even for diagnostic-producing inputs
fn parse_emit_is_deterministic_for_diagnostics_fixture() {
    let (label, source) = load_fixture("09_diagnostics.md");
    let initial = parse_markdown(&source, opts());
    assert!(
        !initial.diagnostics.is_empty(),
        "expected diagnostics fixture to produce diagnostics"
    );
    assert_deterministic_parse_emit(&label, &source, 5);
}

#[test]
// Policy E: deterministic parse->emit behavior for real-world Obsidian sample with intentional unsupported content.
fn parse_emit_is_deterministic_for_real_world_obsidian_fixture_expected_diagnostic() {
    let (label, source) = load_fixture("20_real_world_obsidian.md");
    let initial = parse_markdown(&source, opts());
    assert!(
        !initial.diagnostics.is_empty(),
        "expected real-world Obsidian fixture to produce diagnostics"
    );
    assert_deterministic_parse_emit(&label, &source, 5);
}

#[test]
// Policy E: determinism for phase-2B table fallback path
fn parse_emit_is_deterministic_for_phase2b_fallback_fixtures() {
    let (label_ok, source_ok) = load_fixture("18_phase2b_tsv_like.md");
    assert_deterministic_parse_emit(&label_ok, &source_ok, 5);

    let (label_bad, source_bad) = load_fixture("19_phase2b_tsv_malformed.md");
    let initial = parse_markdown(&source_bad, opts());
    assert!(
        !initial.diagnostics.is_empty(),
        "expected malformed phase-2B fixture to produce diagnostics"
    );
    assert_deterministic_parse_emit(&label_bad, &source_bad, 5);
}


#[test]
// Policy E (Finding 3 regression): pass criteria is zero structural diff and stable emission across repeated parse->emit cycles.
fn parse_emit_converges_for_styled_leading_list_fixture_after_list_item_root_cause_fix() {
    let (label, source) = load_fixture("20_styled_leading_list.md");
    assert_resolved_idempotent_roundtrip(&label, &source, 5);
}

#[test]
// Policy E (Finding 3 regression): pass criteria is idempotent convergence for underscore-heavy math forms with no iterative drift.
fn parse_emit_converges_for_math_underscore_fixture_after_math_root_cause_fix() {
    let (label, source) = load_fixture("21_math_underscore.md");
    assert_resolved_idempotent_roundtrip(&label, &source, 5);
}

#[test]
// Policy C regression: emitted display-math blocks must never include invalid `\ \]` closure text.
fn emitted_display_math_blocks_do_not_use_escaped_space_bracket_closer_for_known_fixtures() {
    for fixture in ["14_math_delimiters.md", "21_math_underscore.md"] {
        let (label, source) = load_fixture(fixture);
        let doc = parse_markdown(&source, opts());
        let emitted = emit_markdown(&doc);
        assert_display_math_blocks_do_not_contain_bad_closer(&label, &emitted);
    }
}

#[test]
// Policy E (Finding 3 regression): pass criteria is fixed-point parse->emit behavior for real-world mixed markdown after root-cause fixes.
fn parse_emit_converges_for_real_world_fixture_after_root_cause_fixes() {
    let (label, source) = load_fixture("22_real_world_mixed.md");
    assert_resolved_idempotent_roundtrip(&label, &source, 5);
}

#[test]
// Policy C: explicit contract for preserving literal wikilink/callout/math syntax
fn emitted_text_preserves_wikilink_callout_and_math_syntax() {
    let (_, source) = load_fixture("13_syntax_preservation.md");
    let doc = parse_markdown(&source, opts());
    let emitted = emit_markdown(&doc);

    assert!(
        emitted.contains("[[wikilink]]"),
        "expected emitted markdown to preserve wikilink syntax: {emitted}"
    );
    assert!(
        emitted.contains("> [!abstract]"),
        "expected emitted markdown to preserve callout marker syntax: {emitted}"
    );
    assert!(
        emitted.contains("$f([x]) = x^2 + y$"),
        "expected emitted markdown to preserve inline equation syntax: {emitted}"
    );

    let reparsed = parse_markdown(&emitted, opts());
    if let Some(diff) = structural_diff(&doc, &reparsed) {
        panic!(
            "expected emitted markdown to preserve syntax semantics, got diff: {diff}\n--- emitted ---\n{emitted}\n--- ir1 ---\n{doc:#?}\n--- ir2 ---\n{reparsed:#?}",
        );
    }
    assert!(
        validate(&reparsed, Some(emitted.len())).is_ok(),
        "expected emitted markdown to remain parseable"
    );
}

#[test]
// Policy C: golden output + structural contract for callout blockquote fixture
fn callout_blockquote_fixture_has_stable_golden_output_and_structure() {
    let (_, source) = load_fixture("12_callout_blockquote.md");
    let doc = parse_markdown(&source, opts());
    let emitted = emit_markdown(&doc);

    let expected = "# Callout Blockquote Fixture\n\n> [!note] Release Notes\n> First quoted line with **bold** and `inline code`.\n> Second quoted line with a [link](https://example.com/docs).\n> Third quoted line with *emphasis* and [[wikilink-like]] text.";
    assert_eq!(
        emitted, expected,
        "golden markdown output changed unexpectedly"
    );

    let reparsed = parse_markdown(&emitted, opts());
    if let Some(diff) = structural_diff(&doc, &reparsed) {
        panic!(
            "expected callout blockquote fixture to stay structurally equivalent after golden emission, got diff: {diff}\n--- emitted ---\n{emitted}\n--- ir1 ---\n{doc:#?}\n--- ir2 ---\n{reparsed:#?}",
        );
    }

    let quote = doc
        .body
        .iter()
        .find_map(|node| match &node.block {
            nucklavee::Block::BlockQuote { children } => Some(children),
            _ => None,
        })
        .expect("expected a blockquote block in fixture");

    assert_eq!(
        quote.len(),
        1,
        "expected callout-like quote content to remain a single paragraph"
    );
    assert!(
        matches!(quote[0].block, nucklavee::Block::Paragraph { .. }),
        "expected first quoted block to be a paragraph"
    );

    let line_break_count = match &quote[0].block {
        nucklavee::Block::Paragraph { content } => content
            .iter()
            .filter(|inline| matches!(inline, nucklavee::Inline::Text(s) if s == "\n"))
            .count(),
        _ => 0,
    };
    assert!(
        line_break_count >= 3,
        "expected quoted callout paragraph to preserve multiple logical lines"
    );
}

#[test]
// Policy C: golden output + structural contract for math delimiter fixture
fn math_delimiters_fixture_has_stable_golden_output_and_structure() {
    let (_, source) = load_fixture("14_math_delimiters.md");
    let doc = parse_markdown(&source, opts());
    let emitted = emit_markdown(&doc);

    let expected = "# Math Delimiter Fixture\n\nInline formulas stay inline: $a^2 + b^2 = c^2$, $e^{i\\pi} + 1 = 0$, and $f([x]) = x^2 + y$.\n\nDisplay equations preserve escaped delimiters:\n\n\\[\nE = mc^2\n\\]\n\nParagraph break between equations.\n\n\\[\n\\int_0^1 x^2 \\, dx = \\frac{1}{3}\n\\]\n\nMixed inline + display math in one section: $\\alpha + \\beta$ then \\[\n\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}\n\\]";
    assert_eq!(
        emitted, expected,
        "golden markdown output changed unexpectedly for math delimiters fixture"
    );

    let reparsed = parse_markdown(&emitted, opts());
    if let Some(diff) = structural_diff(&doc, &reparsed) {
        panic!(
            "expected math delimiters fixture to stay structurally equivalent after golden emission, got diff: {diff}\n--- emitted ---\n{emitted}\n--- ir1 ---\n{doc:#?}\n--- ir2 ---\n{reparsed:#?}",
        );
    }
}

#[test]
// Policy C: optional bare-callout normalization emits stable canonical output
fn normalized_bare_callout_fixture_has_stable_output() {
    let (_, source) = load_fixture("18_normalized_bare_callout.md");
    let doc = parse_markdown(&source, opts_with_callout_normalization());
    let emitted = emit_markdown(&doc);
    let expected = "# Bare Callout Normalization Fixture\n\n> [!tip] Normalized heading\n> Second line stays in the same callout paragraph.";
    assert_eq!(emitted, expected, "normalized callout output changed");

    let reparsed = parse_markdown(&emitted, opts_with_callout_normalization());
    if let Some(diff) = structural_diff(&doc, &reparsed) {
        panic!(
            "expected normalized callout fixture to stay structurally equivalent, got diff: {diff}\n--- emitted ---\n{emitted}\n--- ir1 ---\n{doc:#?}\n--- ir2 ---\n{reparsed:#?}",
        );
    }
}

#[test]
// Policy D: frontmatter must be captured in metadata, not body blocks
fn frontmatter_is_captured_and_not_treated_as_body() {
    let (_, source) = load_fixture("10_frontmatter.md");
    let doc = parse_markdown(&source, opts());
    let frontmatter = doc.meta.frontmatter.as_ref().expect("expected frontmatter");
    let yaml = &frontmatter.yaml;
    assert!(
        matches!(
            doc.body.first().map(|n| &n.block),
            Some(nucklavee::Block::Heading { .. })
        ),
        "expected first body block to be heading, got {:?}",
        doc.body.first().map(|n| &n.block)
    );

    assert!(yaml.contains("tags:"), "expected tags key");
    assert!(
        yaml.contains("published: true"),
        "expected boolean key in frontmatter"
    );
    assert!(
        yaml.contains("date: 2026-04-24"),
        "expected date key in frontmatter"
    );
    assert!(
        yaml.contains("\"[[Nucklavee Reader]]\""),
        "expected Obsidian wikilink-style string in frontmatter aliases"
    );
}

#[test]
fn escaped_newline_frontmatter_is_normalized_and_captured() {
    let (_, source) = load_fixture("18_escaped_newline_frontmatter.md");
    let doc = parse_markdown(&source, opts());
    let frontmatter = doc.meta.frontmatter.as_ref().expect("expected frontmatter");

    assert!(
        frontmatter.yaml.contains("title: Escaped Stream"),
        "expected escaped-newline stream to decode into YAML frontmatter"
    );
    assert!(
        doc.diagnostics.iter().any(|d| {
            matches!(d.kind, nucklavee::DiagnosticKind::Normalized)
                && d.message
                    .contains("decoded escaped newline stream before markdown parse")
        }),
        "expected escaped-newline normalization diagnostic, got {:?}",
        doc.diagnostics
    );
}

#[test]
fn unfenced_frontmatter_is_inferred_and_reported_deterministically() {
    let (_, source) = load_fixture("19_unfenced_frontmatter_block.md");
    let doc = parse_markdown(&source, opts());
    let frontmatter = doc.meta.frontmatter.as_ref().expect("expected frontmatter");

    assert!(
        frontmatter.yaml.contains("author: Parser Bot"),
        "expected inferred frontmatter to include YAML-like key/value lines"
    );
    assert!(
        doc.diagnostics.iter().any(|d| {
            matches!(d.kind, nucklavee::DiagnosticKind::Normalized)
                && d.message
                    .contains("inferred unfenced YAML-like frontmatter block at document start")
        }),
        "expected inferred-frontmatter normalization diagnostic, got {:?}",
        doc.diagnostics
    );

    assert_deterministic_parse_emit("19_unfenced_frontmatter_block.md", &source, 5);
}

#[test]
fn math_inline_underscore_asterisk_fixture_is_deterministic_and_structurally_equivalent() {
    let (_, source) = load_fixture("21_math_inline_underscore_asterisk.md");
    let doc1 = parse_markdown(&source, opts());
    let emitted = emit_markdown(&doc1);
    let doc2 = parse_markdown(&emitted, opts());
    let canonical = emit_markdown(&doc2);

    if let Some(diff) = structural_diff(&doc1, &doc2) {
        panic!(
            "expected fixture 21 to stay structurally equivalent after reparse, got diff: {diff}\n--- emitted ---\n{emitted}\n--- ir1 ---\n{doc1:#?}\n--- ir2 ---\n{doc2:#?}"
        );
    }
    assert_deterministic_parse_emit("21_math_inline_underscore_asterisk.md", &canonical, 5);
}

fn walk_ranges(doc: &Document, source_len: usize) {
    use nucklavee::{Block, BlockNode};

    fn walk(node: &BlockNode, source_len: usize) {
        if let Some(r) = node.prov.byte_range {
            assert!(
                r.is_valid_within(source_len),
                "byte range {:?} not in bounds [0, {source_len}]",
                r
            );
        }
        match &node.block {
            Block::BlockQuote { children } => {
                for c in children {
                    walk(c, source_len);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    for c in &item.content {
                        walk(c, source_len);
                    }
                }
            }
            _ => {}
        }
    }

    for node in &doc.body {
        walk(node, source_len);
    }
}
