//! The shared confirm question: Cancel is the default, Escape cancels, the red button goes ahead,
//! the typed name gates the worst actions, and the keyboard goes back where it was.

use futures::FutureExt as _;
use gpui_kit::InteractiveElement as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, Entity, FocusHandle, IntoElement, Render,
    Styled as _, TestAppContext, Window, div,
};

use super::{Answer, Confirm};

/// A window's content that holds the keyboard before the question opens.
struct Host {
    focus: FocusHandle,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

/// A window whose content has the keyboard.
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Host>) {
    cx.update(gpui_kit::init);
    let (window, host) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Host {
                focus: cx.focus_handle(),
            })
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        window.activate_window();
        let focus = host.read(cx).focus.clone();
        focus.focus(window, cx);
    })
    .unwrap();
    cx.run_until_parked();
    (window, host)
}

fn ask(confirm: Confirm, window: AnyWindowHandle, cx: &mut TestAppContext) -> Answer {
    let answer = cx
        .update_window(window, |_, window, cx| confirm.ask(window, cx))
        .unwrap();
    cx.run_until_parked();
    answer
}

/// Presses `keys`, then lets the dialog's exit (and its deferred return of the keyboard) finish.
fn press(keys: &str, window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.simulate_keystrokes(window, keys);
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
}

/// What has the keyboard, for failure messages.
fn focused(window: AnyWindowHandle, cx: &mut TestAppContext) -> String {
    cx.update_window(window, |_, window, cx| format!("{:?}", window.focused(cx)))
        .unwrap()
}

fn open_dialog(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
        .unwrap()
}

fn host_focused(window: AnyWindowHandle, host: &Entity<Host>, cx: &mut TestAppContext) -> bool {
    cx.update_window(window, |_, window, cx| {
        host.read(cx).focus.is_focused(window)
    })
    .unwrap()
}

fn drop_db() -> Confirm {
    Confirm::new("Drop database shop_dev?", "Drop Database")
        .thing("shop_dev")
        .detail("This deletes all 14 tables.")
}

#[gpui_kit::test]
fn return_answers_cancel_and_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (window, host) = open(cx);
    let mut answer = ask(drop_db(), window, cx);
    assert!(open_dialog(window, cx), "the question didn't open");
    assert!(
        !host_focused(window, &host, cx),
        "the question doesn't have the keyboard"
    );
    press("enter", window, cx);
    assert_eq!(
        (&mut answer).now_or_never(),
        Some(false),
        "Return didn't cancel"
    );
    assert!(!open_dialog(window, cx), "the question is still up");
    assert!(
        host_focused(window, &host, cx),
        "the keyboard didn't come back"
    );
}

#[gpui_kit::test]
fn escape_cancels(cx: &mut TestAppContext) {
    let (window, host) = open(cx);
    let mut answer = ask(drop_db(), window, cx);
    press("escape", window, cx);
    assert_eq!((&mut answer).now_or_never(), Some(false));
    assert!(!open_dialog(window, cx));
    assert!(host_focused(window, &host, cx));
}

#[gpui_kit::test]
fn the_red_button_goes_ahead_once_tabbed_to(cx: &mut TestAppContext) {
    let (window, _) = open(cx);
    let mut answer = ask(drop_db(), window, cx);
    // Cancel has the keyboard; Tab moves it to the red button, and Space presses it.
    let before = focused(window, cx);
    press("tab", window, cx);
    assert_ne!(before, focused(window, cx), "Tab didn't move the keyboard");
    let on_go = cx.update(|cx| {
        let asked = cx.global::<super::testing::Asked>();
        let view = asked.0.last().unwrap().1.upgrade().unwrap();
        view.read(cx).go_focus.clone()
    });
    let on_go = cx
        .update_window(window, |_, window, _| on_go.is_focused(window))
        .unwrap();
    assert!(
        on_go,
        "Tab didn't reach the red button: {}",
        focused(window, cx)
    );
    press("space", window, cx);
    assert_eq!((&mut answer).now_or_never(), Some(true));
    assert!(!open_dialog(window, cx));
}

#[gpui_kit::test]
fn return_on_the_red_button_still_cancels(cx: &mut TestAppContext) {
    let (window, _) = open(cx);
    let mut answer = ask(drop_db(), window, cx);
    press("tab", window, cx);
    press("enter", window, cx);
    assert_eq!((&mut answer).now_or_never(), Some(false));
}

#[gpui_kit::test]
fn a_typed_name_gates_the_red_button(cx: &mut TestAppContext) {
    let (window, host) = open(cx);
    let mut answer = ask(drop_db().type_to_confirm(), window, cx);
    // The field has the keyboard; Return with a wrong name does nothing.
    press("s h o p enter", window, cx);
    assert_eq!(
        (&mut answer).now_or_never(),
        None,
        "a partial name answered"
    );
    assert!(
        open_dialog(window, cx),
        "a partial name closed the question"
    );
    // The whole name turns the red button on, and Return goes ahead.
    press("_ d e v enter", window, cx);
    assert_eq!((&mut answer).now_or_never(), Some(true));
    assert!(!open_dialog(window, cx));
    assert!(host_focused(window, &host, cx));
}

#[gpui_kit::test]
fn escape_cancels_a_typed_question_too(cx: &mut TestAppContext) {
    let (window, _) = open(cx);
    let mut answer = ask(drop_db().type_to_confirm(), window, cx);
    press("s h o p _ d e v escape", window, cx);
    assert_eq!((&mut answer).now_or_never(), Some(false));
}

#[test]
fn long_lists_are_cut_and_counted() {
    let items: Vec<String> = (1..=9).map(|n| format!("bun dev {n}")).collect();
    let confirm = Confirm::new("Close 9 terminals?", "Close Terminals").items(items);
    let listed = confirm.listed();
    assert_eq!(listed.len(), 7);
    assert_eq!(listed.last().unwrap().as_ref(), "and 3 more");
    assert!(Confirm::new("t", "v").thing("x").typed_name().is_none());
    assert_eq!(
        drop_db()
            .type_to_confirm()
            .typed_name()
            .map(|s| s.to_string()),
        Some("shop_dev".into())
    );
}
