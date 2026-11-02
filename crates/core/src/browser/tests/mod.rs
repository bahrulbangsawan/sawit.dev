//! The browser module's tests, by what they test. The page script's run in QuickJS, on pages
//! that stand in for WebKit's.

mod address;
mod agent;
mod base64;
mod design;
mod evaluate;
mod ipc;
mod logs;
mod mesurer_agents;
mod mesurer_capture;
mod mesurer_keys;
mod ops;
mod overlay;
mod presence;
mod project;
mod redact;
mod routes;
mod routes_crawl;
mod viewport;

use serde_json::Value;

use crate::browser::{NetworkEntry, page_script};

fn request(url: &str, status: Option<u16>, error: Option<&str>) -> NetworkEntry {
    NetworkEntry {
        method: "GET".into(),
        url: url.into(),
        status,
        kind: "fetch".into(),
        duration_ms: None,
        request_body: None,
        response_body: None,
        error: error.map(Into::into),
        time_ms: 0.,
    }
}

/// Runs the page script (token `T0K3N`) in QuickJS on a stand-in page, then `test`, which
/// returns a JSON string. The page: a `window` whose `webkit.messageHandlers.ipc` keeps what
/// is posted (`posted`, `kinds`, `count`), wry's `window.ipc` shim (it looks the handler up
/// when called), `EventTarget.prototype`'s listener methods (the window's too) keeping the
/// listeners added in the browser's own list (`listeners`, by type), `fire` and `click` for
/// the browser dispatching an event to them, and one element, `#buy`, under every point.
/// The helpers use nothing a test's page script may patch. It all runs as sloppy-mode code,
/// as WebKit runs a page and the user script (rquickjs would make it strict).
fn on_stand_in_page(test: &str) -> Value {
    const PAGE: &str = r#"
            var window = globalThis;
            var posted = [];
            var listeners = {};
            function Handler() {}
            Handler.prototype.postMessage = function (s) { posted[posted.length] = String(s); };
            window.webkit = { messageHandlers: { ipc: new Handler() } };
            Object.defineProperty(window, "ipc", { value: Object.freeze({ postMessage: function (s) { window.webkit.messageHandlers.ipc.postMessage(s); } }) });
            function EventTarget() {}
            EventTarget.prototype.addEventListener = function (type, fn) {
                var all = listeners[type] || (listeners[type] = []);
                for (var i = 0; i < all.length; i++) if (all[i] === fn) return;
                all[all.length] = fn;
            };
            EventTarget.prototype.removeEventListener = function (type, fn) {
                var all = listeners[type] || [];
                var left = [];
                for (var i = 0; i < all.length; i++) if (all[i] !== fn) left[left.length] = all[i];
                listeners[type] = left;
            };
            window.addEventListener = EventTarget.prototype.addEventListener;
            window.removeEventListener = EventTarget.prototype.removeEventListener;
            var console = { log: function () {}, info: function () {}, warn: function () {}, error: function () {}, debug: function () {}, trace: function () {} };
            var performance = { now: function () { return 0; } };
            var location = { href: "http://localhost:5173/" };
            var history = { pushState: function () {}, replaceState: function () {} };
            function XMLHttpRequest() {}
            XMLHttpRequest.prototype.open = function () {};
            XMLHttpRequest.prototype.send = function () {};
            var setTimeout = function () { return 0; };
            var clearTimeout = function () {};
            var setInterval = function () { return 0; };
            var clearInterval = function () {};
            var requestAnimationFrame = function () { return 0; };
            var innerWidth = 800, innerHeight = 600;
            var Node = { ELEMENT_NODE: 1, TEXT_NODE: 3, DOCUMENT_FRAGMENT_NODE: 11 };
            function Element() {}
            function ShadowRoot() {}
            var CSS = { escape: function (s) { return s; } };
            var getComputedStyle = function () { return { getPropertyValue: function () { return ""; } }; };
            var document = { readyState: "complete", documentElement: {} };
            var el = new Element();
            el.id = "buy"; el.localName = "button"; el.nodeType = 1; el.className = ""; el.classList = [];
            el.innerText = "Buy"; el.outerHTML = "<button id=buy>Buy</button>"; el.parentElement = null; el.isConnected = true;
            el.getBoundingClientRect = function () { return { x: 1, y: 1, width: 40, height: 20, left: 1, top: 1, right: 41, bottom: 21 }; };
            el.getAttribute = function () { return null; };
            el.hasAttribute = function () { return false; };
            el.getRootNode = function () { return document; };
            el.contains = function (other) { return other === el; };
            document.elementsFromPoint = function () { return [el]; };
            document.querySelectorAll = function () { return [el]; };
            var kinds = function (from) {
                var out = [];
                for (var i = from; i < posted.length; i++) out[out.length] = posted[i].split("\n")[1];
                return out;
            };
            var count = function (from, kind) {
                var all = kinds(from);
                var n = 0;
                for (var i = 0; i < all.length; i++) if (all[i] === kind) n++;
                return n;
            };
            var noop = function () {};
            var fire = function (type, event) {
                var all = listeners[type] || [];
                var now = [];
                for (var i = 0; i < all.length; i++) now[now.length] = all[i];
                for (var j = 0; j < now.length; j++) now[j](event);
            };
            var click = function (trusted) {
                fire("click", { isTrusted: trusted, type: "click", clientX: 5, clientY: 5, preventDefault: noop, stopImmediatePropagation: noop });
            };
            var shiftCmdPeriod = function (trusted) {
                fire("keydown", { isTrusted: trusted, type: "keydown", key: ".", code: trusted ? "Period" : "", metaKey: true, shiftKey: true, altKey: false, preventDefault: noop, stopImmediatePropagation: noop });
            };
        "#;
    let source = format!("{PAGE}\n{}\n{test}", page_script("T0K3N"));
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let mut sloppy = rquickjs::context::EvalOptions::default();
    sloppy.strict = false;
    let json: String = ctx.with(|ctx| {
        ctx.eval_with_options::<String, _>(source, sloppy)
            .unwrap_or_else(|e| {
                let caught = ctx.catch();
                let why = caught
                    .as_exception()
                    .map(|x| format!("{:?} {:?}", x.message(), x.stack()));
                panic!("the stand-in page failed: {e:?} {why:?}")
            })
    });
    serde_json::from_str(&json).unwrap()
}
