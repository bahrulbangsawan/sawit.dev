//! The canvas's tab going off screen and coming back: the keyboard it gives the workspace and
//! takes back, and what it holds of the pointer and its frames, let go of while it is hidden.

use gpui_kit::*;

use super::BoardView;

impl BoardView {
    /// The canvas takes the keyboard with the next frame it is drawn in, as the canvas the user
    /// went to (CANVAS-UI-2): the workspace asks for it on the user's ways in, ⌃⌘C and the title
    /// bar's Canvas button, a canvas file opened, a click on the canvas's tab and ⌃Tab, as the
    /// Excalidraw canvas's webview took the keyboard each time it came on screen. Until then the
    /// canvas's keys went to whatever had the keyboard: a terminal pane's shell, or the Files
    /// tree, whose ⌫ moves the selected file to the Trash. The keyboard goes to the palette, the
    /// shortcuts sheet or the context menu instead, when one is open over the canvas. A canvas
    /// taken off screen before that frame takes nothing ([`Self::set_shown`]).
    pub fn take_keyboard(&mut self, cx: &mut Context<Self>) {
        self.wants_keyboard = true;
        cx.notify();
    }

    /// The workspace shows the canvas's tab (`shown`), or no longer does: another tab is on top,
    /// or the tab area is hidden. It says so each time it draws, for each canvas it has.
    ///
    /// Hidden, nothing paints the canvas, and the window's listeners that give a drag, a pan or a
    /// label's scrub begun on it its moves and its release go with the paint. Each one held ends
    /// then, kept, where the pointer was last ([`Self::let_go_of_the_pointer`], CANVAS-UI-8):
    /// otherwise the tools stayed busy until the next press on the canvas, and refused every
    /// agent's edit meanwhile ("finish the drag on the canvas first"). What it ended is saved as
    /// any edit is, though no paint starts the autosave while the canvas is hidden.
    ///
    /// Hidden, it also lets go of what it keeps to draw its next frames ([`Self::let_go_of_frames`],
    /// CANVAS-UI-9). And it gives the keyboard, if it or a field in it has it, to `home`, the
    /// workspace (CANVAS-UI-2): a keyboard left on what isn't drawn sends the keys nowhere, the
    /// app's own (⌃Tab, ⌘W) too. Shown again, it takes the keyboard back if `home` still has it.
    pub fn set_shown(
        &mut self,
        shown: bool,
        home: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if std::mem::replace(&mut self.shown, shown) == shown {
            return;
        }
        if shown {
            if std::mem::take(&mut self.gave_keyboard) && home.is_focused(window) {
                self.wants_keyboard = true;
            }
            cx.notify();
            return;
        }
        self.wants_keyboard = false;
        if self.focus.is_focused(window) || self.focus.contains_focused(window, cx) {
            window.focus(home, cx);
            self.gave_keyboard = true;
        }
        self.let_go_of_the_pointer(cx);
        self.autosave(cx);
        self.let_go_of_frames(window);
    }
}
