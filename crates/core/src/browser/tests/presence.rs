//! The agents' cursors the overlay draws, on a stand-in page it can draw on.

use serde_json::{Value, json};

use crate::browser::OVERLAY_JS;

/// Runs the overlay module on a stand-in page with just enough DOM for the overlay to draw
/// (a document, elements, a closed shadow root, styles and timers that are kept rather than
/// run), then `test`; answers the JSON `test` returns. `run()` runs the timers set so far,
/// `find(cls)` lists the overlay's nodes of a class, and `page(rect)` makes a page element.
fn on_page(test: &str) -> Value {
    const PAGE: &str = r##"
            var window = globalThis;
            var innerWidth = 1200, innerHeight = 800;
            function EventTarget() {}
            EventTarget.prototype.addEventListener = function () {};
            EventTarget.prototype.removeEventListener = function () {};
            function Node() {}
            Node.prototype.appendChild = function (child) {
                if (child.parentNode) child.remove();
                (this.kids = this.kids || []).push(child);
                child.parentNode = this;
                return child;
            };
            Object.defineProperty(Node.prototype, "textContent", {
                set: function (t) { this.text = String(t); },
                get: function () { return this.text || ""; },
            });
            Object.defineProperty(Node.prototype, "isConnected", {
                get: function () {
                    var n = this;
                    while (n.parentNode || n.hostNode) n = n.parentNode || n.hostNode;
                    return n === document;
                },
            });
            function Element() {}
            Element.prototype = Object.create(Node.prototype);
            Element.prototype.setAttribute = function (k, v) { (this.attrs = this.attrs || {})[k] = String(v); };
            Element.prototype.removeAttribute = function (k) { if (this.attrs) delete this.attrs[k]; };
            Element.prototype.getAttribute = function (k) { return this.attrs && k in this.attrs ? this.attrs[k] : null; };
            Element.prototype.getBoundingClientRect = function () {
                var r = this.rect || { left: 0, top: 0, width: 0, height: 0 };
                return { left: r.left, top: r.top, width: r.width, height: r.height, x: r.left, y: r.top,
                    right: r.left + r.width, bottom: r.top + r.height };
            };
            Element.prototype.remove = function () {
                var p = this.parentNode;
                if (p) { p.kids.splice(p.kids.indexOf(this), 1); this.parentNode = null; }
            };
            Element.prototype.attachShadow = function () { var s = new ShadowRoot(); s.hostNode = this; return s; };
            Element.prototype.scrollIntoView = function () { this.scrolled = (this.scrolled || 0) + 1; };
            function CSSStyleDeclaration() { this.props = {}; }
            Object.defineProperty(CSSStyleDeclaration.prototype, "cssText", {
                set: function (text) {
                    this.props = {};
                    var parts = String(text).split(";");
                    for (var i = 0; i < parts.length; i++) {
                        var at = parts[i].indexOf(":");
                        if (at > 0) this.props[parts[i].slice(0, at).trim()] = parts[i].slice(at + 1).trim();
                    }
                },
            });
            CSSStyleDeclaration.prototype.setProperty = function (k, v) { this.props[k] = v; };
            function HTMLElement() {}
            HTMLElement.prototype = Object.create(Element.prototype);
            Object.defineProperty(HTMLElement.prototype, "style", {
                get: function () { return this.css || (this.css = new CSSStyleDeclaration()); },
            });
            HTMLElement.prototype.focus = function () {};
            HTMLElement.prototype.blur = function () {};
            function DocumentFragment() {}
            DocumentFragment.prototype = Object.create(Node.prototype);
            var walk = function (n, test) {
                var kids = n.kids || [];
                for (var i = 0; i < kids.length; i++) {
                    if (test(kids[i])) return kids[i];
                    var found = walk(kids[i], test);
                    if (found) return found;
                }
                return null;
            };
            var hasClass = function (n, cls) {
                return (" " + ((n.attrs && n.attrs["class"]) || "") + " ").indexOf(" " + cls + " ") >= 0;
            };
            DocumentFragment.prototype.querySelector = function (sel) {
                if (sel[0] === "#") return walk(this, function (n) { return n.attrs && n.attrs.id === sel.slice(1); });
                return walk(this, function (n) { return hasClass(n, sel.slice(1)); });
            };
            function ShadowRoot() {}
            ShadowRoot.prototype = Object.create(DocumentFragment.prototype);
            // The overlay's markup, read flat: each element with a class or an id, under the root.
            Object.defineProperty(ShadowRoot.prototype, "innerHTML", {
                set: function (html) {
                    var tags = /<(\w+)([^>]*)>/g, m, root = null;
                    while ((m = tags.exec(html))) {
                        if (m[1] === "style") continue;
                        var el = document.createElement(m[1]);
                        var cls = /class="([^"]*)"/.exec(m[2]), id = / id="([^"]*)"/.exec(m[2]);
                        var hidden = /aria-hidden="([^"]*)"/.exec(m[2]);
                        if (!cls && !id) continue;
                        if (cls) el.setAttribute("class", cls[1]);
                        if (id) el.setAttribute("id", id[1]);
                        if (hidden) el.setAttribute("aria-hidden", hidden[1]);
                        if (root) root.appendChild(el); else { this.appendChild(el); root = el; }
                    }
                },
            });
            function Document() {}
            Document.prototype.createElement = function (tag) { var e = new HTMLElement(); e.localName = tag; return e; };
            var document = new Document();
            document.readyState = "complete";
            document.documentElement = document.createElement("html");
            document.appendChild = Node.prototype.appendChild;
            document.appendChild(document.documentElement);
            document.body = document.createElement("body");
            document.documentElement.appendChild(document.body);
            var timers = [];
            var setTimeout = function (f, ms) { timers.push({ f: f, ms: ms }); return timers.length; };
            var clearTimeout = function (id) { if (timers[id - 1]) timers[id - 1].f = null; };
            var setInterval = function () { return 0; };
            var clearInterval = function () {};
            var requestAnimationFrame = function () { return 0; };
            var getComputedStyle = function () {
                return { fontSize: "16px", paddingLeft: "8px", borderLeftWidth: "1px", font: "16px sans-serif" };
            };
            var run = function () {
                var now = timers; timers = [];
                for (var i = 0; i < now.length; i++) if (now[i].f) now[i].f();
                return now.length;
            };
            var TOKEN = "T0K3N";
            var apply = Reflect.apply;
            var sent = [];
            var send = function (kind, data) { sent.push([kind, data]); };
            var elements = {};
            var resolve = function (op) {
                if (!op.selector) return null;
                if (!elements[op.selector]) throw new Error("none");
                return elements[op.selector];
            };
            var page = function (selector, rect, name) {
                var el = document.createElement("button");
                el.rect = rect;
                el.label = name;
                document.body.appendChild(el);
                elements[selector] = el;
                return el;
            };
            var describe = function () { return ""; };
            var role = function (el) { return el.localName === "input" ? "textbox" : "button"; };
            var nameOf = function (el) { return el.label || ""; };
            var clip = function (t, n) { return t.slice(0, n); };
            var oneLine = function (t) { return (t || "").replace(/\s+/g, " ").trim(); };
            var modules = {};
            var helpers = {};
        "##;
    const AFTER: &str = r##"
            var host = function () {
                return walk(document.documentElement, function (n) { return n.localName === "sawit-overlay"; });
            };
            var shadowOf = function () { var h = host(); return h && h.attachedShadow; };
            var find = function (cls) {
                var out = [], h = host();
                if (!h) return out;
                var each = function (n) {
                    var kids = n.kids || [];
                    for (var i = 0; i < kids.length; i++) {
                        if (hasClass(kids[i], cls)) out.push(kids[i]);
                        each(kids[i]);
                    }
                };
                each(h.shadowRootForTests);
                return out;
            };
            var agent = function (id, name, color) { return { id: id, name: name, color: color }; };
            var presence = function (op) { op.op = "presence"; return modules.presence(op); };
        "##;
    // The shadow root is closed: the stand-in keeps it on the host for the test alone.
    let page = PAGE.replace(
        "var s = new ShadowRoot(); s.hostNode = this; return s;",
        "var s = new ShadowRoot(); s.hostNode = this; this.shadowRootForTests = s; return s;",
    );
    let source = format!("{page}\n{OVERLAY_JS}\n{AFTER}\n{test}");
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let mut sloppy = rquickjs::context::EvalOptions::default();
    sloppy.strict = false;
    let json: String = ctx.with(|ctx| {
        ctx.eval_with_options::<String, _>(source, sloppy)
            .unwrap_or_else(|e| {
                let caught = ctx.catch();
                panic!("the page failed: {e:?} {caught:?}")
            })
    });
    serde_json::from_str(&json).unwrap()
}

