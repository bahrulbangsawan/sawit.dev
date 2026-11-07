//! Bottom-dock terminal: tabs of real terminals (shell or SSH), each showing the coding agents
//! (Claude Code, Codex, …) running in it by their logos.

mod agents;
mod guard;
mod panel;
mod tab;
#[cfg(test)]
mod testing;
mod view;

use gpui_kit::actions;

pub use guard::{Ending, ask_before_closing, end_all};
pub use panel::{CloseScope, stop_all_now};
pub use panel::{JobTag, OpenFileRequest, Origin, Remote, TerminalPanel, ToggleMaximize};
// The agents running in the terminals, and their logos, for the browser's Send to agent.
pub use {agents::logo_in as agent_logo, panel::RunningAgent};

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
        /// Focuses the next pane in reading order (left to right, then top
        /// to bottom), wrapping round.
        FocusNextPane,
        /// Focuses the previous pane in reading order, wrapping round.
        FocusPreviousPane,
        /// At a shell prompt in a split tab (no program in the foreground),
        /// focuses the next pane as `FocusNextPane` does. Anywhere else the
        /// key goes on to the program: Claude Code switches modes with ⇧⇥.
        FocusNextPaneAtPrompt,
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
        /// Copies the output of the last command in the focused pane as plain text.
        CopyLastOutput,
        /// Copies the error lines (with a few around each) of the last command's output.
        CopyError,
        /// Copies the focused pane's selection.
        Copy,
        /// Pastes into the focused pane, as typed (bracketed when its program asks).
        Paste,
        /// Selects the focused pane's whole scrollback and screen.
        SelectAll,
        /// Clears the focused pane's scrollback and screen; a shell draws its prompt again.
        Clear,
        /// Resets the focused pane's terminal state (modes, colours, scrollback).
        ResetTerminal,
        /// Saves the focused pane's scrollback and screen to a text file.
        ExportOutput,
        /// Closes the active terminal tab (asking first while it runs something).
        CloseTab,
        /// Closes every terminal tab but the active one, keeping locked and pinned ones.
        CloseOtherTabs,
        /// Closes the terminal tabs left of the active one.
        CloseTabsLeft,
        /// Closes the terminal tabs right of the active one.
        CloseTabsRight,
        /// Closes the terminal tabs with nothing running, keeping locked and pinned ones.
        SafeCloseAll,
        /// Closes every terminal tab but the locked ones, after asking when one runs something.
        ForceCloseAll,
        /// Opens a new terminal in the active tab's folder, in its project.
        DuplicateTab,
        /// Locks the active terminal tab so nothing closes it, or unlocks it.
        ToggleLockTab,
        /// Pins the active terminal tab to the start of the strip, or unpins it.
        TogglePinTab,
    ]
);

pub fn init(cx: &mut gpui_kit::App) {
    cx.bind_keys(view::bindings());
}

/// The terminal's own key bindings, for the app-wide key tests (`crate::keys_tests`).
#[cfg(test)]
pub(crate) fn key_bindings() -> Vec<gpui_kit::KeyBinding> {
    view::bindings()
}
