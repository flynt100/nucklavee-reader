use super::Parser;
use crate::ir::Document;

#[derive(Debug, Default)]
pub struct HtmlParser;

impl Parser for HtmlParser {
    fn parse(&self, _input: &str) -> Result<Document, String> {
        Err("html parser not implemented".to_string())
    }
}