/// An agent's click: its pointer goes to the button's centre in its colour, with its name and
/// what it does in the pill, the button is ringed, and a click ripples where the pointer
/// lands. None of it can take an event: the host and the agents' layer let every pointer
/// event through, and the layer is hidden from assistive technology.
#[test]
fn an_agents_cursor_goes_to_what_it_clicks() {
    let got = on_page(
        r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            var first = presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"),
                action: "click", selector: "#signup", lead: 120 });
            var cursor = find("agent")[0];
            JSON.stringify({
                first: first,
                transform: cursor.css.props.transform,
                color: cursor.css.props["--c"],
                glide: cursor.css.props["--glide"],
                name: find("name")[0].text,
                doing: find("doing")[0].text,
                ring: find("ring")[0].css.props,
                ripple: find("ripple")[0].css.props,
                ripples: find("ripple").length,
                scrolled: elements["#signup"].scrolled,
                host: host().css.props["pointer-events"],
                hidden_from_at: find("agents")[0].attrs["aria-hidden"],
            })
        "##,
    );
    assert_eq!(
        got["first"],
        json!({"shown": true, "doing": "clicking Sign up", "done": "clicked Sign up",
                   "x": 160, "y": 220})
    );
    assert_eq!(got["transform"], "translate(160px,220px)");
    assert_eq!(got["color"], "#2563eb");
    assert_eq!(got["glide"], "0ms", "the first time it shows where it is");
    assert_eq!(got["name"], "Claude Code");
    assert_eq!(got["doing"], "clicking Sign up");
    assert_eq!(got["ring"]["left"], "100px");
    assert_eq!(got["ring"]["width"], "120px");
    assert_eq!(got["ring"]["--c"], "#2563eb", "in the agent's colour");
    assert_eq!(got["ripple"]["--c"], "#2563eb");
    assert_eq!(got["ripples"], 1);
    assert_eq!(got["scrolled"], 1, "into view, as the click will");
    assert_eq!(got["host"], "none");
    assert_eq!(got["hidden_from_at"], "true");
}

