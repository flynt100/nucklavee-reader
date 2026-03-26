use super::Parser;
use crate::ir::Document;

#[derive(Debug, Default)]
pub struct PdfParser;

impl Parser for PdfParser {
    fn parse(&self, _input: &str) -> Result<Document, String> {
        Err("pdf parser not implemented".to_string())
    }
}
