use super::Emitter;
use crate::ir::Document;

#[derive(Debug, Default)]
pub struct PlainTextEmitter;

impl Emitter for PlainTextEmitter {
    fn emit(&self, _document: &Document) -> Result<String, String> {
        Err("plain text emitter not implemented".to_string())
    }
}
