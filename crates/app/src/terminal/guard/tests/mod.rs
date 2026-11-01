//! The tests of asking before Sawit ends terminals, by what the question is about, and the helpers
//! they share.

mod installs;
mod terminals;
mod unsaved;
mod wording;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::super::TerminalPanel;
use super::super::testing::{ready, start};
use super::{Ending, end_all};

/// Runs `sleep 30` in the first tab's shell, once it is ready, and waits until it runs.
fn start_sleep(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>) {
    let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &view);
    start(cx, &view, "sleep 30", "sleep");
}

/// Counts how often `end_all` went on (quit), asked about `panel`'s window.
fn try_quit(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
    quits: &Rc<Cell<u32>>,
) {
    let quits = quits.clone();
    let windows = vec![(w, panel.clone())];
    cx.update(|cx| {
        end_all(windows, Ending::Quit, cx, move |_| {
            quits.set(quits.get() + 1)
        })
    });
    cx.run_until_parked();
}
