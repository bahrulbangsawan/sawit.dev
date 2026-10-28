//! Terminal emulation (PTY + alacritty_terminal) without any UI dependency,
//! so it can be tested headlessly. The app only draws `Snapshot`s.

mod input;
mod palette;
mod session;

pub use input::{
    KeyInput, Modes, MouseAction, MouseButton, key_to_bytes, mouse_report, paste_bytes,
};
pub use palette::{Palette, Rgb};
pub use session::{
    CellView, Command, CursorView, GridSize, Session, Snapshot, TermEvent, multiplexer_markers,
    wrap_unsetting,
};

#[cfg(test)]
mod tests;
