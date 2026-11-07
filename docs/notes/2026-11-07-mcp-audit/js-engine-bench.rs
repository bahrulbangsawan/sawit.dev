use std::time::Instant;

const SCRIPT: &str = r#"
(async () => {
  const rows = [];
  for (let i = 0; i < 200; i++) {
    const r = JSON.parse(host(JSON.stringify({ tool: "git_status", i })));
    rows.push({ i: r.i, ok: r.ok, files: r.files.length });
  }
  const big = Array.from({ length: 2000 }, (_, i) => ({ id: i, name: "row" + i, tags: ["a", "b"] }));
  const s = JSON.stringify(big);
  const back = JSON.parse(s);
  const total = back.filter(r => r.id % 3 === 0).map(r => r.name.length).reduce((a, b) => a + b, 0);
  const { files } = JSON.parse(host("{}"));
  return JSON.stringify({ calls: rows.length, total, files: files.length });
})()
"#;

fn host_reply(arg: &str) -> String {
    let i = arg.find("\"i\":").map(|p| arg[p + 4..].trim_end_matches('}').to_string()).unwrap_or("0".into());
    format!("{{\"ok\":true,\"i\":{i},\"files\":[\"a.rs\",\"b.rs\",\"c.rs\"]}}")
}

#[cfg(feature = "qjs")]
fn run() -> String {
    use rquickjs::{Context, Function, Promise, Runtime};
    let rt = Runtime::new().unwrap();
    rt.set_memory_limit(64 << 20);
    let cx = Context::full(&rt).unwrap();
    cx.with(|cx| {
        let f = Function::new(cx.clone(), |a: String| host_reply(&a)).unwrap();
        cx.globals().set("host", f).unwrap();
        let p: Promise = cx.eval(SCRIPT).unwrap();
        p.finish::<String>().unwrap()
    })
}

#[cfg(feature = "boa")]
fn run() -> String {
    use boa_engine::{Context, JsValue, NativeFunction, Source, js_string, object::builtins::JsPromise};
    let mut cx = Context::default();
    cx.register_global_builtin_callable(js_string!("host"), 1, NativeFunction::from_fn_ptr(|_, args, cx| {
        let a = args.first().cloned().unwrap_or(JsValue::undefined()).to_string(cx)?.to_std_string_escaped();
        Ok(JsValue::from(js_string!(host_reply(&a))))
    })).unwrap();
    let v = cx.eval(Source::from_bytes(SCRIPT)).unwrap();
    let p = JsPromise::from_object(v.as_object().unwrap().clone()).unwrap();
    cx.run_jobs().unwrap();
    let r = p.state();
    match r { boa_engine::builtins::promise::PromiseState::Fulfilled(v) => v.to_string(&mut cx).unwrap().to_std_string_escaped(), s => format!("{s:?}") }
}

#[cfg(not(any(feature = "qjs", feature = "boa")))]
fn run() -> String { host_reply("{}") }

fn main() {
    let t = Instant::now();
    let first = run();
    let first_t = t.elapsed();
    let t = Instant::now();
    for _ in 0..20 { run(); }
    println!("{first}\nfirst run (cold): {:?}\nmean of 20 runs: {:?}", first_t, t.elapsed() / 20);
    #[cfg(feature = "qjs")]
    {
        let t = Instant::now();
        for _ in 0..100 { let rt = rquickjs::Runtime::new().unwrap(); let _c = rquickjs::Context::full(&rt).unwrap(); }
        println!("engine start: {:?}", t.elapsed() / 100);
    }
    #[cfg(feature = "boa")]
    {
        let t = Instant::now();
        for _ in 0..100 { let _c = boa_engine::Context::default(); }
        println!("engine start: {:?}", t.elapsed() / 100);
    }
}
