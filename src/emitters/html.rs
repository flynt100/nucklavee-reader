use super::Emitter;
use crate::ir::Document;

#[derive(Debug, Default)]
pub struct HtmlEmitter;

impl Emitter for HtmlEmitter {
    fn emit(&self, _document: &Document) -> Result<String, String> {
        Err("html emitter not implemented".to_string())
    }
}
