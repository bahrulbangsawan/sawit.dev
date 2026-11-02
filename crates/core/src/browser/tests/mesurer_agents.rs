//! Lane 2 (mesurer-agents): tests of mesurer's comments and markup for agents: the overlay's
//! `elements_at` and `details` ops, run in QuickJS on a stand-in page, and the glue's comments
//! and store sections on a stand-in world.

use serde_json::{Value, json};

use crate::browser::OVERLAY_JS;
use crate::browser::mesurer::GLUE_JS;

/// Evaluates `source` in QuickJS, runs its promises, and answers `JSON.stringify(out)`.
fn run(source: String) -> Value {
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let mut sloppy = rquickjs::context::EvalOptions::default();
    sloppy.strict = false;
    ctx.with(|ctx| {
        ctx.eval_with_options::<(), _>(source, sloppy)
            .unwrap_or_else(|e| panic!("the script failed: {e:?} {:?}", ctx.catch()))
    });
    while rt.is_job_pending() {
        rt.execute_pending_job().unwrap();
    }
    let json: String = ctx.with(|ctx| ctx.eval("JSON.stringify(out)").unwrap());
    serde_json::from_str(&json).unwrap()
}

/// The overlay alone, on a page whose `elementsFromPoint` answers from `under`, then `test`.
fn on_overlay(test: &str) -> Value {
    const PAGE: &str = r##"
        var window = globalThis;
        var out = {};
        function EventTarget() {}
        EventTarget.prototype.addEventListener = function () {};
        EventTarget.prototype.removeEventListener = function () {};
        var setTimeout = function () { return 0; };
        var TOKEN = "T0K3N";
        var apply = Reflect.apply;
        var send = function () {};
        var describe = function () { return ""; };
        var role = function () { return null; };
        var nameOf = function () { return ""; };
        var clip = function (t, n) { return (t || "").slice(0, n); };
        var oneLine = function (t) { return (t || "").replace(/\s+/g, " ").trim(); };
        var modules = {};
        var helpers = {};
        var CSS = { escape: function (s) { return s; } };
        var innerWidth = 800, innerHeight = 600, scrollX = 0, scrollY = 1000;
        var el = function (tag, id, attrs) {
            var e = { localName: tag, id: id || "", className: "", classList: [], attributes: [],
                parentElement: null, nodeType: 1,
                getAttribute: function (k) { return (attrs || {})[k] || null; },
                hasAttribute: function (k) { return k in (attrs || {}); } };
            e.getRootNode = function () {
                return { querySelectorAll: function (s) { return s === "#" + e.id ? [e] : []; } };
            };
            return e;
        };
        var buy = el("button", "buy");
        var host = el("div", "mesurer-extension-host", { "data-sawit-mesurer": "" });
        var body = el("body");
        var under = {};
        var document = {
            readyState: "loading", body: body, documentElement: el("html"),
            elementsFromPoint: function (x, y) { return under[x + "," + y] || []; },
        };
        var resolve = function (op) {
            if (op.selector === "#buy") return buy;
            throw new Error("Nothing on the page matches “" + op.selector + "”.");
        };
    "##;
    run(format!("{PAGE}\n{OVERLAY_JS}\n{test}"))
}

/// `elements_at` names the page's element under each point of the page, looking through
/// mesurer's host; a point out of the viewport, or over the page's body alone, names none.
#[test]
fn elements_at_names_the_page_element_under_each_point() {
    let got = on_overlay(
        r##"
        under["10,20"] = [host, buy, body];
        under["50,60"] = [body];
        out.got = modules.elements_at({ points: [
            { x: 10, y: 1020 }, { x: 50, y: 1060 }, { x: 10, y: 20 }, { x: "x", y: 1 }, null,
        ] });
        out.none = modules.elements_at({});
        "##,
    );
    assert_eq!(
        got["got"]["elements"],
        json!([{"selector": "#buy", "label": "button#buy"}, null, null, null, null])
    );
    assert_eq!(got["none"], json!({"elements": []}));
}

