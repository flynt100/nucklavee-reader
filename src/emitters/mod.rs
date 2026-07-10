//! Output emitters. Each format exposes a free `emit_*` function that walks the
//! IR; there is no `Emitter` trait, because the `Library` dispatches on the
//! `Format` enum and nothing needs polymorphic emission.

pub mod html;
pub mod markdown;
pub mod text;
