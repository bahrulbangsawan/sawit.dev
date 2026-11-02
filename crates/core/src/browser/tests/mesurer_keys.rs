//! Lane 3 (mesurer-ui): keys beside mesurer. Sawit's picker (the page overlay, on the stand-in
//! page of [`super::on_stand_in_page`]) leaves Esc and the walk keys to mesurer where they are
//! mesurer's; the glue's keys (`page/mesurer/keys.js`, on a stand-in of its own) hand keys to
//! mesurer, open its links through the app, and pass it the keys its gate would stop.

use serde_json::{Value, json};

use super::on_stand_in_page;
use crate::browser::mesurer::GLUE_JS;

/// mesurer's host on the stand-in page (`#mesurer-extension-host`, marked `data-sawit-mesurer`),
/// timers the test runs itself, and a trusted keydown helper that counts what the picker did to
/// it (`preventDefault`, `stopImmediatePropagation`).
const MESURER_HERE: &str = r#"
    var timers = [];
    setTimeout = function (fn) { timers[timers.length] = fn; return timers.length; };
    var runTimers = function () { var t = timers; timers = []; for (var i = 0; i < t.length; i++) t[i](); };
    var mesurer = { hasAttribute: function (n) { return n === "data-sawit-mesurer"; } };
    document.getElementById = function (id) { return id === "mesurer-extension-host" ? mesurer : null; };
    var key = function (k) {
        var e = { isTrusted: true, type: "keydown", key: k, target: el, defaultPrevented: false, touched: 0,
            preventDefault: function () { this.touched++; }, stopImmediatePropagation: function () { this.touched++; } };
        fire("keydown", e);
        return e;
    };
    var pointerAt = function (target) {
        fire("pointermove", { isTrusted: true, type: "pointermove", target: target, clientX: 5, clientY: 5, altKey: false,
            preventDefault: noop, stopImmediatePropagation: noop });
    };
"#;

/// With mesurer's host on the page, the picker doesn't swallow Esc: mesurer sees it first. Once
/// it has gone through the page, picking stops only if no one prevented it (mesurer leaving a
/// tool, or hiding its toolbar, does); the next Esc, which no one takes, stops it.
#[test]
fn beside_mesurer_esc_stops_picking_only_when_no_one_took_it() {
    let got = on_stand_in_page(&format!(
        r#"{MESURER_HERE}
            __sawitAgent.run({{ op: "pick", mode: "annotate" }});
            var from = posted.length;
            var first = key("Escape");
            var untouched = first.touched;
            first.defaultPrevented = true;
            var before = count(from, "key");
            runTimers();
            var taken = count(from, "key");
            click(true);
            var pickedWhileOn = count(from, "picked");
            key("Escape");
            runTimers();
            var free = count(from, "key");
            click(true);
            JSON.stringify({{ untouched: untouched, before: before, taken: taken, pickedWhileOn: pickedWhileOn,
                free: free, pickedAfter: count(from, "picked") }})
        "#
    ));
    assert_eq!(
        got,
        json!({"untouched": 0, "before": 0, "taken": 0, "pickedWhileOn": 1, "free": 1, "pickedAfter": 1})
    );
}

/// Esc typed into one of mesurer's own fields (its comment search, a text note) is the field's:
/// picking goes on even though no one prevented it; once mesurer's field lets the keyboard go,
/// the next Esc that no one takes stops picking again.
#[test]
fn beside_mesurer_esc_in_a_mesurer_field_keeps_picking() {
    let got = on_stand_in_page(&format!(
        r#"{MESURER_HERE}
            var field = {{ tagName: "INPUT", isContentEditable: false }};
            mesurer.shadowRoot = {{ activeElement: field }};
            __sawitAgent.run({{ op: "pick", mode: "annotate" }});
            var from = posted.length;
            var typing = function (k) {{
                var e = {{ isTrusted: true, type: "keydown", key: k, target: mesurer, defaultPrevented: false,
                    touched: 0, preventDefault: noop, stopImmediatePropagation: noop }};
                fire("keydown", e);
                return e;
            }};
            typing("Escape");
            runTimers();
            var inField = count(from, "key");
            field.tagName = "DIV";
            field.isContentEditable = true;
            typing("Escape");
            runTimers();
            var inNote = count(from, "key");
            mesurer.shadowRoot = {{ activeElement: null }};
            typing("Escape");
            runTimers();
            JSON.stringify({{ inField: inField, inNote: inNote, after: count(from, "key") }})
        "#
    ));
    assert_eq!(got, json!({"inField": 0, "inNote": 0, "after": 1}));
}

/// Without mesurer's host, Esc is Sawit's at once, as before: swallowed, and picking stops.
#[test]
fn without_mesurer_esc_stops_picking_at_once() {
    let got = on_stand_in_page(&format!(
        r#"{MESURER_HERE}
            document.getElementById = function () {{ return null; }};
            __sawitAgent.run({{ op: "pick", mode: "annotate" }});
            var from = posted.length;
            var e = key("Escape");
            JSON.stringify({{ touched: e.touched > 0, keys: count(from, "key"), timers: timers.length }})
        "#
    ));
    assert_eq!(got, json!({"touched": true, "keys": 1, "timers": 0}));
}

