//! An agent's `browser_evaluate` on the page: the script that runs it, and the form it takes
//! there (an expression or a function body), picked by compiling it in QuickJS first.

/// The script that runs an agent's `browser_evaluate` script on the page: an expression,
/// or (`body`) a function body that uses `return`. WebKit compiles the script with this
/// call, and the page agent only runs it: compiled by the agent (`new Function`), it failed
/// on pages whose Content-Security-Policy forbids `eval`. [`evaluate_as_body`] picks the form.
pub fn evaluate_call(script: &str, body: bool) -> String {
    // On its own line: a script ending in a `//` comment still closes.
    let function = if body {
        format!("function () {{\n{script}\n}}")
    } else {
        format!("function () {{ return (\n{script}\n); }}")
    };
    format!(
        "(() => {{ const a = window.__sawitAgent; return a ? a.run({{ op: \"evaluate\", fn: {function} }}) : \"\"; }})()"
    )
}

/// Whether an agent's `browser_evaluate` script goes to WebKit as a function body at once:
/// its expression form ([`evaluate_call`]) doesn't compile and its body form does. WebKit
/// reports a failed compile to the page as an uncaught `SyntaxError` of the page's own (in
/// its console, to `window.onerror`, in the notes' console errors), so the form is picked
/// here first, by QuickJS compiling each call without running it. `false` when the
/// expression compiles, and when neither does (a real error, or syntax QuickJS lacks):
/// WebKit then tries the expression, and the body after its `SyntaxError`.
pub fn evaluate_as_body(script: &str) -> bool {
    let Ok(rt) = rquickjs::Runtime::new() else {
        return false;
    };
    rt.set_memory_limit(COMPILE_MEMORY);
    let Ok(cx) = rquickjs::Context::full(&rt) else {
        return false;
    };
    cx.with(|cx| {
        !compiles(&cx, &evaluate_call(script, false)) && compiles(&cx, &evaluate_call(script, true))
    })
}

/// What compiling an agent's script may take; past it, the script counts as not compiling.
const COMPILE_MEMORY: usize = 64 << 20;

/// Whether `source` compiles as a script, the way WebKit compiles one (sloppy mode). Nothing
/// in it runs: the text is an agent's, and could close any wrapper put around it.
pub(super) fn compiles(cx: &rquickjs::Ctx<'_>, source: &str) -> bool {
    use rquickjs::qjs;

    let Ok(text) = std::ffi::CString::new(source) else {
        return false;
    };
    let flags = (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_COMPILE_ONLY) as i32;
    let ctx = cx.as_raw().as_ptr();
    // SAFETY: `ctx` is the live context `cx` borrows. `JS_Eval` reads `input[input_len]`,
    // which is the NUL `CString` ends with. It returns an owned value, the compiled script
    // or the exception marker; the script, or the exception taken from the context, is freed
    // here once, so the runtime is left holding nothing.
    unsafe {
        let compiled = qjs::JS_Eval(
            ctx,
            text.as_ptr(),
            text.as_bytes().len() as _,
            c"evaluate".as_ptr(),
            flags,
        );
        if qjs::JS_IsException(compiled) {
            qjs::JS_FreeValue(ctx, qjs::JS_GetException(ctx));
            return false;
        }
        qjs::JS_FreeValue(ctx, compiled);
        true
    }
}
