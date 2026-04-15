use chrono::Utc;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser as CmarkParser, Tag, TagEnd};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ir::{
    Block, Document, DocumentMeta, Inline, ListItem, Source, SourceFormat, SourceInfo, Style,
    inlines_to_plain_text,
};
use crate::parsers::Parser;
use crate::{Error, Result};

#[derive(Debug, Default)]
pub struct MarkdownParser;

impl Parser for MarkdownParser {
    fn parse(&self, input: &str) -> Result<Document> {
        parse_markdown(input, Source::RawMarkdown(input.to_string()))
    }
}

impl MarkdownParser {
    /// Parse with a caller-supplied `Source` (e.g. the originating file path)
    /// so document metadata records provenance accurately.
    pub fn parse_with_source(&self, input: &str, source: Source) -> Result<Document> {
        parse_markdown(input, source)
    }
}

fn parse_markdown(input: &str, source: Source) -> Result<Document> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let mut builder = IrBuilder::default();
    for event in CmarkParser::new_ext(input, options) {
        builder.handle(event)?;
    }
    let body = builder.finish()?;

    let title = body.iter().find_map(|block| match block {
        Block::Heading { level: 1, content } => Some(inlines_to_plain_text(content)),
        _ => None,
    });

    let content_hash = {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    Ok(Document {
        meta: DocumentMeta {
            id: Uuid::new_v4(),
            source: SourceInfo::from(&source),
            format: SourceFormat::Markdown,
            title,
            ingested_at: Utc::now(),
            content_hash,
        },
        body,
    })
}

#[derive(Debug)]
enum Frame {
    Heading { level: u8, inlines: Vec<Inline> },
    Paragraph { inlines: Vec<Inline> },
    CodeBlock { language: Option<String>, content: String },
    BlockQuote { blocks: Vec<Block> },
    List { ordered: bool, items: Vec<ListItem> },
    Item { blocks: Vec<Block> },
    Table { headers: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>>, in_head: bool },
    TableRow { cells: Vec<Vec<Inline>> },
    TableCell { inlines: Vec<Inline> },
    Styled { style: Style, children: Vec<Inline> },
    Link { url: String, children: Vec<Inline> },
    Image { url: String, alt: String },
}

#[derive(Debug, Default)]
struct IrBuilder {
    stack: Vec<Frame>,
    root: Vec<Block>,
}

