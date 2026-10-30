//! Bottom-dock terminal: tabs of real terminals (shell, herdr or SSH).

mod panel;
mod tab;
#[cfg(test)]
mod testing;
mod view;

use gpui_kit::actions;

pub use panel::{JobTag, Remote, TerminalPanel, ToggleMaximize};

actions!(
    terminal,
    [
        /// Splits the focused pane, with the new pane on the right.
        SplitRight,
        /// Splits the focused pane, with the new pane below.
        SplitDown,
        /// Focuses the pane to the left.
        FocusLeft,
        /// Focuses the pane to the right.
        FocusRight,
        /// Focuses the pane above.
        FocusUp,
        /// Focuses the pane below.
        FocusDown,
        /// Zooms the focused pane to fill its tab, or unzooms it. With one
        /// pane the key goes on to the workspace, which maximizes the dock.
        ZoomPane,
    ]
);

pub fn init(cx: &mut gpui_kit::App) {
    cx.bind_keys(view::bindings());
}
