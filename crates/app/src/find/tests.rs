//! The find bar's state: what ⌘F seeds it with, what it tells the view, what it shows, and the
//! keyboard going back to the view when it closes.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{
    Context, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
};
use sawit_core::find::FindState;

use super::host::Finder;
use super::{FindBar, FindEvent, Supports};

/// A view with something to find in: the keyboard's place before the bar opens, and what the bar
/// told it.
struct Host {
    focus: FocusHandle,
    finder: Finder,
    heard: Rc<RefCell<Vec<FindEvent>>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .children(self.finder.render())
    }
}

fn host(cx: &mut TestAppContext) -> (Entity<Host>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(400.), px(300.)), |_, cx| Host {
        focus: cx.focus_handle(),
        finder: Finder::default(),
        heard: Rc::default(),
    });
    let view = window.root(cx).unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (view, cx)
}

fn open(view: &Entity<Host>, seed: Option<&str>, cx: &mut VisualTestContext) -> Entity<FindBar> {
    let bar = cx.update(|window, cx| {
        view.update(cx, |host, cx| {
            let heard = host.heard.clone();
            host.finder.open(
                Some(Supports::ALL),
                seed,
                false,
                move |_: &mut Host, event, _, _| heard.borrow_mut().push(event.clone()),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    bar
}

#[gpui_kit::test]
fn the_bar_takes_a_selection_counts_and_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (view, mut cx) = host(cx);
    let focus = view.read_with(&cx, |host, _| host.focus.clone());
    cx.update(|window, cx| window.focus(&focus, cx));

    // ⌘F with "needle" selected: the bar's query, and the view is told to find it.
    let bar = open(&view, Some("needle"), &mut cx);
    let heard = view.read_with(&cx, |host, _| host.heard.clone());
    assert_eq!(bar.read_with(&cx, |bar, cx| bar.query(cx)), "needle");
    assert_eq!(heard.borrow().as_slice(), &[FindEvent::Changed]);
    assert!(cx.update(|window, cx| bar.read(cx).has_focus(window, cx)));
    // A selection over several lines is no query: the last one stays.
    open(&view, Some("two\nlines"), &mut cx);
    assert_eq!(bar.read_with(&cx, |bar, cx| bar.query(cx)), "needle");

    // The view's count, as the bar shows it.
    bar.update(&mut cx, |bar, cx| {
        bar.set_found(FindState::found(12, Some(2)), None, cx)
    });
    assert_eq!(bar.read_with(&cx, |bar, cx| bar.status(cx)), "3 of 12");
    bar.update(&mut cx, |bar, cx| {
        bar.set_found(
            FindState::default(),
            Some("Invalid regular expression".into()),
            cx,
        )
    });
    assert_eq!(
        bar.read_with(&cx, |bar, cx| bar.status(cx)),
        "Invalid regular expression"
    );

    // An option toggled asks the view to find again, with it.
    heard.borrow_mut().clear();
    bar.update(&mut cx, |bar, cx| bar.toggle(|o| &mut o.whole_word, cx));
    assert!(bar.read_with(&cx, |bar, _| bar.options().whole_word));
    assert_eq!(heard.borrow().as_slice(), &[FindEvent::Changed]);

    // Escape's close: the keyboard is back in the view.
    let closed = cx.update(|window, cx| {
        view.update(cx, |host, cx| {
            let fallback = host.focus.clone();
            host.finder.close(&fallback, window, cx)
        })
    });
    assert!(closed);
    assert!(cx.update(|window, _| focus.is_focused(window)));
    assert!(view.read_with(&cx, |host, _| !host.finder.is_open()));
}

#[gpui_kit::test]
fn replace_shows_only_where_the_view_can_replace(cx: &mut TestAppContext) {
    let (view, mut cx) = host(cx);
    let bar = open(&view, None, &mut cx);
    bar.update_in(&mut cx, |bar, window, cx| bar.open(None, true, window, cx));
    assert!(
        !bar.read_with(&cx, |bar, _| bar.replacing),
        "Supports::ALL can't replace"
    );
    bar.update_in(&mut cx, |bar, window, cx| {
        bar.set_can_replace(true, cx);
        bar.open(None, true, window, cx)
    });
    assert!(bar.read_with(&cx, |bar, _| bar.replacing));
    // A tab made read-only takes the Replace row away.
    bar.update(&mut cx, |bar, cx| bar.set_can_replace(false, cx));
    assert!(!bar.read_with(&cx, |bar, _| bar.replacing));
}
