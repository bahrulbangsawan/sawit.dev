//! The native canvas's document logic around the FigCraft engine. Must not depend on GPUI so it
//! can be tested with plain `cargo test`; the GPUI views live in `crates/app/src/board/`.

pub mod doc;
pub mod store;
pub mod view;

pub use doc::{BoardDoc, BoardError, DocKind};
