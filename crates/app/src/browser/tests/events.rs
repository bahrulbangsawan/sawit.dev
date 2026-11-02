//! What the page reports: messages that change nothing the view draws redraw nothing.

use serde_json::json;

use super::test_browser;
use crate::browser;

/// A page that logs or fetches all the time, or is hovered over, doesn't keep the window
/// redrawing: its console and network entries and its hovers change nothing the view draws (the
/// panels redraw themselves). They are still kept, and a title still redraws.
#[gpui_kit::test]
fn logs_and_hovers_redraw_nothing(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&view, move |_, _| notified.set(notified.get() + 1))
            .detach();
    });
    let (token, to_view) = cx.update(|_, cx| {
        let b = view.read(cx);
        (b.token.clone(), b.to_view.clone())
    });
    let ipc = |kind: &str, body: serde_json::Value| {
        browser::FromPage::Engine(browser::EngineEvent::Ipc(format!(
            "{token}\n{kind}\n{body}"
        )))
    };
    for i in 0..100 {
        let console = json!({"level": "log", "text": format!("tick {i}")});
        let network = json!({"method": "GET", "url": format!("https://a.dev/{i}"),
            "kind": "fetch", "status": 200});
        for message in [
            ipc("console", console),
            ipc("network", network),
            ipc("hovered", json!({"selector": format!("#e{i}")})),
        ] {
            to_view.unbounded_send(message).unwrap();
        }
    }
    cx.run_until_parked();
    let logs = cx.update(|_, cx| {
        let b = view.read(cx);
        (b.logs.console.len(), b.logs.network.len())
    });
    assert_eq!(logs, (100, 100));
    assert_eq!(notified.get(), 0);
    to_view
        .unbounded_send(browser::FromPage::Engine(browser::EngineEvent::Title(
            "Shop".into(),
        )))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(notified.get(), 1, "a title redraws");
}

/// A load that fails before its page commits takes WebKit's address back with no event: the
/// page's next log, request or hover draws the toolbar again, so that the Explorer button follows
/// the address, and draws nothing more once the toolbar has it.
#[gpui_kit::test]
fn a_page_message_redraws_a_toolbar_drawn_for_another_address(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&view, move |_, _| notified.set(notified.get() + 1))
            .detach();
    });
    let (token, to_view) = cx.update(|_, cx| {
        let b = view.read(cx);
        (b.token.clone(), b.to_view.clone())
    });
    let log = || {
        let entry = json!({"level": "log", "text": "tick"});
        browser::FromPage::Engine(browser::EngineEvent::Ipc(format!(
            "{token}\nconsole\n{entry}"
        )))
    };
    // Drawn while a load to a port nobody serves was on its way; WebKit's address (the view's
    // own here, as a test window has no webview) is the page's again.
    cx.update(|_, cx| {
        view.update(cx, |b, _| {
            b.drawn_url = Some("http://localhost:9999/".into())
        })
    });
    to_view.unbounded_send(log()).unwrap();
    cx.run_until_parked();
    assert_eq!(notified.get(), 1, "drawn again");
    cx.update(|_, cx| view.update(cx, |b, _| b.drawn_url = Some(b.current_url())));
    to_view.unbounded_send(log()).unwrap();
    cx.run_until_parked();
    assert_eq!(notified.get(), 1, "the toolbar has the address");
}