/// `details` answers what a pick tells of the element a selector names, and refuses a selector
/// that names nothing.
#[test]
fn details_tells_of_the_element_a_selector_names() {
    let got = on_overlay(
        r##"
        try { modules.details({ selector: "#gone" }); } catch (e) { out.gone = String(e.message); }
        try { modules.details({}); } catch (e) { out.none = String(e.message); }
        out.ops = typeof modules.details + " " + typeof modules.elements_at;
        "##,
    );
    assert_eq!(got["gone"], "Nothing on the page matches “#gone”.");
    assert_eq!(got["none"], "Give a selector.");
    assert_eq!(got["ops"], "function function");
}

/// The glue on a stand-in world, its window an event target: posts are kept in `posted`.
fn on_world(test: &str) -> Value {
    const WORLD: &str = r##"
        var window = globalThis;
        var out = {};
        var posted = [];
        var listeners = {};
        window.addEventListener = function (type, fn) { (listeners[type] = listeners[type] || []).push(fn); };
        window.dispatchEvent = function (e) { (listeners[e.type] || []).forEach(function (fn) { fn(e); }); };
        function Handler() {}
        Handler.prototype.postMessage = function (s) { posted.push(JSON.parse(s)); return Promise.resolve("null"); };
        window.webkit = { messageHandlers: { sawitMesurer: new Handler() } };
        var location = { href: "http://localhost:3000/shop" };
        var now = 100;
        var performance = { now: function () { return now; } };
        var setTimeout = function () { return 0; };
        var queueMicrotask = function (fn) { Promise.resolve().then(fn); };
        function Clipboard() {}
        Clipboard.prototype.writeText = function () { throw new Error("the page's own"); };
        var navigator = { clipboard: new Clipboard() };
        globalThis.__sawitMesurerBundle = { version: "0.2.7", commit: "5c2fdae",
            render: function () {}, update: function () {}, unmount: function () {} };
    "##;
    run(format!("{WORLD}\n{GLUE_JS}\n{test}"))
}

/// "Copy for agent" posts the text mesurer just copied as `comments.copied`; an event with no copy
/// before it (a page dispatching one), or long after one, posts nothing; "Resolve all" posts
/// `comments.resolved`.
#[test]
fn copy_for_agent_and_resolve_all_reach_the_app() {
    let got = on_world(
        r##"
        window.dispatchEvent({ type: "mesurer:comments-copied" });
        navigator.clipboard.writeText("URL: http://localhost:3000/shop\n\n1. Tighten it")
            .then(function () {
                window.dispatchEvent({ type: "mesurer:comments-copied" });
                window.dispatchEvent({ type: "mesurer:comments-copied" });
                return navigator.clipboard.writeText("#late");
            })
            .then(function () {
                now += 60000;
                window.dispatchEvent({ type: "mesurer:comments-copied" });
                window.dispatchEvent({ type: "mesurer:comments-resolved" });
                out.posted = posted.map(function (m) { return [m.kind, m.payload]; });
            });
        "##,
    );
    assert_eq!(
        got["posted"],
        json!([
            ["clipboard.text", {"text": "URL: http://localhost:3000/shop\n\n1. Tighten it"}],
            ["comments.copied", {"text": "URL: http://localhost:3000/shop\n\n1. Tighten it"}],
            ["clipboard.text", {"text": "#late"}],
            ["comments.resolved", {}],
        ])
    );
}

/// `layout.show` is there for the app to call, and says why it showed nothing while mesurer isn't
/// on the page.
#[test]
fn layout_show_needs_mesurer_on_the_page() {
    let got = on_world(
        r##"
        SawitMesurer.call("layout.show", {}).then(function (r) { out.shown = r; });
        "##,
    );
    assert_eq!(
        got["shown"],
        json!({"shown": false, "reason": "mesurer isn't on the page"})
    );
}
