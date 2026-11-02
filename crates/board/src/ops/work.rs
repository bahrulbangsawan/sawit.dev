//! Work items: the components agents build on the canvas, each in a frame of its own
//! (`sawit_core::agents::work`). `begin` makes the frame and names the work, an op that passes a
//! `work` runs inside that frame ([`in_work`]), and `progress` checks the work is there: with the
//! app closed nothing shows progress, so it changes nothing and says so. The app answers
//! `progress` itself, and draws the work's row and badge.
//!
//! Also the commands an agent may not run ([`agent_refusal`]): the ones that move the view or the
//! page the user has on show.

use std::sync::atomic::{AtomicU64, Ordering};

use figcraft_doc::{Document, NodeId};
use sawit_core::agents::work::{
    WORK_KEY, WORK_KIND_KEY, WORK_STEPS_KEY, WorkStatus, check_kind, new_work_id,
};
use serde_json::{Map, Value, json};

use super::{BATCH, failure};
use crate::doc::BoardDoc;
use crate::skeleton;

/// A frame's size when `begin` names none (page units).
pub const WORK_SIZE: (f64, f64) = (320.0, 240.0);
/// How far from the layer it is `near` a frame goes (page units): room for its row above it.
const NEAR_GAP: f64 = 48.0;

/// What `progress` says with the app closed.
pub const PROGRESS_UNSHOWN: &str = "Sawit isn't running, so nothing shows this progress: the \
     frame and what was drawn in it are saved, and the canvas shows each work's progress while \
     the app runs";

/// The frame of work `work` in `doc`: the layer whose plugin data names it ([`WORK_KEY`]).
pub fn work_frame(doc: &Document, work: &str) -> Option<NodeId> {
    doc.nodes()
        .filter(|node| node.plugin_data.get(WORK_KEY).map(String::as_str) == Some(work))
        .map(|node| node.id)
        .max()
}

/// The steps a work's frame keeps ([`WORK_STEPS_KEY`]).
pub fn work_steps(doc: &Document, frame: NodeId) -> Vec<String> {
    (doc.node(frame))
        .and_then(|node| node.plugin_data.get(WORK_STEPS_KEY))
        .and_then(|steps| serde_json::from_str(steps).ok())
        .unwrap_or_default()
}

/// The `work` an op names, if it names one: `Err` when it isn't a string.
pub fn work_of(op: &Value) -> Result<Option<&str>, String> {
    match op.get("work") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(work)) if !work.trim().is_empty() => Ok(Some(work.trim())),
        Some(_) => Err("`work` must be the id canvas_begin answered, like \"w_7f3a91c2\"".into()),
    }
}

/// What an op says when its work's frame isn't on the canvas.
fn not_on_the_canvas(work: &str) -> String {
    format!(
        "work “{work}” isn't on the canvas: its frame was deleted, or it was never begun. \
         canvas_begin starts one"
    )
}

/// The `begin` op (`{name, frame: {x, y, width?, height?} | {near, side?, width?, height?},
/// steps?, kind?}`): a frame named `name` where `frame` says, marked as a new work's, in one undo
/// step and with the selection left as it is. It answers `{ok, work, frame, page, name, steps}`.
pub(super) fn begin(board: &mut BoardDoc, op: &Value) -> Value {
    match begin_work(board, op) {
        Ok(reply) => reply,
        Err(error) => failure(format!("begin: {error}")),
    }
}

fn begin_work(board: &mut BoardDoc, op: &Value) -> Result<Value, String> {
    let name = (op.get("name").and_then(Value::as_str).map(str::trim))
        .filter(|name| !name.is_empty())
        .ok_or("it needs the component's `name`, like \"Pricing card\"")?;
    let kind = check_kind(op.get("kind").and_then(Value::as_str))?;
    let steps: Vec<String> = match op.get("steps") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(steps)) => (steps.iter())
            .map(|step| step.as_str().map(str::trim).map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or("`steps` must be a list of words, like [\"Laying out\", \"Styling\"]")?,
        Some(_) => return Err("`steps` must be a list of words".into()),
    };
    let doc = board.doc().map_err(|e| e.to_string())?;
    let shown = board.session.page().map_err(|e| e.to_string())?;
    let (page, [x, y, width, height]) = place(&doc, shown, op.get("frame"))?;
    let work = new_work_id(seed());
    let data = json!({
        (WORK_KEY): work,
        (WORK_KIND_KEY): kind,
        (WORK_STEPS_KEY): serde_json::to_string(&steps).unwrap_or_default(),
    });
    let args = json!({
        "x": x, "y": y, "width": width, "height": height, "name": name,
        "parent": page, "select": false, "props": {"pluginData": data},
    });
    let made = on_page(board, page, |board| board.execute("frame.create", &args))
        .map_err(|e| e.to_string())?;
    let frame = made.get("id").and_then(figcraft_engine::params::id_of);
    let frame = frame.ok_or("the frame wasn't made")?;
    Ok(json!({
        "ok": true, "work": work, "frame": frame.to_string(), "page": page.to_string(),
        "name": name, "steps": steps,
    }))
}

