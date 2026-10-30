//! Shared setup for the terminal's GPUI tests.

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use super::TerminalPanel;

/// Registers the actions and key bindings the terminal tests rely on.
pub(super) fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::workspace::init(cx);
    super::init(cx);
}

/// Opens a 1200×800 headless window (big enough to split) whose root is a
/// fresh `TerminalPanel` (one shell tab, started in `$HOME`). The shells are
/// real processes whose output arrives from reader threads, so the test
/// scheduler is told to expect wakeups from other threads.
pub(super) fn open_panel(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<TerminalPanel>) {
    cx.executor().allow_parking();
    let opened = cx.update(|cx| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(TerminalPanel::new))
            .expect("the test window opens")
    });
    cx.run_until_parked();
    opened
}

/// Gives the keyboard to the active tab's focused pane, so simulated keys
/// reach the terminal's key context.
pub(super) fn focus_terminal(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
) {
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |p, cx| p.focus_active(window, cx))
    })
    .expect("the test window is open");
    cx.run_until_parked();
}
