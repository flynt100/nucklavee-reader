use super::Emitter;
use crate::ir::Document;

#[derive(Debug, Default)]
pub struct MarkdownEmitter;

impl Emitter for MarkdownEmitter {
    fn emit(&self, _document: &Document) -> Result<String, String> {
        Err("markdown emitter not implemented".to_string())
    }
}