/// A seed for a new work's id: the time, the process and a count.
fn seed() -> u128 {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    let nanos = now.map_or(0, |d| d.as_nanos());
    nanos
        ^ (u128::from(std::process::id()) << 64)
        ^ u128::from(COUNT.fetch_add(1, Ordering::Relaxed))
}

/// Where `begin`'s `frame` puts the frame, and on which page: at its `x`, `y` on the page on show
/// (`shown`), or beside the layer it is `near` on that layer's page, on its `side` (`right`, the
/// default, `left`, `above` or `below`). Its size is `width` and `height`, else [`WORK_SIZE`].
fn place(
    doc: &Document,
    shown: NodeId,
    frame: Option<&Value>,
) -> Result<(NodeId, [f64; 4]), String> {
    let frame = frame.and_then(Value::as_object).ok_or(
        "it needs a `frame`: {\"x\": 0, \"y\": 0, \"width\": 320, \"height\": 240}, or \
         {\"near\": \"<id>\", \"side\": \"right\"}",
    )?;
    let number = |key: &str| {
        frame
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
    };
    let size = |key: &str, default: f64| number(key).filter(|n| *n > 0.0).unwrap_or(default);
    let (w, h) = (size("width", WORK_SIZE.0), size("height", WORK_SIZE.1));
    if let Some(near) = frame.get("near") {
        let id = match near {
            Value::String(id) => id.clone(),
            Value::Number(id) => id.to_string(),
            _ => return Err("`frame.near` must be a layer's id".into()),
        };
        let layer = skeleton::resolve(doc, &id)
            .filter(|layer| doc.node(*layer).is_some_and(|n| !n.is_page()))
            .ok_or_else(|| format!("`frame.near`: no layer has the id “{id}”"))?;
        let page = doc
            .page_of(layer)
            .ok_or("`frame.near` names a layer on no page")?;
        let b = doc
            .world_bounds(layer)
            .ok_or("`frame.near` names a layer with no place")?;
        let side = frame.get("side").and_then(Value::as_str).unwrap_or("right");
        let (x, y) = match side.trim().to_ascii_lowercase().as_str() {
            "right" => (b.x1 + NEAR_GAP, b.y0),
            "left" => (b.x0 - NEAR_GAP - w, b.y0),
            "above" => (b.x0, b.y0 - NEAR_GAP - h),
            "below" => (b.x0, b.y1 + NEAR_GAP),
            other => {
                return Err(format!(
                    "`frame.side` “{other}” isn't one: right, left, above or below"
                ));
            }
        };
        return Ok((page, [x, y, w, h]));
    }
    match (number("x"), number("y")) {
        (Some(x), Some(y)) => Ok((shown, [x, y, w, h])),
        _ => Err("`frame` needs its `x` and `y` (page units), or the `near` layer".into()),
    }
}

/// The `progress` op as the canvas file answers it with the app closed: the work has to be there,
/// and its status one there is, but nothing shows it, so it changes nothing and says so.
pub(super) fn progress(board: &mut BoardDoc, op: &Value) -> Value {
    let checked = (|| {
        let work = work_of(op)?.ok_or("it needs the `work` canvas_begin answered")?;
        let doc = board.doc().map_err(|e| e.to_string())?;
        work_frame(&doc, work).ok_or_else(|| not_on_the_canvas(work))?;
        let message = op.get("message").and_then(Value::as_str);
        let status = op.get("status").and_then(Value::as_str);
        let status = status.map(|s| WorkStatus::parse(s, message)).transpose()?;
        Ok::<_, String>(json!({
            "ok": true, "work": work, "status": status.map(|s| s.name()),
            "step": op.get("step"), "shown": false, "note": PROGRESS_UNSHOWN,
        }))
    })();
    checked.unwrap_or_else(|error| failure(format!("progress: {error}")))
}

