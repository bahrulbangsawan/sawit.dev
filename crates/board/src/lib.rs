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

/// Has the system's fonts indexed the first time a board lays out text, for a process with no app
/// to scan them in the background (`sawit-mcp`'s headless path). Sawit embeds no font, so without
/// it that text would come out with no glyphs. Harmless to call more than once.
pub fn scan_system_fonts_on_demand() {
    figcraft_text::scan_system_fonts_on_demand();
}
