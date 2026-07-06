use crate::ir::Document;
use crate::{Error, Result};

/// Phase-6 target. Parsing PDF is not implemented yet.
#[derive(Debug, Default)]
pub struct PdfParser;

impl PdfParser {
    pub fn parse(&self, _input: &str) -> Result<Document> {
        Err(Error::NotImplemented("pdf parser"))
    }
}