/// Two agents at once have a cursor each, in their own colours; an agent that acts again
/// glides there, and its type shows its caret in the field before the text lands.
#[test]
fn a_swarm_has_a_cursor_each_and_typing_shows_the_caret_first() {
    let got = on_page(
        r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            var email = page("#email", { left: 400, top: 100, width: 300, height: 36 }, "Email");
            email.localName = "input";
            email.value = "";
            presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"), action: "click",
                selector: "#signup", lead: 120 });
            presence({ token: TOKEN, agent: agent("b", "Codex · shop", "#c2410c"), action: "hover",
                selector: "#signup", lead: 120 });
            var typed = presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"),
                action: "type", selector: "#email", text: "ada@example.com", lead: 120 });
            var cursors = find("agent");
            JSON.stringify({
                count: cursors.length,
                colors: cursors.map(function (c) { return c.css.props["--c"]; }),
                glide: cursors[0].css.props["--glide"],
                at: cursors[0].css.props.transform,
                typed: typed.doing,
                carets: find("caret").map(function (c) { return c.css.props; }),
                badge: find("typing")[0].text,
            })
        "##,
    );
    assert_eq!(got["count"], 2);
    assert_eq!(got["colors"], json!(["#2563eb", "#c2410c"]));
    assert_eq!(got["glide"], "120ms", "it glides from where it was");
    assert_eq!(got["at"], "translate(550px,118px)");
    assert_eq!(got["typed"], "typing in Email");
    let caret = &got["carets"][0];
    assert_eq!(
        caret["left"], "409px",
        "after the empty field's padding and border"
    );
    assert_eq!(caret["--c"], "#2563eb", "in the agent's colour");
    assert_eq!(got["badge"], "▍");
}

/// An idle cursor fades after a few seconds, and goes after 30 s; a page's own call (without
/// Sawit's token) draws none, and turning cursors off clears them.
#[test]
fn agents_cursors_fade_go_and_only_sawit_draws_them() {
    let got = on_page(
        r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            var forged = presence({ token: "nope", agent: agent("x", "Claude Code", "#2563eb"),
                action: "click", selector: "#signup" });
            presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"), action: "click",
                selector: "#signup" });
            var fades = timers.filter(function (t) { return t.ms === 5000; }).length;
            var goes = timers.filter(function (t) { return t.ms === 30000; }).length;
            run();
            var after = find("agent").length;
            presence({ token: TOKEN, agent: agent("b", "Codex", "#c2410c"), action: "scroll" });
            var scrolled = find("doing")[0].text;
            presence({ token: TOKEN, action: "clear" });
            JSON.stringify({ forged: forged, fades: fades, goes: goes, after: after,
                scrolled: scrolled, cleared: find("agent").length })
        "##,
    );
    assert_eq!(got["forged"], json!({"shown": false}));
    assert_eq!(
        (got["fades"].as_u64(), got["goes"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(got["after"], 0, "gone once its timers ran");
    assert_eq!(got["scrolled"], "scrolling");
    assert_eq!(got["cleared"], 0);
}

/// The overlay hides for a screenshot, the agents' cursors with it, unless the user wants
/// them in agents' screenshots: then only the cursors stay.
#[test]
fn screenshots_leave_the_agents_cursors_out() {
    let got = on_page(
        r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"), action: "click",
                selector: "#signup" });
            var shown = host().css.props.display;
            modules.overlay({ hidden: true });
            var shot = host().css.props.display;
            modules.overlay({ hidden: true, agents: true });
            var kept = [host().css.props.display, find("root")[0].attrs["class"]];
            modules.overlay({ hidden: false });
            var back = [host().css.props.display, find("root")[0].attrs["class"]];
            JSON.stringify({ shown: shown, shot: shot, kept: kept, back: back })
        "##,
    );
    assert_eq!(got["shown"], "block");
    assert_eq!(got["shot"], "none");
    assert_eq!(got["kept"], json!(["block", "root shot"]));
    assert_eq!(got["back"], json!(["block", "root"]));
}
