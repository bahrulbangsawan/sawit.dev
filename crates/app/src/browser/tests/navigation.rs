//! Opening pages: addresses WebKit reads, new windows, and links to other apps.

use super::{engine_event, test_browser};
use crate::browser;
use crate::browser::navigation::address_url;

/// Whatever is typed (or an agent asks for) is loaded as a URL WebKit reads, or searched:
/// wry unwraps the `NSURL` it makes of the address, so anything else aborted Sawit.
#[test]
fn every_address_is_one_webkit_reads() {
    for typed in [
        "localhost:3000:3000",
        "http://localhost 3000",
        "https://my site.com",
        "https://foo:exa%7Cmple.com",
        "https://::1",
        "file://my host/x",
        "file://a:1:2/x",
        "about://a:1:2",
        "localhost:5173",
        "example.com/a b",
        "https://example.com/a b",
        "file:///tmp/a b.html",
        "mailto:a@b.c",
        "rust gpui",
        "",
    ] {
        let url = address_url(typed);
        assert!(browser::engine::loadable(&url), "{typed} → {url}");
    }
    for bad in [
        "https://localhost:3000:3000",
        "https://my site.com",
        "file://my host/x",
    ] {
        assert!(!browser::engine::loadable(bad), "{bad}");
    }
    assert_eq!(
        address_url("file://my host/x"),
        sawit_core::browser::search_url("file://my host/x")
    );
}

/// `window.open()` and `target=_blank` links: web pages open in the tab as WebKit gives
/// them; a blank window (`window.open()`, then `w.location = …`) leaves the page alone;
/// links to other apps are asked about; a local file opens only from a local page, and a
/// page's own `blob:` only from that page.
#[test]
fn new_windows() {
    use browser::navigation::NewWindow::{External, Ignore, Load};
    use browser::navigation::new_window_action as act;
    let page = "http://localhost:5173/cart";
    let load = |u: &str| Load(u.to_string());
    let ext = |u: &str| External(u.to_string());
    for (url, want) in [
        ("", Ignore),
        ("about:blank", Ignore),
        ("about:srcdoc", Ignore),
        (
            "https://accounts.example/login?next=/",
            load("https://accounts.example/login?next=/"),
        ),
        ("http://localhost:5173/x", load("http://localhost:5173/x")),
        ("mailto:a@b.c", ext("mailto:a@b.c")),
        ("tel:+15551234", ext("tel:+15551234")),
        ("slack://open?team=T1", ext("slack://open?team=T1")),
        // `localhost:` is a scheme here, as WebKit read it: no app has it.
        ("localhost:3000:3000", ext("localhost:3000:3000")),
        ("file:///etc/hosts", Ignore),
        ("data:text/html,<p>hi</p>", Ignore),
        ("javascript:alert(1)", Ignore),
        (
            "blob:http://localhost:5173/9d2c",
            load("blob:http://localhost:5173/9d2c"),
        ),
        ("blob:https://other.example/9d2c", Ignore),
        ("https://my site.com", Ignore),
    ] {
        assert_eq!(act(url, page), want, "{url:?}");
    }
    assert_eq!(
        act("file:///tmp/b.html", "file:///tmp/a.html"),
        load("file:///tmp/b.html")
    );
}

#[test]
fn links_to_other_apps_are_asked_about_one_at_a_time() {
    let mut gate = browser::navigation::ExternalGate::default();
    assert!(gate.ask(), "the first link asks");
    assert!(!gate.ask(), "a second, while asking, is dropped");
    gate.answered(true);
    assert!(gate.ask(), "after an Open, the next link asks");
    gate.answered(false);
    assert!(!gate.ask(), "after a Cancel, the page asks no more");
    gate.reset();
    assert!(gate.ask(), "until a page is opened");
}

