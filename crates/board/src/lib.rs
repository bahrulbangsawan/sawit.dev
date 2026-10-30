//! The native canvas's document logic around the FigCraft engine. Must not depend on GPUI so it
//! can be tested with plain `cargo test`; the GPUI views live in `crates/app/src/board/`.

pub mod actions;
pub mod clipboard;
pub mod context_bar;
pub mod design;
pub mod doc;
pub mod excalidraw;
pub mod input;
pub mod layers;
pub mod menu;
pub mod mermaid;
pub mod ops;
pub mod overlay;
pub mod policy;
pub mod shortcuts;
pub mod skeleton;
pub mod store;
pub mod summary;
pub mod toolbar;
pub mod view;

pub use doc::{BoardDoc, BoardError, DocKind};
