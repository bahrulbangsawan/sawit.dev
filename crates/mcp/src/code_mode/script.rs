//! Runs an agent's script for code mode's `run`, in QuickJS (`rquickjs`, already in the app for
//! Pele): a fresh runtime per script, no file, network, module or timer access (only the
//! functions given, which call Sawit's tools), a memory limit and a deadline.
//!
//! Boa (a JavaScript engine in Rust) was measured first; see `docs/notes/2026-11-07-mcp-audit.md`
//! for the numbers and why QuickJS runs the scripts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::{Context, Ctx, Function, Promise, Runtime};
use serde_json::Value;

/// What one script may use.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Wall time, Sawit's own calls included.
    pub time: Duration,
    /// The JavaScript heap.
    pub memory: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            time: Duration::from_secs(120),
            memory: 128 << 20,
        }
    }
}

/// A call from the script: the function's name and its arguments; the answer, or why it failed
/// (thrown in the script as an `Error`).
pub type Host = Box<dyn FnMut(&str, Value) -> Result<Value, String> + Send>;

/// What a script did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// What it returned (`None` when it returned nothing).
    pub value: Option<Value>,
    /// Why it stopped, when it threw, ran out of time or memory, or didn't compile.
    pub error: Option<String>,
    /// What it logged with `console.log` (and `info`, `warn`, `error`).
    pub logs: Vec<String>,
    /// How many Sawit functions it called.
    pub calls: usize,
}

/// The most a script's console keeps; later lines are dropped with a note.
const MAX_LOG: usize = 20_000;

/// Defines `sawit` (one function per name, and `sawit.call(name, args)`) and `console` around the
/// host's `__call` and `__log`.
const PRELUDE: &str = r#"
(() => {
  const call = globalThis.__call, log = globalThis.__log, names = globalThis.__names;
  delete globalThis.__call; delete globalThis.__log; delete globalThis.__names;
  const invoke = (name, args) => {
    if (typeof name !== "string") throw new TypeError("sawit.call takes a function name first");
    let json;
    try {
      json = JSON.stringify(args === undefined ? {} : args);
    } catch (e) {
      throw new TypeError(`${name}: its arguments can't be sent (${e.message})`);
    }
    const reply = JSON.parse(call(name, json));
    if ("error" in reply) {
      const err = new Error(reply.error);
      err.tool = name;
      throw err;
    }
    return reply.ok;
  };
  const sawit = { call: invoke };
  for (const name of JSON.parse(names)) sawit[name] = (args) => invoke(name, args);
  const show = (v) => {
    if (typeof v === "string") return v;
    if (v instanceof Error) return `${v.name}: ${v.message}`;
    try { return JSON.stringify(v); } catch { return String(v); }
  };
  const write = (...args) => log(args.map(show).join(" "));
  Object.defineProperty(globalThis, "sawit", { value: Object.freeze(sawit), enumerable: true });
  globalThis.console = { log: write, info: write, warn: write, error: write, debug: write };
})();
"#;