/// A page's link to another app (a `mailto:`, or an iframe's `slack://` with no click)
/// opens only when the user says so, through the system (no process left behind); while
/// asked, and after a Cancel, the page's further links are dropped.
#[gpui_kit::test]
fn links_to_other_apps_open_only_when_the_user_agrees(cx: &mut gpui_kit::TestAppContext) {
    use browser::EngineEvent::External;
    let (view, cx) = test_browser(cx);
    engine_event(&view, External("mailto:a@b.c".into()), cx);
    assert!(cx.has_pending_prompt(), "the user is asked");
    assert_eq!(cx.opened_url(), None, "nothing opens unasked");
    engine_event(&view, External("slack://open?team=T1".into()), cx);
    cx.simulate_prompt_answer("Open");
    cx.run_until_parked();
    assert_eq!(cx.opened_url().as_deref(), Some("mailto:a@b.c"));
    assert!(
        !cx.has_pending_prompt(),
        "the link sent while asking was dropped"
    );

    engine_event(&view, External("zoommtg://join?confno=1".into()), cx);
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    engine_event(&view, External("zoommtg://join?confno=2".into()), cx);
    assert!(
        !cx.has_pending_prompt(),
        "after a Cancel the page asks no more"
    );
    assert_eq!(cx.opened_url().as_deref(), Some("mailto:a@b.c"));

    cx.update(|_, cx| view.update(cx, |b, cx| b.navigate("example.com", cx)));
    engine_event(&view, External("zoommtg://join?confno=3".into()), cx);
    assert!(
        cx.has_pending_prompt(),
        "a page the user opened may ask again"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
}

/// After a Cancel the page's further links to other apps are dropped, so the question says
/// so, and how to have them ask again: they were dropped without a word, and looked dead.
/// A reload the user or an agent asks for lets them ask again, as opening a page does; a
/// page's own reload doesn't come this way, so it can't loop the question.
#[gpui_kit::test]
fn after_a_cancel_a_reload_lets_links_ask_again(cx: &mut gpui_kit::TestAppContext) {
    use browser::EngineEvent::External;
    use sawit_core::browser::BrowserOp;
    let (view, cx) = test_browser(cx);
    engine_event(&view, External("zoommtg://join?confno=1".into()), cx);
    let (_, detail) = cx.pending_prompt().expect("the user is asked");
    assert!(detail.contains("zoommtg://join?confno=1"), "{detail}");
    assert!(
        detail.contains("reload"),
        "Cancel isn't explained: {detail}"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    engine_event(&view, External("zoommtg://join?confno=2".into()), cx);
    assert!(
        !cx.has_pending_prompt(),
        "after a Cancel the page asks no more"
    );

    cx.update(|_, cx| view.update(cx, |b, cx| drop(b.apply(BrowserOp::Reload, cx))));
    engine_event(&view, External("zoommtg://join?confno=3".into()), cx);
    assert!(cx.has_pending_prompt(), "a reload lets the page ask again");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
}

/// A port clicked in Ports opens in the browser even when it is open already: it used to
/// come forward still on the page it had.
#[gpui_kit::test]
fn an_open_browser_goes_to_the_port_clicked(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let url = |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| view.read(cx).url.clone());
    cx.update(|_, cx| view.update(cx, |b, cx| b.show(Some("http://localhost:5173"), cx)));
    assert_eq!(url(cx), "http://localhost:5173");
    cx.update(|_, cx| view.update(cx, |b, cx| b.show(Some("http://localhost:3000"), cx)));
    assert_eq!(url(cx), "http://localhost:3000");
    // The Browser button: the page stays.
    cx.update(|_, cx| view.update(cx, |b, cx| b.show(None, cx)));
    assert_eq!(url(cx), "http://localhost:3000");
}

/// A blank new window (`window.open()`, from a timer) leaves the page under test as it is,
/// and a `mailto:` popup asks before Mail opens instead of loading `https://mailto:…`.
#[gpui_kit::test]
fn new_windows_leave_the_page_alone(cx: &mut gpui_kit::TestAppContext) {
    use browser::EngineEvent::NewWindow;
    let (view, cx) = test_browser(cx);
    cx.update(|_, cx| view.update(cx, |b, cx| b.navigate("localhost:5173/form", cx)));
    let url = |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| view.read(cx).url.clone());
    assert_eq!(url(cx), "http://localhost:5173/form");
    engine_event(&view, NewWindow(String::new()), cx);
    assert_eq!(url(cx), "http://localhost:5173/form");
    assert!(!cx.has_pending_prompt());
    engine_event(&view, NewWindow("mailto:a@b.c".into()), cx);
    assert_eq!(url(cx), "http://localhost:5173/form");
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    engine_event(&view, NewWindow("https://a.example/x".into()), cx);
    assert_eq!(url(cx), "https://a.example/x");
}
