//! The terminal's key bindings, and keys, pastes and modes as the program reads them.

use gpui_kit::*;
use sawit_core::terminal::{KeyInput, key_to_bytes, paste_bytes};

use super::TerminalView;
use crate::terminal::{
    ClosePane, FocusDown, FocusLeft, FocusNextPane, FocusNextPaneAtPrompt, FocusPreviousPane,
    FocusRight, FocusUp, RenamePane, RenameTab, SplitDown, SplitRight, ToggleSendRightClicks,
    ZoomPane,
};

/// Key bindings run before `on_key_down`, so an outer binding for a key the
/// shell needs would swallow it: Root's Tab/Shift-Tab move focus out of the
/// terminal (and Tab is completion), Root's Cmd-C copies a stale selection
/// elsewhere in the window. Unbinding them here hands them to `on_key`.
/// App shortcuts (Ctrl-Tab, Cmd-…) stay with the workspace. The pane keys
/// (split, move focus, zoom, and the rest of the pane and tab menus) live
/// here too, so they only act in a terminal. Renaming follows Terminal.app's
/// Edit Title (⇧⌘I, the tab) and iTerm2's Edit Session (⌘I, the pane). ⌘W
/// stays the workspace's (it closes the center tab), so a pane closes with
/// ⌥⌘W. ⌘] and ⌘[ step through the panes as in iTerm2.
///
/// Shift-Tab is the one key a program gets that the terminal also uses: at a
/// shell prompt in a split tab it moves to the next pane, and anywhere else its
/// action lets it go on to `on_key`. It is bound after its `NoAction`, so it
/// comes first, while the `NoAction` still keeps Root's Shift-Tab out.
pub(in crate::terminal) fn bindings() -> Vec<KeyBinding> {
    let ctx = Some("Terminal");
    vec![
        KeyBinding::new("tab", NoAction, ctx),
        KeyBinding::new("shift-tab", NoAction, ctx),
        KeyBinding::new("shift-tab", FocusNextPaneAtPrompt, ctx),
        KeyBinding::new("cmd-c", NoAction, ctx),
        KeyBinding::new("cmd-d", SplitRight, ctx),
        KeyBinding::new("cmd-shift-d", SplitDown, ctx),
        KeyBinding::new("cmd-alt-left", FocusLeft, ctx),
        KeyBinding::new("cmd-alt-right", FocusRight, ctx),
        KeyBinding::new("cmd-alt-up", FocusUp, ctx),
        KeyBinding::new("cmd-alt-down", FocusDown, ctx),
        KeyBinding::new("cmd-]", FocusNextPane, ctx),
        KeyBinding::new("cmd-[", FocusPreviousPane, ctx),
        KeyBinding::new("cmd-shift-enter", ZoomPane, ctx),
        KeyBinding::new("cmd-i", RenamePane, ctx),
        KeyBinding::new("cmd-shift-i", RenameTab, ctx),
        KeyBinding::new("cmd-alt-r", ToggleSendRightClicks, ctx),
        KeyBinding::new("cmd-alt-w", ClosePane, ctx),
    ]
}

impl TerminalView {
    pub(super) fn modes(&self) -> sawit_core::terminal::Modes {
        self.session.modes()
    }

    /// Pastes `text` as the program asked (bracketed when it enabled that).
    pub(crate) fn paste(&mut self, text: &str) {
        self.touched = true;
        self.write(paste_bytes(text, self.modes()));
    }

    /// The program enabled bracketed paste, so a paste never runs as commands.
    pub(crate) fn accepts_paste(&self) -> bool {
        self.modes().bracketed_paste && !self.exited
    }

    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        if m.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                self.paste(&text);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if m.platform && ks.key == "c" {
            if let Some(text) = self.session.selection_text() {
                self.copy(text, cx);
                cx.stop_propagation();
            }
            return;
        }
        let input = KeyInput {
            key: ks.key.clone(),
            text: ks.key_char.clone(),
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            cmd: m.platform,
        };
        if let Some(bytes) = key_to_bytes(&input, self.modes()) {
            self.touched = true;
            self.write(bytes);
            cx.notify();
            cx.stop_propagation();
            window.prevent_default();
        }
    }
}
