use crate::Result;
use crate::ir::Document;

pub trait Parser {
    fn parse(&self, input: &str) -> Result<Document>;
}

/// Defines a unit-struct parser whose `parse` returns `Error::NotImplemented`.
macro_rules! parser_stub {
    ($name:ident, $label:expr) => {
        #[derive(Debug, Default)]
        pub struct $name;

        impl $crate::parsers::Parser for $name {
            fn parse(&self, _input: &str) -> $crate::Result<$crate::ir::Document> {
                Err($crate::Error::NotImplemented(concat!($label, " parser")))
            }
        }
    };
}
pub(crate) use parser_stub;

pub mod html;
pub mod markdown;
pub mod pdf;
