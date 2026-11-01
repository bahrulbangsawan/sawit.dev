//! Bottom-dock terminal: tabs of real terminals (shell or SSH).

mod guard;
mod panel;
mod tab;
#[cfg(test)]
mod testing;
mod view;

use gpui_kit::actions;

pub use guard::{Ending, ask_before_closing, end_all};
pub use panel::{JobTag, Origin, Remote, TerminalPanel, ToggleMaximize};

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
        /// Names the focused pane, in its header. A tab of one pane has no
        /// header: the tab is named instead, as `RenameTab` does.
        RenamePane,
        /// Names the tab, in the tab strip: the name stays whichever pane
        /// has focus.
        RenameTab,
        /// Sends the focused pane's right-clicks to its program instead of
        /// opening the pane menu, or stops sending them.
        ToggleSendRightClicks,
        /// Closes the focused pane, ending what runs there; the last pane
        /// closes the tab.
        ClosePane,
    ]
);

pub fn init(cx: &mut gpui_kit::App) {
    cx.bind_keys(view::bindings());
}
