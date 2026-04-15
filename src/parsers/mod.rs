use crate::Result;
use crate::ir::Document;

pub trait Parser {
    fn parse(&self, input: &str) -> Result<Document>;
}

/// Generates a unit-struct parser stub whose `parse` returns a not-implemented error.
/// Used by format-specific scaffold modules; real implementations will replace these.
macro_rules! parser_stub {
    ($name:ident, $label:expr) => {
        #[derive(Debug, Default)]
        pub struct $name;

        impl $crate::parsers::Parser for $name {
            fn parse(&self, _input: &str) -> $crate::Result<$crate::ir::Document> {
                Err($crate::Error::NotImplemented(concat!(
                    $label,
                    " parser"
                )))
            }
        }
    };
}
pub(crate) use parser_stub;

pub mod html;
pub mod markdown;
pub mod pdf;
