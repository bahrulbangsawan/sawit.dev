//! The page agent with its modules spliced in: what a page script can't get from it, and that
//! it is strict-mode code that parses.

use serde_json::json;

use super::on_stand_in_page;
use crate::browser::{
    AGENT_JS, DESIGN_JS, MODULES_PLACEHOLDER, OVERLAY_JS, TOKEN_PLACEHOLDER, page_script,
};

#[test]
fn page_script_splices_the_modules() {
    let s = page_script("abc123");
    assert!(!s.contains(MODULES_PLACEHOLDER) && !s.contains(TOKEN_PLACEHOLDER));
    assert!(s.contains("\"abc123\""));
    assert_eq!(
        s.matches("Object.defineProperty(window, \"__sawitAgent\"")
            .count(),
        1
    );
    assert!(s.find("sawit-overlay").unwrap() < s.find("const OPS").unwrap());
    assert!(s.contains("blocked_sheets"));
    assert!(s.find("blocked_sheets").unwrap() < s.find("const OPS").unwrap());
}

/// The page script is made once around its token: a page's is the agent with its modules
/// spliced in and its token set, as `replace` sets it, whatever the token.
#[test]
fn page_script_sets_the_token_as_replace_would() {
    let spliced = AGENT_JS.replace(MODULES_PLACEHOLDER, &[OVERLAY_JS, DESIGN_JS].join("\n"));
    for token in ["T0K3N", "", TOKEN_PLACEHOLDER, "a\nb"] {
        assert_eq!(
            page_script(token),
            spliced.replace(TOKEN_PLACEHOLDER, token)
        );
    }
}

/// A page script that patches the IPC handler's `postMessage` (on its prototype, which
/// wry's shim looks up on every call) sees what the agent posts after that; the token
/// signing it must not be in it, or the page could sign messages of its own.
#[test]
fn page_scripts_never_see_the_token() {
    let got = on_stand_in_page(
        r#"
            var stolen = [];
            Handler.prototype.postMessage = function (s) { stolen.push(String(s)); };
            window.webkit.messageHandlers.ipc.postMessage = Handler.prototype.postMessage;
            console.error("boom");
            JSON.stringify({ stolen: stolen.filter(function (m) { return m.indexOf("T0K3N") >= 0; }).length, sent: kinds(0) })
        "#,
    );
    assert_eq!(got["stolen"], 0, "{got}");
    assert_eq!(got["sent"], json!(["console"]), "{got}");
}

/// The picker takes only the user's clicks: a page that arms it through `__sawitAgent`
/// and dispatches a click of its own picks nothing.
#[test]
fn synthetic_clicks_pick_nothing() {
    let got = on_stand_in_page(
        r#"
            __sawitAgent.run({ op: "pick", mode: "annotate" });
            var from = posted.length;
            click(false);
            var forged = count(from, "picked");
            click(true);
            var real = count(from, "picked") - forged;
            JSON.stringify({ forged: forged, real: real })
        "#,
    );
    assert_eq!(got, json!({"forged": 0, "real": 1}));
}

/// The picker's listeners never reach a page script: one that kept them could call them
/// with an `isTrusted: true` object of its own, and pick (with a screenshot, taking the
/// keyboard) while the user is annotating. The page here patched, before the picker was
/// armed, every way they could pass it: the window's and `EventTarget.prototype`'s listener
/// methods, the array iterator (`for…of` and destructuring), `Array.prototype`'s callbacks
/// and `Reflect.apply`.
#[test]
fn pages_never_get_the_pickers_listeners() {
    let got = on_stand_in_page(
        r#"
            var kept = [];
            var keep = function (v) {
                if (typeof v === "function") kept[kept.length] = v;
                else if (v && typeof v === "object" && typeof v.length === "number")
                    for (var i = 0; i < v.length; i++) if (typeof v[i] === "function") kept[kept.length] = v[i];
            };
            var add = EventTarget.prototype.addEventListener;
            var remove = EventTarget.prototype.removeEventListener;
            var keptAdd = function (type, fn, options) { keep(fn); return add.call(this, type, fn, options); };
            var keptRemove = function (type, fn, options) { keep(fn); return remove.call(this, type, fn, options); };
            window.addEventListener = keptAdd;
            window.removeEventListener = keptRemove;
            EventTarget.prototype.addEventListener = keptAdd;
            EventTarget.prototype.removeEventListener = keptRemove;
            var iterator = Object.getPrototypeOf([][Symbol.iterator]());
            var next = iterator.next;
            iterator.next = function () {
                var r = next.call(this);
                if (r && !r.done) keep(r.value);
                return r;
            };
            var methods = ["forEach", "map", "filter", "some", "every", "find", "findIndex", "reduce", "flatMap", "slice", "concat", "includes", "indexOf"];
            for (var m = 0; m < methods.length; m++) (function (name) {
                var real = Array.prototype[name];
                Array.prototype[name] = function () {
                    for (var i = 0; i < this.length; i++) keep(this[i]);
                    for (var j = 0; j < arguments.length; j++) keep(arguments[j]);
                    return real.apply(this, arguments);
                };
            })(methods[m]);
            var reflectApply = Reflect.apply;
            Reflect.apply = function (f, self, args) { keep(f); keep(args); return reflectApply(f, self, args); };

            // Sawit arms the picker (the user opened Annotate), stops it and arms it again.
            __sawitAgent.run({ op: "pick", mode: "annotate" });
            __sawitAgent.run({ op: "pick", mode: "off" });
            __sawitAgent.run({ op: "pick", mode: "annotate" });
            var from = posted.length;
            var event = { isTrusted: true, type: "click", key: ".", code: "Period", metaKey: true, shiftKey: true, altKey: false,
                clientX: 5, clientY: 5, deltaY: -1, preventDefault: noop, stopImmediatePropagation: noop };
            for (var i = 0; i < kept.length; i++) {
                try { kept[i](event); } catch (_) {}
            }
            var forged = kinds(from).length;
            click(true);
            JSON.stringify({ forged: forged, real: count(from, "picked") })
        "#,
    );
    assert_eq!(got, json!({"forged": 0, "real": 1}));
}

