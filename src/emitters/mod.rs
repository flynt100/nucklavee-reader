use crate::ir::Document;

pub trait Emitter {
    fn emit(&self, document: &Document) -> Result<String, String>;
}

pub mod html;
pub mod markdown;
pub mod text;
