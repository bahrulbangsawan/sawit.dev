//! Keys pressed while the canvas has the keyboard: FigCraft's shortcuts, which are [`Command`]
//! bindings, and the keys no binding took, which go to the tools and the text editor; then what
//! the key came to, carried out as its menu item would be.

use gpui_kit::*;
use sawit_board::input::{self, KeyOutcome};
use sawit_board::{BoardError, menu, policy};

use super::{BoardView, Command, PressedKey, commands, events};

impl BoardView {
    /// A key pressed while the canvas has the keyboard, which no key binding took: not one of
    /// FigCraft's shortcuts, which are [`Command`] bindings ([`Self::on_command`]). It goes to the
    /// tools, the text editor and the canvas's own keys ([`input::route_unbound_key`]), and then
    /// the canvas carries out what the engine asked of it, as after every input:
    ///
    /// - A key the canvas uses stops here. A command the engine refuses says why, in a toast.
    /// - Any other key goes on: while text is edited, typing reaches the input method.
    ///
    /// Space held down without modifiers, while no text is edited, arms a pan instead
    /// ([`events`]).
    ///
    /// The canvas takes only the keys pressed while it has the keyboard itself: a key typed in a
    /// field of the Design panel or in the Actions palette, inside the canvas's view, goes on to
    /// the field, and the menus and the shortcuts sheet take their own keys. While a label's
    /// drag scrubs a field, Escape puts the value back and no key runs a command, which would end
    /// up in the scrub's undo step (ruling P5-C3).
    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scrubbing_label() {
            if event.keystroke.key == "escape" {
                self.scrub_end(false, cx);
            }
            cx.stop_propagation();
            return;
        }
        if !self.focus.is_focused(window) {
            return;
        }
        if self.hold_space(event, cx) {
            cx.stop_propagation();
            return;
        }
        let keystroke = &event.keystroke;
        let Some(key) = input::key_name(&keystroke.key) else {
            return;
        };
        let Some(board) = self.board_mut() else {
            return;
        };
        let routed = input::route_unbound_key(board, &key, events::mods(keystroke.modifiers));
        self.finish_key(routed, window, cx);
    }

    /// A [`Command`] binding fired while the canvas has the keyboard: one of FigCraft's shortcuts,
    /// as the keymap binds it now. The command runs as its shortcut did ([`input::route_command`]):
    /// the tools have the key first, and while text is edited copy and paste act on the text. One
    /// the canvas has no use for lets the key go on, to [`Self::on_key`] and the app.
    ///
    /// As with [`Self::on_key`], only while the canvas has the keyboard itself (a field of the
    /// Design panel or the Actions palette types the key), and no command runs while a label's
    /// drag scrubs a field.
    pub(super) fn on_command(
        &mut self,
        command: &Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scrubbing_label() {
            let escape = cx
                .try_global::<PressedKey>()
                .and_then(|pressed| pressed.0.as_ref())
                .is_some_and(|k| k.key == "escape");
            if escape {
                self.scrub_end(false, cx);
            }
            cx.stop_propagation();
            return;
        }
        if !self.focus.is_focused(window) {
            cx.propagate();
            return;
        }
        let pressed = cx
            .try_global::<PressedKey>()
            .and_then(|pressed| pressed.0.as_ref())
            .and_then(|k| Some((input::key_name(&k.key)?, events::mods(k.modifiers))));
        let Some(board) = self.board_mut() else {
            cx.propagate();
            return;
        };
        // A command that needs what the canvas has no dialog to ask for (a link's URL) has
        // nothing to do from a key: the key does nothing, as one with nothing selected does.
        if menu::NEEDS_INPUT.contains(&command.id.as_str()) {
            cx.stop_propagation();
            return;
        }
        let key = pressed.as_ref().map(|(key, mods)| (key.as_str(), *mods));
        let routed = input::route_command(board, &command.id, key);
        if !self.finish_key(routed, window, cx) {
            cx.propagate();
        }
    }

    /// After a key or a command went to the canvas: carries out what the engine asked of it, then
    /// what the key came to. Whether the canvas used the key: if not, it goes on.
    ///
    /// The shortcut of a command Sawit carries out itself is carried out here as its menu item is:
    /// copy and paste ([`Self::run_clipboard`]), and import, save, export and copy as
    /// ([`commands`], ruling P6-C2).
    ///
    /// A key with nothing to act on does nothing and says nothing, as in FigCraft's own keyboard
    /// and in Figma: a command that doesn't apply now (`sawit_board::menu::applies`: Copy as text
    /// with nothing selected) doesn't run, and the engine's refusal of one that has nothing to act
    /// on (⌥⌘G with nothing selected) shows no toast (`BoardError::for_key`). Any other refusal
    /// says why, in plain words.
    fn finish_key(
        &mut self,
        routed: Result<KeyOutcome, BoardError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.handle_ui_requests(Vec::new(), cx);
        let used = match routed {
            Ok(KeyOutcome::Unhandled) => false,
            Ok(KeyOutcome::Handled) => true,
            Ok(KeyOutcome::Ui(id)) => {
                let applies = (self.board()).is_some_and(|board| menu::applies(&board.session, id));
                if !applies {
                    // Nothing to act on: the key is used, quietly.
                } else if policy::is_clipboard(id) {
                    self.run_clipboard(id, None, true, cx);
                } else {
                    commands::run_ui(self, id, window, cx);
                }
                true
            }
            // After the requests' toast, so that the error's is the one that shows.
            Err(error) => {
                if let Some(said) = error.for_key() {
                    self.show_toast(said, cx);
                }
                true
            }
        };
        if used {
            cx.stop_propagation();
        }
        cx.notify();
        used
    }
}
