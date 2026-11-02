//! The browser tab's tests, by what they test, with the tab in a test window that they share.

mod keyboard;
mod navigation;
mod notes;
mod ops;
mod picking;
mod shots;
mod viewport;

use super::new_token;

#[test]
fn tokens_are_random_hex() {
    let (a, b) = (new_token(), new_token());
    assert_eq!(a.len(), 32);
    assert_ne!(a, b);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
}

/// A browser tab that is never drawn (so it has no webview), in a test window.
fn test_browser(
    cx: &mut gpui_kit::TestAppContext,
) -> (
    gpui_kit::Entity<super::BrowserView>,
    &mut gpui_kit::VisualTestContext,
) {
    use gpui_kit::AppContext as _;
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| cx.new(|cx| super::BrowserView::new(None, None, window, cx)));
    (view, cx)
}

/// What the engine reports, as the view gets it.
fn engine_event(
    view: &gpui_kit::Entity<super::BrowserView>,
    event: super::EngineEvent,
    cx: &mut gpui_kit::VisualTestContext,
) {
    cx.update(|window, cx| {
        view.update(cx, |b, cx| {
            b.receive(super::FromPage::Engine(event), window, cx)
        })
    });
    cx.run_until_parked();
}
