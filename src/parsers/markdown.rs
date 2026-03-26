use super::Parser;
use crate::ir::Document;

#[derive(Debug, Default)]
pub struct MarkdownParser;

impl Parser for MarkdownParser {
    fn parse(&self, _input: &str) -> Result<Document, String> {
        Err("markdown parser not implemented".to_string())
    }
}
