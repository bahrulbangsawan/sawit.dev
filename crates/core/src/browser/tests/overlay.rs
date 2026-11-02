//! The overlay module's helpers and ops, run alone in QuickJS.

use serde_json::{Value, json};

use crate::browser::OVERLAY_JS;

/// Runs the overlay module alone with the agent's names stubbed, then `test`, which reads
/// what the module put on `helpers`; answers the JSON `test` returns.
fn on_overlay(test: &str) -> Value {
    const STUBS: &str = r#"
            var window = globalThis;
            function EventTarget() {}
            EventTarget.prototype.addEventListener = function () {};
            EventTarget.prototype.removeEventListener = function () {};
            var document = { readyState: "loading" };
            var setTimeout = function () { return 0; };
            var TOKEN = "T0K3N";
            var apply = Reflect.apply;
            var sent = [];
            var send = function (kind, data) { sent.push([kind, data]); };
            var resolve = function () { throw new Error("none"); };
            var describe = function () { return ""; };
            var role = function () { return null; };
            var nameOf = function () { return ""; };
            var clip = function (t, n) { return t.slice(0, n); };
            var oneLine = function (t) { return (t || "").replace(/\s+/g, " ").trim(); };
            var modules = {};
            var helpers = {};
        "#;
    let source = format!("{STUBS}\n{OVERLAY_JS}\n{test}");
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let mut sloppy = rquickjs::context::EvalOptions::default();
    sloppy.strict = false;
    let json: String = ctx.with(|ctx| {
        ctx.eval_with_options::<String, _>(source, sloppy)
            .unwrap_or_else(|e| {
                let caught = ctx.catch();
                panic!("the overlay failed: {e:?} {caught:?}")
            })
    });
    serde_json::from_str(&json).unwrap()
}

/// The popover sits below its element, flips above it near the bottom of the viewport,
/// shifts inside the viewport's sides, and goes inside an element taller than the view.
#[test]
fn the_popover_flips_and_shifts_to_stay_in_view() {
    let got = on_overlay(
        r#"
            var view = { width: 1200, height: 800 };
            var size = { width: 320, height: 200 };
            var at = function (left, top, right, bottom, v) {
                return helpers.place({ left: left, top: top, right: right, bottom: bottom }, size, v || view);
            };
            JSON.stringify({
                below: at(100, 100, 200, 140),
                above: at(100, 700, 200, 740),
                shifted: at(1100, 100, 1180, 140),
                tall: at(0, -500, 1200, 3000),
                narrow: at(40, 100, 80, 140, { width: 300, height: 800 }),
                low: at(100, 790, 200, 900),
            })
        "#,
    );
    assert_eq!(
        got["below"],
        json!({"left": 100, "top": 148, "side": "below"})
    );
    assert_eq!(
        got["above"],
        json!({"left": 100, "top": 492, "side": "above"})
    );
    assert_eq!(
        got["shifted"],
        json!({"left": 872, "top": 148, "side": "below"})
    );
    assert_eq!(got["tall"], json!({"left": 8, "top": 8, "side": "inside"}));
    // Wider than the viewport: it starts at the margin rather than off its left edge.
    assert_eq!(got["narrow"]["left"], 8);
    // Partly off the bottom: above it, and never below the viewport.
    assert_eq!(
        got["low"],
        json!({"left": 100, "top": 582, "side": "above"})
    );
}

/// Names a build tool made up don't go into a selector meant to last.
#[test]
fn made_up_class_names_are_left_out_of_selectors() {
    let got = on_overlay(
        r#"
            var names = ["css-1x2y3z", "sc-bdVaJa", "jsx-4012", "radix-:r1:", "w-1/2", "svelte-xyz12",
                "btn-primary", "card", "hero", "nav-link", "is-active"];
            var out = {};
            for (var i = 0; i < names.length; i++) out[names[i]] = helpers.madeUp(names[i]);
            JSON.stringify(out)
        "#,
    );
    for name in [
        "css-1x2y3z",
        "sc-bdVaJa",
        "jsx-4012",
        "radix-:r1:",
        "w-1/2",
        "svelte-xyz12",
    ] {
        assert_eq!(got[name], true, "{name}");
    }
    for name in ["btn-primary", "card", "hero", "nav-link", "is-active"] {
        assert_eq!(got[name], false, "{name}");
    }
}

/// The overlay's ops are there for the app, `theme` among them, and the module loads on a
/// page with none of the DOM it draws with.
#[test]
fn the_overlay_adds_its_ops() {
    let got = on_overlay("JSON.stringify(Object.keys(modules))");
    assert_eq!(
        got,
        json!(["overlay", "pick", "markers", "flash", "theme", "presence"])
    );
}