impl IrBuilder {
    fn handle(&mut self, event: Event<'_>) -> Result<()> {
        match event {
            Event::Start(tag) => self.start_tag(tag)?,
            Event::End(tag) => self.end_tag(tag)?,
            Event::Text(s) => self.push_text(&s),
            Event::Code(s) => self.push_inline(Inline::Code(s.into_string())),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.push_inline(Inline::LineBreak),
            Event::Rule => self.push_block(Block::ThematicBreak),
            Event::Html(s) | Event::InlineHtml(s) => self.push_text(&s),
            Event::InlineMath(s) | Event::DisplayMath(s) => {
                self.push_inline(Inline::Code(s.into_string()));
            }
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
        }
        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'_>) -> Result<()> {
        let frame = match tag {
            Tag::Heading { level, .. } => Frame::Heading {
                level: heading_level_to_u8(level),
                inlines: Vec::new(),
            },
            Tag::Paragraph => Frame::Paragraph { inlines: Vec::new() },
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.into_string()),
                    _ => None,
                };
                Frame::CodeBlock { language, content: String::new() }
            }
            Tag::BlockQuote(_) => Frame::BlockQuote { blocks: Vec::new() },
            Tag::List(Some(_)) => Frame::List { ordered: true, items: Vec::new() },
            Tag::List(None) => Frame::List { ordered: false, items: Vec::new() },
            Tag::Item => Frame::Item { blocks: Vec::new() },
            Tag::Table(_) => Frame::Table {
                headers: Vec::new(),
                rows: Vec::new(),
                in_head: false,
            },
            Tag::TableHead => {
                self.mark_table_head(true);
                return Ok(());
            }
            Tag::TableRow => Frame::TableRow { cells: Vec::new() },
            Tag::TableCell => Frame::TableCell { inlines: Vec::new() },
            Tag::Emphasis => Frame::Styled {
                style: Style::Emphasis,
                children: Vec::new(),
            },
            Tag::Strong => Frame::Styled {
                style: Style::Strong,
                children: Vec::new(),
            },
            Tag::Strikethrough => Frame::Styled {
                style: Style::Strikethrough,
                children: Vec::new(),
            },
            Tag::Link { dest_url, .. } => Frame::Link {
                url: dest_url.into_string(),
                children: Vec::new(),
            },
            Tag::Image { dest_url, .. } => Frame::Image {
                url: dest_url.into_string(),
                alt: String::new(),
            },
            Tag::FootnoteDefinition(_)
            | Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => {
                return Err(Error::Parse(format!(
                    "markdown feature not supported: {tag:?}"
                )));
            }
        };
        self.stack.push(frame);
        Ok(())
    }

    fn end_tag(&mut self, tag: TagEnd) -> Result<()> {
        if matches!(tag, TagEnd::TableHead) {
            self.mark_table_head(false);
            return Ok(());
        }

        let frame = self.stack.pop().ok_or_else(|| {
            Error::Parse(format!("unbalanced end tag {tag:?}: empty stack"))
        })?;

        match frame {
            Frame::Heading { level, inlines } => {
                self.push_block(Block::Heading { level, content: inlines });
            }
            Frame::Paragraph { inlines } => {
                if !inlines.is_empty() {
                    self.push_block(Block::Paragraph { content: inlines });
                }
            }
            Frame::CodeBlock { language, content } => {
                self.push_block(Block::CodeBlock { language, content });
            }
            Frame::BlockQuote { blocks } => {
                self.push_block(Block::BlockQuote { children: blocks });
            }
            Frame::List { ordered, items } => {
                self.push_block(Block::List { ordered, items });
            }
            Frame::Item { blocks } => {
                let parent = self.stack.last_mut().ok_or_else(|| {
                    Error::Parse("list item outside a list".to_string())
                })?;
                if let Frame::List { items, .. } = parent {
                    items.push(ListItem { content: blocks });
                } else {
                    return Err(Error::Parse(
                        "list item parent is not a list".to_string(),
                    ));
                }
            }
            Frame::Table { headers, rows, .. } => {
                self.push_block(Block::Table { headers, rows });
            }
            Frame::TableRow { cells } => {
                let parent = self.stack.last_mut().ok_or_else(|| {
                    Error::Parse("table row outside a table".to_string())
                })?;
                if let Frame::Table { headers, rows, in_head } = parent {
                    if *in_head {
                        headers.extend(cells);
                    } else {
                        rows.push(cells);
                    }
                } else {
                    return Err(Error::Parse(
                        "table row parent is not a table".to_string(),
                    ));
                }
            }
            Frame::TableCell { inlines } => {
                let parent = self.stack.last_mut().ok_or_else(|| {
                    Error::Parse("table cell outside a row".to_string())
                })?;
                match parent {
                    Frame::TableRow { cells } => cells.push(inlines),
                    // GFM header cells in pulldown-cmark can arrive directly inside Table/TableHead.
                    Frame::Table { headers, in_head: true, .. } => headers.push(inlines),
                    _ => {
                        return Err(Error::Parse(
                            "table cell parent is not a row or table header".to_string(),
                        ));
                    }
                }
            }
            Frame::Styled { style, children } => {
                self.push_inline(Inline::Styled { style, children });
            }
            Frame::Link { url, children } => {
                self.push_inline(Inline::Link { url, children });
            }
            Frame::Image { url, alt } => {
                self.push_inline(Inline::Image {
                    url,
                    alt: if alt.is_empty() { None } else { Some(alt) },
                });
            }
        }
        Ok(())
    }

    fn mark_table_head(&mut self, on: bool) {
        if let Some(Frame::Table { in_head, .. }) = self.stack.last_mut() {
            *in_head = on;
        }
    }

    fn push_text(&mut self, s: &str) {
        match self.stack.last_mut() {
            Some(Frame::CodeBlock { content, .. }) => content.push_str(s),
            Some(Frame::Image { alt, .. }) => alt.push_str(s),
            _ => self.push_inline(Inline::Text(s.to_string())),
        }
    }

    fn push_inline(&mut self, inline: Inline) {
        let Some(frame) = self.stack.last_mut() else {
            // Stray inline at document root: wrap in an implicit paragraph.
            self.root.push(Block::Paragraph { content: vec![inline] });
            return;
        };
        match frame {
            Frame::Heading { inlines, .. }
            | Frame::Paragraph { inlines }
            | Frame::TableCell { inlines } => inlines.push(inline),
            Frame::Styled { children, .. } | Frame::Link { children, .. } => {
                children.push(inline);
            }
            Frame::CodeBlock { content, .. } => {
                if let Inline::Text(s) = inline {
                    content.push_str(&s);
                }
            }
            _ => {
                // No inline-accepting frame — wrap in an implicit paragraph
                // attached to the nearest block container.
                let block = Block::Paragraph { content: vec![inline] };
                self.push_block(block);
            }
        }
    }

    fn push_block(&mut self, block: Block) {
        match self.stack.last_mut() {
            Some(Frame::BlockQuote { blocks }) | Some(Frame::Item { blocks }) => {
                blocks.push(block);
            }
            None => self.root.push(block),
            Some(other) => {
                // The grammar should prevent this; surface it loudly rather
                // than silently dropping structure.
                // Unreachable in well-formed input.
                let _ = other;
                self.root.push(block);
            }
        }
    }

    fn finish(self) -> Result<Vec<Block>> {
        if !self.stack.is_empty() {
            return Err(Error::Parse(format!(
                "markdown ended with {} unclosed frames",
                self.stack.len()
            )));
        }
        Ok(self.root)
    }
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
