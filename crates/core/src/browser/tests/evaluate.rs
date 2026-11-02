//! `browser_evaluate`: the form its script takes, compiled and never run here, and running it
//! where the page forbids `eval`.

use serde_json::json;

use super::on_stand_in_page;
use crate::browser::evaluate::compiles;
use crate::browser::{evaluate_as_body, evaluate_call};

/// `browser_evaluate` on a page whose Content-Security-Policy forbids `eval` (helmet's
/// default `script-src 'self'`): the page's `Function` and `eval` refuse, as WebKit's do
/// there, and the script still runs, as an expression or as a function body.
#[test]
fn evaluate_runs_where_the_page_forbids_eval() {
    let run = |call: String| {
        let test = format!(
            r#"
                var refuse = function () {{ throw new EvalError("Refused to evaluate a string as JavaScript because 'unsafe-eval' is not an allowed source of script"); }};
                Function.prototype.constructor = refuse;
                globalThis.Function = refuse;
                globalThis.eval = refuse;
                document.title = "Shop";
                {call}
            "#
        );
        on_stand_in_page(&test)
    };
    assert_eq!(
        run(evaluate_call("1 + 1", false)),
        json!({"ok": true, "value": 2})
    );
    assert_eq!(
        run(evaluate_call(
            "const t = document.title;\nreturn t + '!'",
            true
        )),
        json!({"ok": true, "value": "Shop!"})
    );
    // A throw is the script's error.
    assert_eq!(run(evaluate_call("null.x", false))["ok"], json!(false));
}

/// `browser_evaluate` picks its form before WebKit compiles anything. WebKit reports a
/// compile's `SyntaxError` to the page as an error of its own (the page's console,
/// `window.onerror`, the notes' console errors), so trying the expression there first left
/// one behind for every script written as a function body.
#[test]
fn evaluate_picks_its_form_before_webkit_compiles() {
    // An expression, as WebKit tried first, also when it reads as a body too.
    for script in [
        "document.title",
        "1 // c",
        "({a: 1})",
        "{a: 1}",
        "x = 5",
        "async () => (await fetch('/a')).status",
        "`${location.host}/a`",
        "/a+/.test(document.title)",
        "010",
    ] {
        assert!(!evaluate_as_body(script), "{script}");
    }
    // A function body, at once.
    for script in [
        "return 1",
        "const t = 1; return t",
        "document.title;",
        "if (!document.body) return null\nreturn document.body.innerText // the text",
        "with (document) return title",
        "for (const a of document.links) a.remove()",
    ] {
        assert!(evaluate_as_body(script), "{script}");
    }
    // Neither compiles here (a real error, or syntax QuickJS lacks): WebKit says, trying the
    // expression first.
    for script in ["1 +", "return (", "}", "a\0b"] {
        assert!(!evaluate_as_body(script), "{script:?}");
    }
}

/// The agent's script is only compiled here, never run: it could close the wrapper and run
/// anything (loop forever) in Sawit's own QuickJS.
#[test]
fn evaluate_forms_are_compiled_not_run() {
    let rt = rquickjs::Runtime::new().unwrap();
    let cx = rquickjs::Context::full(&rt).unwrap();
    cx.with(|cx| {
        let ran = || {
            !cx.eval::<rquickjs::Value, _>("globalThis.ran")
                .unwrap()
                .is_undefined()
        };
        assert!(compiles(&cx, "globalThis.ran = true"));
        assert!(!ran());
        assert!(compiles(&cx, "globalThis.ran = true; while (true) {}"));
        assert!(!ran());
        // Compiled as WebKit compiles a script: sloppy-mode code.
        assert!(compiles(&cx, "with (document) title"));
        // A failed compile leaves nothing pending.
        assert!(!compiles(&cx, "1 +"));
        assert!(!cx.has_exception());
    });
}
