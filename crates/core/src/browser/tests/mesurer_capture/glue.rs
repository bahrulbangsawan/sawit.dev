//! The glue's overrides (`page/mesurer/{clipboard, files, capture, color}.js`) run in QuickJS on
//! a page that stands in for WebKit's world.

use serde_json::{Value, json};

use crate::browser::mesurer::GLUE_JS;
use crate::browser::mesurer::files::avc_codec_for;

/// Runs the glue on a stand-in for WebKit's world, then `test`, then `JSON.stringify(out)` once
/// every promise has settled. `replies[kind]` is what the app answers (a JSON text, `{error}`, or
/// a function of the message); `posted` keeps what the glue posted.
fn on_stand_in(test: &str) -> Value {
    const PAGE: &str = r#"
        var window = globalThis, out = {}, posted = [], replies = {}, nativeClicks = [], asked = [];
        function Handler() {}
        Handler.prototype.postMessage = function (s) {
            var m = JSON.parse(s), r = replies[m.kind];
            posted.push(m);
            if (typeof r === "function") return r(m);
            if (r && r.error) return Promise.reject(r.error);
            return Promise.resolve(r === undefined ? "null" : r);
        };
        window.webkit = { messageHandlers: { sawitMesurer: new Handler() } };
        var location = { href: "http://localhost:3000/" }, innerWidth = 1280, devicePixelRatio = 2;
        var performance = { now: function () { return 0; } };
        var setTimeout = function () { return 0; };
        var queueMicrotask = function (fn) { Promise.resolve().then(fn); };
        var B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        function btoa(s) {
            var o = "";
            for (var i = 0; i < s.length; i += 3) {
                var n = (s.charCodeAt(i) << 16) | ((s.charCodeAt(i + 1) || 0) << 8) | (s.charCodeAt(i + 2) || 0);
                o += B64[n >> 18] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
            }
            return o;
        }
        function atob(s) {
            var o = "", bits = 0, n = 0;
            for (var i = 0; i < s.length && s[i] !== "="; i++) {
                n = (n << 6) | B64.indexOf(s[i]); bits += 6;
                if (bits >= 8) { bits -= 8; o += String.fromCharCode((n >> bits) & 255); }
            }
            return o;
        }
        function DOMException(message, name) { this.message = message; this.name = name; }
        function Event(type) { this.type = type; }
        function TextEncoder() {}
        TextEncoder.prototype.encode = function (s) { return new Uint8Array(s.split("").map(function (c) { return c.charCodeAt(0); })); };
        function Blob(parts, options) {
            var bytes = [];
            (parts || []).forEach(function (p) {
                if (typeof p === "string") for (var i = 0; i < p.length; i++) bytes.push(p.charCodeAt(i));
                else for (var j = 0; j < p.length; j++) bytes.push(p[j]);
            });
            this.bytes = new Uint8Array(bytes); this.type = (options && options.type) || "";
        }
        Blob.prototype.arrayBuffer = function () { return Promise.resolve(this.bytes.buffer); };
        Blob.prototype.text = function () { return Promise.resolve(String.fromCharCode.apply(null, this.bytes)); };
        function Clipboard() {}
        Clipboard.prototype.writeText = Clipboard.prototype.write = function () { throw new Error("the page's own"); };
        function MediaDevices() {}
        var navigator = { clipboard: new Clipboard(), mediaDevices: new MediaDevices() };
        var blobs = 0;
        function URL() {}
        URL.createObjectURL = function () { return "blob:http://localhost:3000/" + (++blobs); };
        URL.revokeObjectURL = function () {};
        function HTMLAnchorElement() { this.attrs = {}; this.href = ""; }
        HTMLAnchorElement.prototype.click = function () { nativeClicks.push(this.href); };
        HTMLAnchorElement.prototype.setAttribute = function (k, v) { this.attrs[k] = String(v); };
        HTMLAnchorElement.prototype.getAttribute = function (k) { return k in this.attrs ? this.attrs[k] : null; };
        HTMLAnchorElement.prototype.hasAttribute = function (k) { return k in this.attrs; };
        function MediaRecorder() {}
        MediaRecorder.isTypeSupported = function () { return true; };
        function VideoEncoder() {}
        VideoEncoder.isConfigSupported = function (c) { asked.push(c.codec); return Promise.resolve({ supported: true, config: c }); };
        function AbortController() {
            var all = [], signal = { aborted: false };
            signal.addEventListener = function (t, f) { all.push(f); };
            signal.removeEventListener = function (t, f) { all = all.filter(function (g) { return g !== f; }); };
            this.signal = signal;
            this.abort = function () { signal.aborted = true; all.slice().forEach(function (f) { f(); }); };
        }
        var track = { stopped: false, events: [], getSettings: function () { return { frameRate: 30 }; },
            stop: function () { this.stopped = true; }, dispatchEvent: function (e) { this.events.push(e.type); } };
        var canvas = { getContext: function () { return { drawImage: function () {} }; },
            captureStream: function (fps) { canvas.fps = fps; return { getVideoTracks: function () { return [track]; } }; } };
        function createImageBitmap(blob) { return Promise.resolve({ width: 4, height: 2, bytes: blob.bytes.length, close: function () {} }); }
        function CSSStyleSheet() {}
        CSSStyleSheet.prototype.replaceSync = function () {};
        function MutationObserver() {}
        MutationObserver.prototype.observe = MutationObserver.prototype.disconnect = function () {};
        var el = function (tag) {
            if (tag === "canvas") return canvas;
            var e = { id: "", attrs: {}, children: [], style: { setProperty: function () {} },
                setAttribute: function (k, v) { e.attrs[k] = v; }, removeAttribute: function () {},
                appendChild: function (c) { c.parentNode = e; e.children.push(c); return c; },
                attachShadow: function () { return e.shadow = { adoptedStyleSheets: [], children: [], tree: false,
                    appendChild: function (c) { this.children.push(c); c.parentNode = this; return c; },
                    querySelector: function () { return this.tree ? {} : null; } }; } };
            return e;
        };
        var html = el("html");
        var document = { documentElement: html, body: null, createElement: el, getElementById: function () { return null; } };
        var renders = [];
        globalThis.__sawitMesurerBundle = { version: "0.2.7", commit: "5c2fdae",
            render: function (box, root, props) { renders.push(props); root.tree = true; },
            update: function () {}, unmount: function () {} };
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

/// mesurer's copies go to the app: a promised PNG as `clipboard.image`, text as
/// `clipboard.text`; other types are refused. The page's own clipboard is never called.
#[test]
fn the_worlds_clipboard_copies_images_and_text_through_the_app() {
    let got = on_stand_in(
        r##"
        var png = new Blob([new Uint8Array([137, 80, 78, 71])], { type: "image/png" });
        navigator.clipboard.write([new ClipboardItem({ "image/png": Promise.resolve(png) })])
            .then(function () { return navigator.clipboard.write([new ClipboardItem({ "text/plain": "#336699" })]); })
            .then(function () { return navigator.clipboard.write([new ClipboardItem({ "text/html": "<b>" })]); })
            .catch(function (e) { out.refused = e.name; })
            .then(function () {
                out.posted = posted.map(function (m) { return [m.kind, m.payload]; });
                out.supports = [ClipboardItem.supports("image/png"), ClipboardItem.supports("text/html")];
            });
        "##,
    );
    assert_eq!(
        got["posted"],
        json!([["clipboard.image", {"png_base64": "iVBORw=="}], ["clipboard.text", {"text": "#336699"}]])
    );
    assert_eq!(got["refused"], "NotAllowedError");
    assert_eq!(got["supports"], json!([true, false]));
}

/// A download link mesurer clicks posts its file, even when its blob is revoked right after;
/// a data: link is read here. Links with no `download`, or to the web, click as before.
#[test]
fn download_links_are_posted_not_followed() {
    let got = on_stand_in(
        r#"
        var link = function (href, download) {
            var a = new HTMLAnchorElement();
            a.href = href;
            if (download !== null) a.setAttribute("download", download);
            a.click();
            return a;
        };
        var gif = link(URL.createObjectURL(new Blob(["GIF89a"], { type: "image/gif" })), "clip.webm");
        URL.revokeObjectURL(gif.href);
        link("data:text/plain;base64,aGk=", "");
        link("https://example.com/x.zip", "x.zip");
        link("blob:http://localhost:3000/9", null);
        Object.defineProperty(out, "saved", { enumerable: true, get: function () {
            return posted.filter(function (m) { return m.kind === "file.save"; }).map(function (m) { return m.payload; });
        } });
        Object.defineProperty(out, "native", { enumerable: true, get: function () { return nativeClicks; } });
        "#,
    );
    assert_eq!(
        got["saved"],
        json!([
            {"name": "clip.webm", "mime": "image/gif", "base64": "R0lGODlh"},
            {"name": "", "mime": "text/plain", "base64": "aGk="}
        ])
    );
    assert_eq!(
        got["native"],
        json!(["https://example.com/x.zip", "blob:http://localhost:3000/9"])
    );
}

/// `EyeDropper` resolves with the colour the app sampled; a cancelled sample (`null`) and an
/// aborted signal reject with an AbortError mesurer takes quietly, the abort telling the app.
#[test]
fn the_eye_dropper_samples_through_the_app() {
    let got = on_stand_in(
        r##"
        replies["color.sample"] = JSON.stringify({ sRGBHex: "#336699" });
        new EyeDropper().open()
            .then(function (picked) {
                out.picked = picked;
                replies["color.sample"] = "null";
                return new EyeDropper().open();
            })
            .catch(function (e) {
                out.cancelled = [e.name, e instanceof DOMException];
                replies["color.sample"] = function () { return new Promise(function () {}); };
                var c = new AbortController();
                var open = new EyeDropper().open({ signal: c.signal });
                c.abort();
                return open;
            })
            .catch(function (e) {
                out.aborted = e.name;
                return new EyeDropper().open({ signal: { aborted: true } });
            })
            .catch(function (e) {
                out.early = e.name;
                out.kinds = posted.map(function (m) { return m.kind; });
            });
        "##,
    );
    assert_eq!(got["picked"], json!({"sRGBHex": "#336699"}));
    assert_eq!(got["cancelled"], json!(["AbortError", true]));
    assert_eq!(got["aborted"], "AbortError");
    assert_eq!(got["early"], "AbortError");
    assert_eq!(
        got["kinds"],
        json!([
            "color.sample",
            "color.sample",
            "color.sample",
            "color.cancel"
        ])
    );
}

/// mesurer's screenshots come from `capture.visible`, its recordings from a canvas stream fed by
/// `capture.frame`, which says it shares the browser tab and logs its frame rate once stopped.
/// MediaRecorder has no WebM; VideoEncoder is asked for the level `avc_codec_for` gives.
#[test]
fn screenshots_and_recordings_come_from_the_app() {
    let cases = [
        ("avc1.42001E", 640, 480),
        ("avc1.42001E", 1280, 720),
        ("avc1.64001F", 1920, 1080),
        ("avc1.4D001F", 2560, 1600),
        ("avc1.42001E", 8192, 8192),
        ("vp8", 640, 480),
    ];
    let test = format!(
        r#"
        replies["capture.visible"] = JSON.stringify({{ png_base64: "iVBORw==" }});
        replies["capture.frame"] = JSON.stringify({{ jpeg_base64: "/9j/", width: 4, height: 2 }});
        var cases = {cases};
        SawitMesurer.mount({{}})
            .then(function () {{ return renders[0].captureVisibleTab(); }})
            .then(function (shot) {{
                out.shot = [shot.type, Array.from(shot.bytes)];
                return navigator.mediaDevices.getDisplayMedia({{ video: true }});
            }})
            .then(function (stream) {{
                var t = stream.getVideoTracks()[0];
                out.surface = t.getSettings().displaySurface;
                out.canvas = [canvas.width, canvas.height, canvas.fps];
                t.stop();
                out.stopped = t.stopped;
                out.frame = posted.filter(function (m) {{ return m.kind === "capture.frame"; }})[0].payload;
                out.log = posted.filter(function (m) {{ return m.kind === "log" && /recording/.test(m.payload.text); }}).map(function (m) {{ return m.payload.text; }});
                out.webm = [MediaRecorder.isTypeSupported("video/webm;codecs=vp9"), MediaRecorder.isTypeSupported("video/mp4")];
                return Promise.all(cases.map(function (c) {{
                    return VideoEncoder.isConfigSupported({{ codec: c[0], width: c[1], height: c[2], framerate: 30 }});
                }}));
            }})
            .then(function (answers) {{
                out.supported = answers.map(function (a) {{ return a.supported ? a.config.codec : null; }});
                out.asked = asked;
            }});
        "#,
        cases = serde_json::to_string(&cases).unwrap(),
    );
    let got = on_stand_in(&test);
    assert_eq!(got["shot"], json!(["image/png", [137, 80, 78, 71]]));
    assert_eq!(got["surface"], "browser");
    assert_eq!(got["canvas"], json!([4, 2, 30]));
    assert_eq!(got["stopped"], true);
    assert_eq!(got["frame"], json!({"max_width": 1920, "quality": 0.8}));
    let log = got["log"][0].as_str().unwrap();
    assert!(
        log.starts_with("mesurer recording:") && log.contains("fps, 4x2"),
        "{log}"
    );
    assert_eq!(got["webm"], json!([false, true]));
    let want: Vec<Option<String>> = cases
        .iter()
        .map(|(c, w, h)| avc_codec_for(c, *w, *h, 30.))
        .collect();
    assert_eq!(got["supported"], json!(want));
    let asked: Vec<String> = want.into_iter().flatten().collect();
    assert_eq!(got["asked"], json!(asked));
}
