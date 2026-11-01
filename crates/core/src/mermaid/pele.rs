//! Pele on its own thread, in an embedded QuickJS engine that loads the bundle once.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use super::{
    MAX_EDGES, MEMORY_LIMIT, OUTPUT_LIMIT, PELE_JS, RENDER_TIMEOUT, Raw, SOURCE_LIMIT,
    TOOK_TOO_LONG, unsupported,
};

struct Job {
    source: String,
    reply: mpsc::Sender<Result<Raw, String>>,
}

/// Sends `source` to the Pele thread, starting it on first use.
pub(super) fn run(source: String) -> Result<Raw, String> {
    static WORKER: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("sawit-mermaid".into())
            .spawn(move || worker(rx))
            .expect("failed to spawn mermaid thread");
        tx
    });
    let stopped = || "The diagram renderer stopped.".to_string();
    let (reply, result) = mpsc::channel();
    worker.send(Job { source, reply }).map_err(|_| stopped())?;
    result.recv().map_err(|_| stopped())?
}

fn worker(jobs: mpsc::Receiver<Job>) {
    use rquickjs::{Context, Runtime};

    let deadline: Arc<Mutex<Option<Instant>>> = Arc::default();
    let engine = (|| {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(MEMORY_LIMIT);
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            d.lock()
                .ok()
                .and_then(|d| *d)
                .is_some_and(|d| Instant::now() > d)
        })));
        let cx = Context::full(&rt).map_err(|e| e.to_string())?;
        cx.with(|cx| {
            cx.eval::<(), _>(PELE_JS)
                .map_err(|e| exception(&cx, e, "Pele failed to load", ""))
        })?;
        Ok::<_, String>((rt, cx))
    })();
    let set_deadline = |at| {
        if let Ok(mut d) = deadline.lock() {
            *d = at;
        }
    };
    for job in jobs {
        let result = match &engine {
            Ok((_, cx)) => {
                set_deadline(Some(Instant::now() + RENDER_TIMEOUT));
                let result = cx.with(|cx| call_render(&cx, &job.source));
                set_deadline(None);
                result
            }
            Err(e) => Err(e.clone()),
        };
        let _ = job.reply.send(result);
    }
}

fn call_render(cx: &rquickjs::Ctx<'_>, source: &str) -> Result<Raw, String> {
    use rquickjs::{Function, Object};

    let err = |e| exception(cx, e, "The diagram can't be drawn", source);
    let pele: Object = cx.globals().get("pele").map_err(err)?;
    let render: Function = pele.get("render").map_err(err)?;
    let options = Object::new(cx.clone()).map_err(err)?;
    // A fixed pixel size: the preview scales it down to fit.
    options.set("responsive", false).map_err(err)?;
    options.set("idPrefix", "mermaid").map_err(err)?;
    options.set("limit", SOURCE_LIMIT).map_err(err)?;
    options.set("maxEdges", MAX_EDGES).map_err(err)?;
    options.set("outputLimit", OUTPUT_LIMIT).map_err(err)?;
    let result: Object = render.call((source, options)).map_err(err)?;
    Ok(Raw {
        svg: result.get("svg").map_err(err)?,
        width: result.get("width").map_err(err)?,
        height: result.get("height").map_err(err)?,
        kind: result.get("type").unwrap_or_default(),
    })
}

/// What went wrong, in Sawit's words where Pele's would leave the reader
/// guessing, from the pending JS exception: a `PeleError`'s `code` and
/// `message`, or `fallback` with the engine's error.
///
/// - `unsupported-diagram`: the type isn't one Pele draws ([`unsupported`]).
/// - `limit`: the diagram is too big, and Pele's message says which limit.
/// - `syntax` and `semantic`: Pele's own message, which quotes the line.
fn exception(cx: &rquickjs::Ctx<'_>, e: rquickjs::Error, fallback: &str, source: &str) -> String {
    if !e.is_exception() {
        return format!("{fallback}: {e}");
    }
    let thrown = cx.catch();
    let field = |name: &str| {
        thrown
            .as_object()
            .and_then(|o| o.get::<_, String>(name).ok())
            .filter(|m| !m.is_empty())
    };
    let message = field("message")
        .or_else(|| thrown.as_string().and_then(|s| s.to_string().ok()))
        .filter(|m| !m.is_empty());
    match (field("code").as_deref(), message) {
        // QuickJS throws "interrupted" once the deadline passes.
        (_, Some(m)) if m == "interrupted" => format!("{fallback}: {TOOK_TOO_LONG}"),
        (Some("unsupported-diagram"), _) => unsupported(source),
        (Some("limit"), Some(m)) => format!("The diagram is too big to draw: {m}"),
        (_, Some(m)) => m,
        (_, None) => format!("{fallback}."),
    }
}
