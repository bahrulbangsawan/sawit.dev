//! Shared setup for the terminal's GPUI tests.

use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity, TestAppContext};

use super::TerminalPanel;

/// Registers the actions and key bindings the terminal tests rely on.
pub(super) fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::workspace::init(cx);
    super::init(cx);
}

/// Opens a headless window whose root is a fresh `TerminalPanel` (one
/// shell tab, started in `$HOME`).
pub(super) fn open_panel(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<TerminalPanel>) {
    let opened = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |_, cx| cx.new(TerminalPanel::new))
            .expect("the test window opens")
    });
    cx.run_until_parked();
    opened
}