/// The walk keys (↑ ↓ [ ]) are Sawit's only while it has an element under the pointer: over
/// mesurer (its host on top) they go to mesurer untouched; over the page they walk, as before.
#[test]
fn beside_mesurer_the_walk_keys_need_an_element_under_the_pointer() {
    let got = on_stand_in_page(&format!(
        r#"{MESURER_HERE}
            __sawitAgent.run({{ op: "pick", mode: "annotate" }});
            var walk = function () {{
                var out = [];
                var keys = ["ArrowUp", "ArrowDown", "[", "]"];
                for (var i = 0; i < keys.length; i++) out[out.length] = key(keys[i]).touched;
                return out;
            }};
            var none = walk();
            document.elementsFromPoint = function () {{ return [mesurer, el]; }};
            pointerAt(mesurer);
            var over = walk();
            document.elementsFromPoint = function () {{ return [el]; }};
            pointerAt(el);
            JSON.stringify({{ none: none, over: over, page: walk() }})
        "#
    ));
    assert_eq!(
        got,
        json!({"none": [0, 0, 0, 0], "over": [0, 0, 0, 0], "page": [2, 2, 2, 2]})
    );
}

/// Runs the glue on a stand-in for WebKit (window listeners kept by type, `KeyboardEvent`, a DOM
/// just big enough for mesurer's host and its shadow root), mounts it, then runs `test`; answers
/// `JSON.stringify(out)` once every promise has settled.
fn on_glue(test: &str) -> Value {
    const PAGE: &str = r#"
        var window = globalThis;
        var out = {};
        var posted = [];
        function Handler() {}
        Handler.prototype.postMessage = function (s) { posted.push(JSON.parse(s)); return Promise.resolve("null"); };
        window.webkit = { messageHandlers: { sawitMesurer: new Handler() } };
        var location = { href: "http://localhost:3000/" };
        var performance = { now: function () { return 1; } };
        var setTimeout = function () { return 0; };
        var queueMicrotask = function (fn) { Promise.resolve().then(fn); };
        function URL(u, base) { if (!/^[a-z]+:/i.test(u)) u = base.replace(/\/$/, "") + "/" + u.replace(/^\//, ""); this.href = u; }
        function Clipboard() {}
        var navigator = { clipboard: new Clipboard() };
        function CSSStyleSheet() {}
        CSSStyleSheet.prototype.replaceSync = function () {};
        function MutationObserver() {}
        MutationObserver.prototype.observe = function () {};
        MutationObserver.prototype.disconnect = function () {};
        var listeners = {};
        window.addEventListener = function (type, fn) { (listeners[type] = listeners[type] || []).push(fn); };
        function KeyboardEvent(type, init) {
            this.type = type;
            for (var k in init) this[k] = init[k];
            this.defaultPrevented = false;
        }
        KeyboardEvent.prototype.preventDefault = function () { this.defaultPrevented = true; };
        KeyboardEvent.prototype.stopImmediatePropagation = function () { this.stopped = true; };
        var el = function (tag) {
            var e = { nodeType: 1, tagName: tag, id: "", attrs: {}, children: [], parentNode: null, style: { setProperty: function () {} },
                setAttribute: function (k, v) { this.attrs[k] = v; }, removeAttribute: function (k) { delete this.attrs[k]; },
                getAttribute: function (k) { return k in this.attrs ? this.attrs[k] : null; },
                hasAttribute: function (k) { return k in this.attrs; },
                appendChild: function (c) { c.parentNode = this; this.children.push(c); return c; },
                remove: function () { this.parentNode = null; },
                dispatched: [],
                dispatchEvent: function (ev) { this.dispatched.push(ev); if (this.onKey) this.onKey(ev); return !ev.defaultPrevented; },
                attachShadow: function () {
                    var root = el("SHADOW");
                    root.activeElement = null;
                    root.adoptedStyleSheets = [];
                    root.querySelector = function () { return {}; };
                    root.contains = function (n) { return n === root.activeElement; };
                    e.shadowRoot = root;
                    return root;
                },
            };
            return e;
        };
        var html = el("HTML");
        var body = el("BODY");
        var document = {
            documentElement: html, body: body, activeElement: body, createElement: el,
            getElementById: function (id) { return html.children.filter(function (c) { return c.id === id; })[0] || null; },
        };
        globalThis.__sawitMesurerBundle = { version: "0.2.7", commit: "5c2fdae", render: function () {}, update: function () {}, unmount: function () {} };
    "#;
    let source = format!(
        "{PAGE}\n{GLUE_JS}\nSawitMesurer.mount({{}}).then(function () {{\n\
         var host = document.getElementById('mesurer-extension-host'); var shadow = host.shadowRoot;\n{test}\n}});"
    );
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let mut sloppy = rquickjs::context::EvalOptions::default();
    sloppy.strict = false;
    ctx.with(|ctx| {
        ctx.eval_with_options::<(), _>(source, sloppy)
            .unwrap_or_else(|e| panic!("the glue failed: {e:?} {:?}", ctx.catch()))
    });
    while rt.is_job_pending() {
        rt.execute_pending_job().unwrap();
    }
    let json: String = ctx.with(|ctx| ctx.eval("JSON.stringify(out)").unwrap());
    serde_json::from_str(&json).unwrap()
}

/// `SawitMesurer.call("key", …)`: the key goes to the element with the keyboard as a keydown and
/// a keyup, composed, with its modifiers; the answer says whether something prevented it.
#[test]
fn a_key_the_app_hands_over_goes_to_the_element_with_the_keyboard() {
    let got = on_glue(
        r#"
        var field = el("TEXTAREA");
        document.activeElement = host;
        shadow.activeElement = field;
        field.onKey = function (ev) { if (ev.type === "keydown" && ev.metaKey) ev.preventDefault(); };
        return SawitMesurer.call("key", { key: "k", meta: true, shift: true }).then(function (r) {
            out.answer = r;
            out.events = field.dispatched.map(function (ev) {
                return [ev.type, ev.key, ev.code, ev.metaKey, ev.shiftKey, ev.altKey, ev.composed, ev.bubbles];
            });
            document.activeElement = body;
            return SawitMesurer.call("key", { key: "a", meta: true });
        }).then(function (r) {
            out.onBody = [r.prevented, body.dispatched.length];
            return SawitMesurer.call("key", {}).catch(function (e) { out.nameless = String(e.message); });
        });
        "#,
    );
    assert_eq!(
        got["answer"],
        json!({"key": "k", "prevented": true, "mounted": true})
    );
    assert_eq!(
        got["events"],
        json!([
            ["keydown", "k", "KeyK", true, true, false, true, true],
            ["keyup", "k", "KeyK", true, true, false, true, true]
        ])
    );
    assert_eq!(got["onBody"], json!([false, 2]));
    assert!(
        got["nameless"].as_str().unwrap().contains("needs its name"),
        "{got}"
    );
}

/// mesurer's `window.open` (Settings → Website) posts `link.open` with the absolute address, and
/// opens nothing in the page; an address that isn't http(s) isn't posted.
#[test]
fn mesurers_links_go_to_the_app() {
    let got = on_glue(
        r#"
        out.returned = window.open("https://mesurer.dev", "_blank", "noopener,noreferrer");
        window.open("javascript:alert(1)");
        return Promise.resolve().then(function () {
            out.links = posted.filter(function (m) { return m.kind === "link.open"; }).map(function (m) { return m.payload.url; });
        });
        "#,
    );
    assert_eq!(got["returned"], Value::Null);
    assert_eq!(got["links"], json!(["https://mesurer.dev"]));
}

/// While mesurer has the keyboard, the keys its gate stops outright (L, B, ⌥S…) are handed to
/// mesurer on its shadow root, not composed, and stopped where they were; the keys the gate
/// bridges (G, Esc, ⌘K), the annotation keys (⌫, ⌘A), the browser's chords (⌘C), keys typed in
/// a mesurer field, and every key while mesurer hasn't the keyboard, are left as they are.
#[test]
fn keys_the_gate_would_stop_are_handed_to_mesurer() {
    let got = on_glue(
        r#"
        var press = function (init) {
            var ev = new KeyboardEvent("keydown", Object.assign({ code: "", altKey: false, ctrlKey: false, metaKey: false, shiftKey: false }, init));
            listeners.keydown.forEach(function (fn) { fn(ev); });
            var copy = shadow.dispatched[shadow.dispatched.length - 1];
            var handed = copy && copy !== press.last ? [copy.key, copy.code, copy.altKey, copy.composed] : null;
            press.last = copy;
            return { handed: handed, stopped: Boolean(ev.stopped), prevented: ev.defaultPrevented };
        };
        out.notOwned = press({ key: "l", code: "KeyL" });
        html.attrs["data-mesurer-keyboard-owned"] = "1";
        out.l = press({ key: "l", code: "KeyL" });
        out.b = press({ key: "b", code: "KeyB" });
        out.altS = press({ key: "ß", code: "KeyS", altKey: true });
        out.left = ["g", "Escape", "Backspace"].map(function (k) { return press({ key: k }).handed; })
            .concat([press({ key: "k", metaKey: true }).handed, press({ key: "a", metaKey: true }).handed,
                press({ key: "c", metaKey: true }).handed]);
        var field = el("INPUT");
        document.activeElement = host;
        shadow.activeElement = field;
        out.typing = press({ key: "l", code: "KeyL" });
        "#,
    );
    let untouched = json!({"handed": null, "stopped": false, "prevented": false});
    assert_eq!(got["notOwned"], untouched);
    assert_eq!(
        got["l"],
        json!({"handed": ["l", "KeyL", false, false], "stopped": true, "prevented": true})
    );
    assert_eq!(got["b"]["handed"], json!(["b", "KeyB", false, false]));
    assert_eq!(got["altS"]["handed"], json!(["ß", "KeyS", true, false]));
    assert_eq!(got["left"], json!([null, null, null, null, null, null]));
    assert_eq!(got["typing"], untouched);
}
