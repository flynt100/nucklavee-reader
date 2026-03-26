use crate::ir::Document;

pub trait Parser {
    fn parse(&self, input: &str) -> Result<Document, String>;
}

pub mod html;
pub mod markdown;
pub mod pdf;
