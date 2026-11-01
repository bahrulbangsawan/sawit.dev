//! Quitting and closing a window while a terminal runs something: the question names it, comes
//! forward with its window, and is never stacked.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext, VisualTestContext};

use super::super::{Ending, ask_before_closing, end_all};
use super::{start_sleep, try_quit};
use crate::terminal::testing::{init, open_panel, ready};

/// Quitting goes ahead at once while the shells wait at their prompts, and asks first while
/// one runs something, naming it: Cancel keeps the app, Quit quits.
#[gpui_kit::test]
fn quitting_asks_while_a_terminal_runs_something(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let quits = Rc::new(Cell::new(0));
    let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &shell);
    try_quit(cx, w, &panel, &quits);
    assert_eq!(quits.get(), 1, "an idle shell held up quitting");
    assert!(!cx.has_pending_prompt());

    start_sleep(cx, &panel);
    try_quit(cx, w, &panel, &quits);
    assert_eq!(quits.get(), 1, "quitting didn't ask");
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Quit Sawit?".to_string(),
            "A terminal is still running: sleep. Quitting stops it.".to_string()
        ))
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(quits.get(), 1, "Cancel quit");

    try_quit(cx, w, &panel, &quits);
    cx.simulate_prompt_answer("Quit");
    cx.run_until_parked();
    assert_eq!(quits.get(), 2);
}

/// The window the platform has in front.
fn front(cx: &mut TestAppContext) -> Option<AnyWindowHandle> {
    cx.update(|cx| cx.active_window())
}

/// Brings `w` to the front, as a click on it does.
fn bring_forward(cx: &mut TestAppContext, w: AnyWindowHandle) {
    cx.update_window(w, |_, window, _| window.activate_window())
        .expect("the test window is open");
    cx.run_until_parked();
}

/// With two windows, the question goes up in the one whose terminal runs something, behind
/// the idle one in front, and that window comes forward with it: a question on a window
/// behind is never seen, and ⌘Q seems to do nothing. ⌘Q again, from the front window, brings
/// the question forward again instead of stacking a second one.
#[gpui_kit::test]
fn the_question_comes_forward_with_its_window(cx: &mut TestAppContext) {
    cx.update(init);
    let (idle_window, idle) = open_panel(cx);
    let (busy_window, busy) = open_panel(cx);
    let shell = cx.update(|cx| idle.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &shell);
    start_sleep(cx, &busy);
    bring_forward(cx, idle_window);
    assert_eq!(front(cx), Some(idle_window));

    let quits = Rc::new(Cell::new(0));
    let quit = |cx: &mut TestAppContext| {
        let quits = quits.clone();
        let windows = vec![(idle_window, idle.clone()), (busy_window, busy.clone())];
        cx.update(|cx| {
            end_all(windows, Ending::Quit, cx, move |_| {
                quits.set(quits.get() + 1)
            })
        });
        cx.run_until_parked();
    };
    quit(cx);
    assert!(cx.has_pending_prompt(), "quitting didn't ask");
    assert_eq!(
        front(cx),
        Some(busy_window),
        "the question went up on the window behind"
    );

    bring_forward(cx, idle_window);
    quit(cx);
    assert_eq!(
        front(cx),
        Some(busy_window),
        "⌘Q again left the question behind"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "a second question was stacked");
    assert_eq!(quits.get(), 0, "Cancel quit");
}

/// Closing a window while the quit question is up, this window or another, keeps it open
/// and brings the question forward rather than stacking a second one.
#[gpui_kit::test]
fn closing_while_the_question_is_up_brings_it_forward(cx: &mut TestAppContext) {
    cx.update(init);
    let (asked, asked_panel) = open_panel(cx);
    let (other, other_panel) = open_panel(cx);
    for (w, panel) in [(asked, &asked_panel), (other, &other_panel)] {
        cx.update_window(w, |_, window, cx| {
            ask_before_closing(window, panel.downgrade(), cx)
        })
        .expect("the test window is open");
        start_sleep(cx, panel);
    }
    let windows = vec![(asked, asked_panel.clone()), (other, other_panel.clone())];
    cx.update(|cx| end_all(windows, Ending::Quit, cx, |_| {}));
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "quitting didn't ask");

    for (w, which) in [(other, "another window"), (asked, "the asked window")] {
        bring_forward(cx, other);
        let mut vcx = VisualTestContext::from_window(w, cx);
        assert!(!vcx.simulate_close(), "closing {which} didn't wait");
        cx.run_until_parked();
        assert_eq!(
            front(cx),
            Some(asked),
            "closing {which} left the question behind"
        );
    }
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "a second question was stacked");
    assert!(cx.windows().contains(&asked) && cx.windows().contains(&other));
}

/// Closing a window asks first while its terminals run something; Cancel keeps it open and
/// Close Window closes it.
#[gpui_kit::test]
fn closing_a_window_asks_while_a_terminal_runs_something(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    cx.update_window(w, |_, window, cx| {
        ask_before_closing(window, panel.downgrade(), cx)
    })
    .expect("the test window is open");
    let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &shell);
    let mut vcx = VisualTestContext::from_window(w, cx);
    assert!(vcx.simulate_close(), "a window of idle shells asked");
    assert!(!cx.has_pending_prompt());

    start_sleep(cx, &panel);
    let mut vcx = VisualTestContext::from_window(w, cx);
    assert!(!vcx.simulate_close(), "the window closed without asking");
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "Close this window?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(cx.windows().contains(&w), "Cancel closed the window");

    let mut vcx = VisualTestContext::from_window(w, cx);
    assert!(!vcx.simulate_close());
    cx.simulate_prompt_answer("Close Window");
    cx.run_until_parked();
    assert!(!cx.windows().contains(&w), "the window is still open");
}
