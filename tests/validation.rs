//! IR validation tests. Constructs known-bad IR values and verifies the
//! validator rejects them with the expected error.

use chrono::Utc;
use nucklavee::{
    Block, BlockNode, ByteRange, Document, DocumentMeta, Inline, ListItem, Provenance,
    SourceFormat, SourceInfo, ValidationError, validate,
};
use uuid::Uuid;

fn bare_doc(body: Vec<BlockNode>) -> Document {
    Document {
        meta: DocumentMeta {
            id: Uuid::nil(),
            source: SourceInfo {
                raw_source: "test".into(),
            },
            format: SourceFormat::Markdown,
            title: None,
            frontmatter: None,
            ingested_at: Utc::now(),
            content_hash: String::new(),
            processing_fingerprint: String::new(),
        },
        body,
        diagnostics: Vec::new(),
    }
}

fn prov() -> Provenance {
    Provenance::new(Uuid::nil())
}

#[test]
fn heading_level_zero_rejected() {
    let doc = bare_doc(vec![BlockNode::new(
        Block::Heading {
            level: 0,
            content: vec![Inline::Text("x".into())],
        },
        prov(),
    )]);
    assert_eq!(
        validate(&doc, None),
        Err(ValidationError::HeadingLevelOutOfRange(0))
    );
}

#[test]
fn heading_level_seven_rejected() {
    let doc = bare_doc(vec![BlockNode::new(
        Block::Heading {
            level: 7,
            content: vec![],
        },
        prov(),
    )]);
    assert_eq!(
        validate(&doc, None),
        Err(ValidationError::HeadingLevelOutOfRange(7))
    );
}

#[test]
fn generic_confidence_out_of_range() {
    let doc = bare_doc(vec![BlockNode::new(
        Block::GenericBlock {
            content: vec![],
            hint: None,
            confidence: 1.5,
        },
        prov(),
    )]);
    assert!(matches!(
        validate(&doc, None),
        Err(ValidationError::ConfidenceOutOfRange(_))
    ));
}

#[test]
fn empty_list_rejected() {
    let doc = bare_doc(vec![BlockNode::new(
        Block::List {
            ordered: false,
            items: Vec::new(),
        },
        prov(),
    )]);
    assert_eq!(validate(&doc, None), Err(ValidationError::EmptyList));
}

#[test]
fn table_row_shape_mismatch() {
    let doc = bare_doc(vec![BlockNode::new(
        Block::Table {
            headers: vec![
                vec![Inline::Text("A".into())],
                vec![Inline::Text("B".into())],
            ],
            rows: vec![vec![vec![Inline::Text("only one".into())]]],
        },
        prov(),
    )]);
    assert!(matches!(
        validate(&doc, None),
        Err(ValidationError::TableRowShape { .. })
    ));
}

#[test]
fn link_empty_url_rejected() {
    let doc = bare_doc(vec![BlockNode::new(
        Block::Paragraph {
            content: vec![Inline::Link {
                url: String::new(),
                children: vec![Inline::Text("x".into())],
            }],
        },
        prov(),
    )]);
    assert_eq!(validate(&doc, None), Err(ValidationError::LinkEmptyUrl));
}

#[test]
fn inverted_provenance_range_rejected() {
    let mut p = prov();
    p.byte_range = Some(ByteRange::new(10, 5));
    let doc = bare_doc(vec![BlockNode::new(
        Block::Paragraph { content: vec![] },
        p,
    )]);
    assert!(matches!(
        validate(&doc, None),
        Err(ValidationError::ProvenanceRangeInverted { .. })
    ));
}

#[test]
fn out_of_bounds_provenance_range_rejected() {
    let mut p = prov();
    p.byte_range = Some(ByteRange::new(0, 100));
    let doc = bare_doc(vec![BlockNode::new(
        Block::Paragraph { content: vec![] },
        p,
    )]);
    assert!(matches!(
        validate(&doc, Some(10)),
        Err(ValidationError::ProvenanceRangeOutOfBounds { .. })
    ));
}

#[test]
fn nested_list_item_validation_surfaces() {
    // Inner list has zero items → should surface when validating the outer.
    let inner = BlockNode::new(
        Block::List {
            ordered: false,
            items: Vec::new(),
        },
        prov(),
    );
    let outer = BlockNode::new(
        Block::List {
            ordered: false,
            items: vec![ListItem {
                content: vec![inner],
            }],
        },
        prov(),
    );
    let doc = bare_doc(vec![outer]);
    assert_eq!(validate(&doc, None), Err(ValidationError::EmptyList));
}
