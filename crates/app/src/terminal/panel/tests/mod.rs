//! The terminal panel's tests, by what they exercise, and the helpers they share.

mod control;
mod jobs;
mod run;
mod running;
mod split;
mod strip;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::super::tab::TerminalTab;
use super::TerminalPanel;

/// The active tab.
fn active(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>) -> Entity<TerminalTab> {
    cx.update(|cx| {
        let p = panel.read(cx);
        p.tab(p.active_index()).clone()
    })
}

/// Opens `n` more shell tabs; the last one opened is the active one.
fn open_tabs(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>, n: usize) {
    for _ in 0..n {
        cx.update(|cx| panel.update(cx, |p, cx| p.open_shell(None, cx)));
    }
    cx.run_until_parked();
}

/// Makes tab `ix` the active one with the keyboard, as a click on it does.
fn click_tab(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
    ix: usize,
) {
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.activate(ix, window, cx))
    })
    .expect("the test window is open");
    cx.run_until_parked();
}

/// Whether a pane of `tab` has the keyboard.
fn has_keyboard(cx: &mut TestAppContext, w: AnyWindowHandle, tab: &Entity<TerminalTab>) -> bool {
    cx.update_window(w, |_, window, cx| tab.read(cx).has_focus(window, cx))
        .expect("the test window is open")
}

/// A debug selector made at run time: tests only, so leaking it is fine.
fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A stand-in SSH server: a session that ends once it reads a line.
fn test_remote() -> super::Remote {
    super::Remote {
        key: "test".into(),
        name: "test".into(),
        launch: sawit_core::ssh::Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "read line".into()],
            env: Vec::new(),
        },
    }
}
