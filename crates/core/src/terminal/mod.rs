//! Terminal emulation (PTY + alacritty_terminal) without any UI dependency,
//! so it can be tested headlessly. The app only draws `Snapshot`s.

mod input;
mod layout;
mod palette;
mod session;
mod tab;

pub use input::{
    KeyInput, Modes, MouseAction, MouseButton, ScrollAccumulator, cd_bytes, drop_bytes,
    is_shell_lang, key_to_bytes, mouse_report, paste_bytes, run_bytes, shell_commands, shell_quote,
};
pub use layout::{
    Direction, Divider, Layout, PaneId, Rect, Removed, SplitDir, SplitPath, new_pane_id,
};
pub use palette::{Palette, Rgb};
pub use session::{
    CellView, Command, CursorView, GridSize, SelectKind, Session, Snapshot, TermEvent,
    default_shell, multiplexer_markers, wrap_unsetting,
};
pub use tab::{MIN_COLS, MIN_ROWS, PaneMeta, TabState};

#[cfg(test)]
mod tests;
