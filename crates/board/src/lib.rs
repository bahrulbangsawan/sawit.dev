//! The native canvas's document logic around the FigCraft engine. Must not depend on GPUI so it
//! can be tested with plain `cargo test`; the GPUI views live in `crates/app/src/board/`.

pub mod context_bar;
pub mod design;
pub mod doc;
pub mod input;
pub mod layers;
pub mod policy;
pub mod skeleton;
pub mod store;
pub mod toolbar;
pub mod view;

pub use doc::{BoardDoc, BoardError, DocKind};