/// Runs `op` with `run`, inside the frame of the work it names, if it names one: on that frame's
/// page, whichever is on show, and with what it places placed in the frame. An `add`'s elements
/// go into the frame (their `frameId`, when they name none), and the `x` and `y` of an `add`, an
/// `update` and a `mermaid` are from the frame's top-left corner. An op with no work runs as it
/// is; one whose work isn't on the canvas changes nothing.
pub(super) fn in_work(
    board: &mut BoardDoc,
    op: &Value,
    run: impl FnOnce(&mut BoardDoc, &Value) -> Value,
) -> Value {
    let name = op.get("op").and_then(Value::as_str).unwrap_or("op");
    let work = match work_of(op) {
        Ok(Some(work)) => work.to_string(),
        Ok(None) => return run(board, op),
        Err(error) => return failure(format!("{name}: {error}")),
    };
    let found = board.doc().ok().and_then(|doc| {
        let frame = work_frame(&doc, &work)?;
        let b = doc.world_bounds(frame)?;
        Some((frame, doc.page_of(frame)?, (b.x0, b.y0)))
    });
    let Some((frame, page, origin)) = found else {
        return failure(format!("{name}: {}", not_on_the_canvas(&work)));
    };
    let op = placed_in(op, frame, origin);
    let mut reply = on_page(board, page, |board| run(board, &op));
    if reply.get("ok") == Some(&Value::Bool(true)) {
        reply["work"] = json!(work);
    }
    reply
}

/// `op`, its places moved from the corner `origin` of the work's `frame` to the page, and an
/// `add`'s elements and a diagram put into the frame.
fn placed_in(op: &Value, frame: NodeId, (ox, oy): (f64, f64)) -> Value {
    let mut op = op.clone();
    let shift = |item: &mut Map<String, Value>, x: &str, y: &str| {
        for (key, by) in [(x, ox), (y, oy)] {
            if let Some(n) = item.get(key).and_then(Value::as_f64) {
                item.insert(key.into(), json!(n + by));
            }
        }
    };
    match op.get("op").and_then(Value::as_str) {
        Some("add") => {
            for item in (op["elements"].as_array_mut().into_iter().flatten())
                .filter_map(Value::as_object_mut)
            {
                item.entry("x").or_insert(json!(0.0));
                item.entry("y").or_insert(json!(0.0));
                shift(item, "x", "y");
                for end in ["start", "end"] {
                    if let Some(end) = item.get_mut(end).and_then(Value::as_object_mut) {
                        shift(end, "x", "y");
                    }
                }
                item.entry("frameId").or_insert(json!(frame.to_string()));
            }
        }
        Some("update") => {
            for item in (op["elements"].as_array_mut().into_iter().flatten())
                .filter_map(Value::as_object_mut)
            {
                shift(item, "x", "y");
            }
        }
        Some("mermaid") => {
            if let Some(item) = op.as_object_mut() {
                item.entry("x").or_insert(json!(0.0));
                item.entry("y").or_insert(json!(0.0));
                shift(item, "x", "y");
                item.insert(INTO_FRAME.into(), json!(frame.to_string()));
            }
        }
        _ => {}
    }
    op
}

/// The key of a `mermaid` op that names the frame its diagram goes into, set by [`in_work`].
pub(super) const INTO_FRAME: &str = "intoFrame";

/// Runs `f` with page `page` on show in `board`'s session, then puts back the page that was:
/// an agent's op on a work on another page than the user's changes nothing they see.
fn on_page<T>(board: &mut BoardDoc, page: NodeId, f: impl FnOnce(&mut BoardDoc) -> T) -> T {
    let shown = board.session.ds().ok().map(|ds| ds.page);
    let swap = shown.is_some_and(|shown| shown != page);
    if swap && let Ok(ds) = board.session.ds_mut() {
        ds.page = page;
    }
    let result = f(board);
    if swap
        && let (Some(shown), Ok(ds)) = (shown, board.session.ds_mut())
        && ds.doc.pages.contains(&shown)
    {
        ds.page = shown;
    }
    result
}

/// Why an agent's op may not run: a `command` (or a step of a `command.batch`) that moves the
/// view (`view.*`) or shows another page (`page.select`). The view and the page on show are the
/// user's; agents address pages and layers by id instead. `None` for any other op.
pub fn agent_refusal(op: &Value) -> Option<String> {
    if op.get("op").and_then(Value::as_str) != Some("command") {
        return None;
    }
    let id = op
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    refused_command(id, op.get("params").unwrap_or(&Value::Null))
}

fn refused_command(id: &str, params: &Value) -> Option<String> {
    if id.starts_with("view.") || id == "page.select" {
        return Some(format!(
            "{id} moves the view or the page the user has on show, which agents leave to the \
             user: address pages and layers by id instead (page.list, node.get {{id}}, a \
             command's parent)"
        ));
    }
    if id != BATCH {
        return None;
    }
    let steps = params.get("steps").or_else(|| params.get("commands"));
    (steps.and_then(Value::as_array).into_iter().flatten()).find_map(|step| {
        let id = step.get("command").and_then(Value::as_str)?;
        refused_command(id, step.get("params").unwrap_or(&Value::Null))
    })
}
