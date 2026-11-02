//! The glue, run in QuickJS on a page that stands in for WebKit's: a `sawitMesurer` handler that
//! keeps what is posted and replies, a DOM just big enough for the host, and a bundle that records
//! how it is rendered.

use serde_json::{Value, json};

use crate::browser::mesurer::GLUE_JS;

/// Runs the glue, then `test`, then `JSON.stringify(out)` once every promise has settled.
fn on_stand_in(test: &str) -> Value {
    const PAGE: &str = r#"
        var window = globalThis;
        var out = {};
        var posted = [];
        var replies = {};
        function Handler() {}
        Handler.prototype.postMessage = function (s) {
            var m = JSON.parse(s);
            posted.push(m);
            var r = replies[m.kind];
            if (r && r.error) return Promise.reject(r.error);
            return Promise.resolve(r === undefined ? "null" : r);
        };
        window.webkit = { messageHandlers: { sawitMesurer: new Handler() } };
        var location = { href: "http://localhost:3000/docs?b=1" };
        var performance = { now: function () { return 12.5; } };
        var setTimeout = function () { return 0; };
        var queueMicrotask = function (fn) { Promise.resolve().then(fn); };
        function Clipboard() {}
        Clipboard.prototype.writeText = function () { throw new Error("the page's own"); };
        var navigator = { clipboard: new Clipboard() };
        function CSSStyleSheet() { this.disabled = false; }
        CSSStyleSheet.prototype.replaceSync = function (t) { this.text = t; };
        function MutationObserver() {}
        MutationObserver.prototype.observe = function () {};
        MutationObserver.prototype.disconnect = function () {};
        var all = [];
        var el = function (tag) {
            var e = {
                tagName: tag, id: "", attrs: {}, children: [], parentNode: null,
                style: { props: {}, setProperty: function (p, v) { this.props[p] = v; } },
                setAttribute: function (k, v) { this.attrs[k] = v; },
                removeAttribute: function (k) { delete this.attrs[k]; },
                getAttribute: function (k) { return k in this.attrs ? this.attrs[k] : null; },
                appendChild: function (c) { c.parentNode = this; this.children.push(c); return c; },
                remove: function () {
                    var p = this.parentNode;
                    if (p) p.children = p.children.filter(function (c) { return c !== e; });
                    this.parentNode = null;
                },
                attachShadow: function (o) {
                    e.shadow = { host: e, mode: o.mode, children: [], adoptedStyleSheets: [], tree: false,
                        appendChild: function (c) { c.parentNode = this; this.children.push(c); return c; },
                        querySelector: function (s) { return s === "[data-mesurer-root]" && this.tree ? {} : null; } };
                    return e.shadow;
                },
            };
            all.push(e);
            return e;
        };
        var html = el("html");
        var document = {
            documentElement: html, body: null,
            createElement: el,
            getElementById: function (id) {
                var hit = null;
                var walk = function (n) { if (!hit && n.id === id) hit = n; n.children.forEach(walk); };
                walk(html);
                return hit;
            },
        };
        var renders = [];
        var updates = [];
        var unmounts = 0;
        globalThis.__sawitMesurerBundle = {
            version: "0.2.7", commit: "5c2fdae",
            render: function (box, root, props) { renders.push({ box: box.id, mode: root.mode, props: props }); root.tree = true; },
            update: function (p) { updates.push(p); },
            unmount: function () { unmounts++; },
        };
    "#;
    let source = format!("{PAGE}\n{GLUE_JS}\n{test}");
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

/// A first mount: the host on `<html>` with Sawit's marks, under Sawit's overlay, an open shadow
/// root, Sawit's props with mesurer opened on no tool, the app's props over the rest, and `ready`
/// once the tree is there. A second mount changes nothing.
#[test]
fn a_mount_builds_the_host_and_renders_mesurer_with_sawits_props() {
    let got = on_stand_in(
        r#"
        SawitMesurer.mount({ props: { theme: "dark", persistSession: false }, snapshot: { settings: { theme: "light" }, workspace: null } })
            .then(function (r) {
                out.first = r;
                var host = document.getElementById("mesurer-extension-host");
                out.host = { attrs: host.attrs, z: host.style.props["z-index"], display: host.style.props["display"],
                    pointer: host.style.props["pointer-events"], parent: host.parentNode === html, mode: host.shadow.mode,
                    box: host.shadow.children[0].id, sheets: host.shadow.adoptedStyleSheets.length };
                var p = renders[0].props;
                out.props = { persistSession: p.persistSession, persistOnReload: p.persistOnReload, theme: p.theme,
                    features: p.features, initialState: p.initialState, load: p.persistence.load() };
                return SawitMesurer.mount({});
            })
            .then(function (r) {
                out.second = r;
                out.renders = renders.length;
                out.kinds = posted.map(function (m) { return m.kind; });
                out.ready = posted.filter(function (m) { return m.kind === "ready"; })[0];
                out.mounted = SawitMesurer.isMounted();
            });
        "#,
    );
    assert_eq!(got["first"], json!({"mounted": true, "version": "0.2.7"}));
    assert_eq!(
        got["host"],
        json!({
            "attrs": {"data-sawit-overlay": "", "data-sawit-mesurer": ""},
            "z": "2147483646", "display": "block", "pointer": "none", "parent": true,
            "mode": "open", "box": "mesurer-extension-root", "sheets": 1
        })
    );
    assert_eq!(
        got["props"],
        json!({
            "persistSession": false, "persistOnReload": true, "theme": "dark",
            "features": {"review": false},
            "initialState": {"enabled": true, "toolMode": "none"},
            "load": {"settings": {"theme": "light"}, "workspace": null}
        })
    );
    assert_eq!(got["second"]["again"], true);
    assert_eq!(got["renders"], 1);
    assert_eq!(got["kinds"], json!(["ready"]));
    assert_eq!(
        got["ready"],
        json!({"kind": "ready", "url": "http://localhost:3000/docs?b=1",
            "payload": {"version": "0.2.7", "url": "http://localhost:3000/docs?b=1", "mount_ms": 0}})
    );
    assert_eq!(got["mounted"], true);
}

/// The persistence adapter: loads what the app read, posts each save with the page's address,
/// tells mesurer of the app's snapshots, and reports a save the app refused.
#[test]
fn the_persistence_adapter_loads_saves_and_applies() {
    let got = on_stand_in(
        r#"
        var saved = { toolMode: "guides", pages: { "/docs?b=1": { guides: [{ id: "g1" }] } } };
        replies["store.settings"] = { error: "disk full" };
        SawitMesurer.mount({ snapshot: { settings: {}, workspace: saved } }).then(function () {
            var p = renders[0].props.persistence;
            out.initialState = renders[0].props.initialState === undefined;
            out.loaded = p.load().workspace.pages["/docs?b=1"].guides[0].id;
            var heard = [];
            var errors = [];
            p.setErrorHandler(function (e) { errors.push(String(e && e.message || e)); });
            var stop = p.subscribe(function (snap, source) { heard.push([snap.workspace, source]); });
            p.saveWorkspace({ toolMode: "none", pages: {} });
            p.saveSettings({ theme: "dark" });
            p.clearWorkspace();
            p.clearSettings();
            out.afterClear = p.load();
            out.applied = SawitMesurer.apply({ settings: { a: 1 }, workspace: { pages: { "/": {} } } }, { workspace: true });
            out.heard = heard;
            stop();
            SawitMesurer.apply({ settings: {}, workspace: null });
            out.heardAfterStop = heard.length;
            out.load = p.load();
            return Promise.resolve().then(function () { out.errors = errors; });
        }).then(function () {
            out.saves = posted.filter(function (m) { return m.kind.indexOf("store.") === 0; });
            out.logged = posted.filter(function (m) { return m.kind === "log"; }).map(function (m) { return m.payload; });
        });
        "#,
    );
    assert_eq!(got["initialState"], true, "a saved workspace wins");
    assert_eq!(got["loaded"], "g1");
    let url = "http://localhost:3000/docs?b=1";
    assert_eq!(
        got["saves"],
        json!([
            {"kind": "store.workspace", "payload": {"toolMode": "none", "pages": {}}, "url": url},
            {"kind": "store.settings", "payload": {"theme": "dark"}, "url": url},
            {"kind": "store.clear", "payload": {"what": "workspace"}, "url": url},
            {"kind": "store.clear", "payload": {"what": "settings"}, "url": url},
        ])
    );
    assert_eq!(
        got["afterClear"],
        json!({"settings": {}, "workspace": null})
    );
    assert_eq!(got["applied"]["applied"], true);
    assert_eq!(
        got["heard"],
        json!([[{"pages": {"/": {}}}, {"settings": false, "workspace": true}]])
    );
    assert_eq!(got["heardAfterStop"], 1);
    assert_eq!(got["load"], json!({"settings": {}, "workspace": null}));
    assert_eq!(got["errors"], json!(["disk full"]));
    assert_eq!(
        got["logged"],
        json!([{"level": "warn", "text": "mesurer's save failed: Error: disk full"}])
    );
}

/// `hide` hides all of mesurer (X-ray's outlines too), or its chrome only, and shows it again; unmounting removes the
/// host and unmounts mesurer once; a mount after it renders again.
#[test]
fn hide_unmount_and_mount_again() {
    let got = on_stand_in(
        r#"
        SawitMesurer.mount({ snapshot: {} }).then(function () {
            var host = document.getElementById("mesurer-extension-host");
            var sheet = host.shadow.adoptedStyleSheets[0];
            var xray = el("style");
            xray.id = "mesurer-xray-styles";
            html.appendChild(xray);
            out.all = [SawitMesurer.hide("all"), host.style.props.display, sheet.disabled, xray.attrs.media];
            out.chrome = [SawitMesurer.hide("chrome").hidden, host.style.props.display, sheet.disabled, sheet.text.indexOf(".mesurer-toolbar-container") >= 0, xray.attrs.media || null];
            out.none = [SawitMesurer.hide("none").hidden, host.style.props.display, sheet.disabled];
            try { SawitMesurer.hide("bogus"); } catch (e) { out.bogus = e.message; }
            out.update = SawitMesurer.update({ theme: "light" });
            out.updates = updates;
            out.unmount = SawitMesurer.unmount();
            out.gone = document.getElementById("mesurer-extension-host") === null;
            out.unmounts = unmounts;
            out.again = SawitMesurer.unmount();
            out.unmountsAfter = unmounts;
            out.updateWhenUnmounted = SawitMesurer.update({ theme: "dark" });
            return SawitMesurer.mount({ snapshot: {} });
        }).then(function (r) {
            out.remount = [r.mounted, renders.length, document.getElementById("mesurer-extension-host") !== null];
        });
        "#,
    );
    assert_eq!(
        got["all"],
        json!([{"hidden": "all", "mounted": true}, "none", true, "not all"])
    );
    // X-ray's outlines, a `<style>` in the page, went with the rest, and come back.
    assert_eq!(got["chrome"], json!(["chrome", "block", false, true, null]));
    assert_eq!(got["none"], json!(["none", "block", true]));
    assert_eq!(got["bogus"], "Unknown hide mode “bogus”.");
    assert_eq!(got["update"], json!({"mounted": true}));
    assert_eq!(got["updates"], json!([{"theme": "light"}]));
    assert_eq!(got["unmount"], json!({"mounted": false, "was": true}));
    assert_eq!(got["gone"], true);
    assert_eq!(got["unmounts"], 1);
    assert_eq!(got["again"], json!({"mounted": false, "was": false}));
    assert_eq!(got["unmountsAfter"], 1);
    assert_eq!(got["updateWhenUnmounted"], json!({"mounted": false}));
    assert_eq!(got["remount"], json!([true, 2, true]));
}

/// mesurer's `writeText` goes to the app, in this world only; `SawitMesurer` is frozen, can't be
/// replaced, and `call` refuses a name no lane gave it.
#[test]
fn the_clipboard_goes_to_the_app_and_sawit_mesurer_is_fixed() {
    let got = on_stand_in(
        r#"
        navigator.clipboard.writeText(".card > h2").then(function (r) {
            out.copied = [r === undefined, posted[0]];
            out.frozen = Object.isFrozen(SawitMesurer);
            try { SawitMesurer = null; } catch (e) { out.assign = "threw"; }
            out.still = typeof SawitMesurer.mount;
            return SawitMesurer.call("nope", {});
        }).then(function () { out.call = "ran"; }, function (e) { out.call = e.message; });
        "#,
    );
    assert_eq!(
        got["copied"],
        json!([true, {"kind": "clipboard.text", "payload": {"text": ".card > h2"}, "url": "http://localhost:3000/docs?b=1"}])
    );
    assert_eq!(got["frozen"], true);
    assert_eq!(got["still"], "function");
    assert_eq!(got["call"], "Sawit's mesurer has no “nope”.");
}