/// Runs `code` (the body of an async function: `return` gives the result, `await` works) with
/// `names` as `sawit.<name>()`, each call going to `host`.
///
/// A script that is one expression (`sawit.git_status({repo: "."})`) returns its value without
/// a `return`.
pub fn run(code: &str, names: &[String], host: Host, limits: Limits) -> Outcome {
    let mut outcome = Outcome::default();
    let Ok(rt) = Runtime::new() else {
        outcome.error = Some("The script engine didn't start.".into());
        return outcome;
    };
    rt.set_memory_limit(limits.memory);
    rt.set_max_stack_size(1 << 20);
    let deadline = Instant::now() + limits.time;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let Ok(cx) = Context::full(&rt) else {
        outcome.error = Some("The script engine didn't start.".into());
        return outcome;
    };

    let names_json = serde_json::to_string(names).unwrap_or_else(|_| "[]".into());
    let logs = Rc::new(RefCell::new(Console::default()));
    let calls = Rc::new(Cell::new(0usize));
    let host = Rc::new(RefCell::new(host));

    let result: Result<Option<Value>, String> = cx.with(|cx| {
        let console = logs.clone();
        let log = Function::new(cx.clone(), move |line: String| {
            console.borrow_mut().push(line)
        })
        .map_err(|e| e.to_string())?;
        let counter = calls.clone();
        let call = Function::new(cx.clone(), move |name: String, args: String| -> String {
            counter.set(counter.get() + 1);
            let reply = if Instant::now() > deadline {
                Err("The script ran out of time.".to_string())
            } else {
                match serde_json::from_str::<Value>(&args) {
                    Ok(args) => (host.borrow_mut())(&name, args),
                    Err(e) => Err(format!("{name}: its arguments aren't JSON ({e})")),
                }
            };
            match reply {
                Ok(v) => serde_json::json!({ "ok": v }),
                Err(e) => serde_json::json!({ "error": e }),
            }
            .to_string()
        })
        .map_err(|e| e.to_string())?;
        let globals = cx.globals();
        globals.set("__log", log).map_err(|e| e.to_string())?;
        globals.set("__call", call).map_err(|e| e.to_string())?;
        globals
            .set("__names", names_json.as_str())
            .map_err(|e| e.to_string())?;
        cx.eval::<(), _>(PRELUDE).map_err(|e| exception(&cx, e))?;
        run_body(&cx, code)
    });
    outcome.logs = std::mem::take(&mut logs.borrow_mut().lines);
    outcome.calls = calls.get();
    match result {
        Ok(value) => outcome.value = value,
        Err(e) => {
            outcome.error = Some(if Instant::now() > deadline {
                format!(
                    "The script ran out of time ({} s). Pass a larger timeout_secs, or do less in one run.",
                    limits.time.as_secs_f64()
                )
            } else if e.contains("out of memory") {
                format!("The script ran out of memory ({} MB).", limits.memory >> 20)
            } else {
                e
            })
        }
    }
    outcome
}

/// The script's console: its lines, up to [`MAX_LOG`] bytes.
#[derive(Default)]
struct Console {
    lines: Vec<String>,
    size: usize,
    cut: bool,
}

impl Console {
    fn push(&mut self, line: String) {
        if self.size + line.len() > MAX_LOG {
            if !self.cut {
                self.lines.push("… (more console output dropped)".into());
                self.cut = true;
            }
            return;
        }
        self.size += line.len();
        self.lines.push(line);
    }
}

/// Compiles `code` as one expression if it is one, else as a function body, and runs it to
/// the end. The answer comes back as JSON from the script itself, so any value survives.
fn run_body(cx: &Ctx<'_>, code: &str) -> Result<Option<Value>, String> {
    const DONE: &str = ".then((v) => v === undefined ? \"\\u0000undefined\" : JSON.stringify(v, (k, x) => typeof x === \"bigint\" ? String(x) : x), (e) => { throw e; })";
    let expression = code.trim().trim_end_matches(';').trim_end();
    let as_expression = format!("(async () => (\n{expression}\n))(){DONE}");
    let as_body = format!("(async () => {{\n{code}\n}})(){DONE}");
    // A compile error throws now; anything the script does wrong rejects the promise instead.
    let promise: Promise = match cx.eval::<Promise, _>(as_expression) {
        Ok(p) => p,
        Err(_) => {
            let _ = cx.catch();
            cx.eval::<Promise, _>(as_body)
                .map_err(|e| exception(cx, e))?
        }
    };
    let json: String = promise.finish().map_err(|e| exception(cx, e))?;
    if json == "\u{0}undefined" {
        return Ok(None);
    }
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|e| format!("The script's result isn't JSON: {e}"))
}

/// The pending exception as text: `Name: message` and where it was thrown.
fn exception(cx: &Ctx<'_>, error: rquickjs::Error) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let caught = cx.catch();
    if let Some(obj) = caught.as_object() {
        let name: Option<String> = obj.get("name").ok();
        let message: Option<String> = obj.get("message").ok();
        let stack: Option<String> = obj.get("stack").ok();
        let mut text = match (name, message) {
            (Some(n), Some(m)) if !m.is_empty() => format!("{n}: {m}"),
            (Some(n), _) => n,
            (None, Some(m)) => m,
            (None, None) => "The script threw.".into(),
        };
        if let Some(stack) = stack.filter(|s| !s.trim().is_empty()) {
            // Lines of the wrapper are line 1 off; say where in the script, not the wrapper.
            text.push('\n');
            text.push_str(stack.trim_end());
        }
        return text;
    }
    if let Some(s) = caught.as_string().and_then(|s| s.to_string().ok()) {
        return s;
    }
    if caught.is_null() || caught.is_undefined() {
        // The interrupt handler's uncatchable error, or one the engine raised without a value.
        return "The script was stopped.".into();
    }
    format!("The script threw {caught:?}")
}