/// ⇧⌘. opens Annotate and arms the picker, and the page's script answers it from no one: a
/// page can dispatch one too, and the user's own is the keymap's, which sees ⌘ keys before
/// the page does, so a key the keymap doesn't bind to Annotate doesn't toggle it. (An
/// agent's `browser_press_key "Meta+Shift+."` goes to Sawit.)
#[test]
fn the_page_script_answers_no_shift_cmd_period() {
    let got = on_stand_in_page(
        r#"
            var from = posted.length;
            shiftCmdPeriod(false);
            var synthetic = count(from, "key");
            shiftCmdPeriod(true);
            JSON.stringify({ synthetic: synthetic, real: count(from, "key") - synthetic })
        "#,
    );
    assert_eq!(got, json!({"synthetic": 0, "real": 0}));
}

/// What the user typed into the page's password fields never reaches the network log: a
/// request that carries it, under any name, as typed, URL-encoded or in a JSON string, is
/// logged with [redacted] there (Sawit then hides fields named like secrets). A value shorter
/// than 4 characters is left alone: it would hide the same letters everywhere.
#[test]
fn typed_passwords_never_reach_the_network_log() {
    let got = on_stand_in_page(
        r#"
            var typed = [{ value: 'hunter2"!' }, { value: "abc" }];
            document.querySelectorAll = function (selector) {
                return selector.indexOf("password") >= 0 ? typed : [el];
            };
            var request = function (url, body) {
                var xhr = new XMLHttpRequest();
                var done = null;
                xhr.addEventListener = function (type, fn) { if (type === "loadend") done = fn; };
                xhr.status = 200;
                xhr.responseType = "";
                xhr.responseText = "{}";
                xhr.open("POST", url);
                xhr.send(body);
                done();
                return JSON.parse(posted[posted.length - 1].split("\n").slice(2).join("\n"));
            };
            var json = request("http://localhost:5173/login", '{"user":"ana","pw":"hunter2\\"!","code":"abc"}');
            var form = request("http://localhost:5173/login?p=" + encodeURIComponent('hunter2"!'), "u=ana&p=hunter2%22!");
            JSON.stringify({ json: json.request_body, url: form.url, form: form.request_body })
        "#,
    );
    assert_eq!(
        got,
        json!({
            "json": r#"{"user":"ana","pw":"[redacted]","code":"abc"}"#,
            "url": "http://localhost:5173/login?p=[redacted]",
            "form": "u=ana&p=[redacted]",
        })
    );
}

/// The agent, with the modules spliced into it, is strict-mode code. WebKit runs a user
/// script as sloppy code unless it says otherwise, and JavaScriptCore answers
/// `Function.prototype.caller` for a sloppy function, arrows included: a page function the
/// agent calls (a replaced `JSON.stringify` or `preventDefault`, a getter on an event) got
/// `send`, which signs with the token, or the picker's own `click`, from its caller. With
/// either a page opened Annotate and picked for itself. QuickJS has no `caller`, but tells
/// the two apart: reading it throws for a strict function and not for a sloppy one.
#[test]
fn the_agent_is_strict_mode_code() {
    // The agent's hooks, on a page that is sloppy (as pages are).
    let got = on_stand_in_page(
        r#"
            var strict = function (f) { try { f.caller; return false; } catch (_) { return true; } };
            JSON.stringify({ console: strict(console.log), xhr: strict(XMLHttpRequest.prototype.send),
                history: strict(history.pushState), page: strict(function () {}) })
        "#,
    );
    assert_eq!(
        got,
        json!({"console": true, "xhr": true, "history": true, "page": false})
    );
    // So is every other function in it: the directive is first in the one function that
    // holds the agent and its modules.
    let mut code = AGENT_JS
        .lines()
        .skip_while(|line| line.starts_with("//") || line.trim().is_empty());
    assert_eq!(code.next(), Some("(() => {"));
    assert_eq!(code.next().map(str::trim), Some("\"use strict\";"));
    assert!(AGENT_JS.trim_end().ends_with("})();"));
}

#[test]
fn agent_script_parses() {
    assert!(AGENT_JS.contains(TOKEN_PLACEHOLDER) && AGENT_JS.contains("__sawitAgent"));
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    ctx.with(|ctx| {
        // Compiles without running: there is no DOM here.
        let source = format!("(function () {{ {} }})", page_script("t"));
        ctx.eval::<rquickjs::Value, _>(source)
            .map(|_| ())
            .unwrap_or_else(|e| panic!("agent script doesn't parse: {e:?}"));
    });
}
