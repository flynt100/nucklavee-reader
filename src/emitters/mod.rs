use crate::ir::Document;

pub trait Emitter {
    fn emit(&self, document: &Document) -> Result<String, String>;
}

/// Generates a unit-struct emitter stub whose `emit` returns a not-implemented error.
/// Used by format-specific scaffold modules; real implementations will replace these.
macro_rules! emitter_stub {
    ($name:ident, $label:expr) => {
        #[derive(Debug, Default)]
        pub struct $name;

        impl $crate::emitters::Emitter for $name {
            fn emit(&self, _document: &$crate::ir::Document) -> Result<String, String> {
                Err(concat!($label, " emitter not implemented").to_string())
            }
        }
    };
}
pub(crate) use emitter_stub;

pub mod html;
pub mod markdown;
pub mod text;
