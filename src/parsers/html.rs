use crate::ir::Document;
use crate::{Error, Result};

/// Phase-3 target. Parsing HTML is not implemented yet.
#[derive(Debug, Default)]
pub struct HtmlParser;

impl HtmlParser {
    pub fn parse(&self, _input: &str) -> Result<Document> {
        Err(Error::NotImplemented("html parser"))
    }
}
