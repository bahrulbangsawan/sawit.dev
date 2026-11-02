//! What an agent's op does, in words: the caption while it works, the sentence after, and
//! whether it takes long enough to show a placeholder.

use serde_json::Value;

/// What an agent does with one op, in words: `doing` for its caption while it works ("adding 3
/// stickies"), `done` for the sentence the app announces after ("added 3 stickies").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub doing: String,
    pub done: String,
}

impl Action {
    fn new(doing: impl Into<String>, done: impl Into<String>) -> Self {
        Self {
            doing: doing.into(),
            done: done.into(),
        }
    }

    /// `doing` and `done` of one verb on one object: "adding" and "added" "3 stickies".
    fn of(doing: &str, done: &str, object: &str) -> Self {
        Self::new(format!("{doing} {object}"), format!("{done} {object}"))
    }
}

/// `n` things of a kind: "a sticky", "3 stickies".
fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("{} {one}", if starts_with_vowel(one) { "an" } else { "a" }),
        n => format!("{n} {many}"),
    }
}

fn starts_with_vowel(word: &str) -> bool {
    word.starts_with(['a', 'e', 'i', 'o', 'u'])
}

/// What an element of `add` is called, one and many, by its `type`.
fn element_kind(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "sticky" | "note" => ("sticky", "stickies"),
        "rectangle" | "ellipse" | "diamond" | "shape" | "triangle" => ("shape", "shapes"),
        "text" => ("text", "texts"),
        "arrow" => ("arrow", "arrows"),
        "line" => ("line", "lines"),
        "frame" => ("frame", "frames"),
        "section" => ("section", "sections"),
        "image" => ("image", "images"),
        "freedraw" | "ink" => ("drawing", "drawings"),
        _ => ("element", "elements"),
    }
}

/// The items of an op's `elements`.
fn elements(op: &Value) -> &[Value] {
    op.get("elements")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// What an agent's canvas op (`sawit_board::ops`) does, in words: "adding 3 stickies", "moving
/// 2 layers", "drawing a diagram", "running align.left".
pub fn canvas_action(op: &Value) -> Action {
    let name = op.get("op").and_then(Value::as_str).unwrap_or_default();
    match name {
        "add" => {
            let items = elements(op);
            let kinds: Vec<&str> = items
                .iter()
                .map(|e| e.get("type").and_then(Value::as_str).unwrap_or_default())
                .map(|kind| element_kind(kind).0)
                .collect();
            let (one, many) = match kinds.first() {
                Some(first) if kinds.iter().all(|k| k == first) => {
                    let kind = items[0].get("type").and_then(Value::as_str);
                    element_kind(kind.unwrap_or_default())
                }
                _ => ("element", "elements"),
            };
            Action::of("adding", "added", &count(items.len(), one, many))
        }
        "update" => {
            let items = elements(op);
            let has = |keys: &[&str]| {
                items
                    .iter()
                    .any(|e| keys.iter().any(|k| e.get(*k).is_some()))
            };
            let layers = count(items.len(), "layer", "layers");
            if has(&["text", "label"]) {
                Action::new("editing text", "edited text")
            } else if has(&["width", "height"]) {
                Action::of("resizing", "resized", &layers)
            } else if has(&["x", "y"]) {
                Action::of("moving", "moved", &layers)
            } else if has(&["backgroundColor", "strokeColor"]) {
                Action::of("recolouring", "recoloured", &layers)
            } else {
                Action::of("updating", "updated", &layers)
            }
        }
        "delete" => {
            let n = op.get("ids").and_then(Value::as_array).map_or(0, Vec::len);
            Action::of("deleting", "deleted", &count(n, "layer", "layers"))
        }
        "clear" => Action::new("clearing the canvas", "cleared the canvas"),
        "replace" => Action::new("loading a canvas", "loaded a canvas"),
        "mermaid" => Action::new("drawing a diagram", "drew a diagram"),
        "get" => Action::new("reading the canvas", "read the canvas"),
        "export_svg" | "export_png" => Action::new("exporting the page", "exported the page"),
        "key" => {
            let key = op.get("key").and_then(Value::as_str).unwrap_or("a key");
            Action::of("pressing", "pressed", key)
        }
        "type" => Action::new("typing", "typed"),
        "screen_write" => Action::new("writing a screen", "wrote a screen"),
        "screen_get" | "screen" => Action::new("reading a screen", "read a screen"),
        "pointer" => Action::new("using the pointer", "used the pointer"),
        "command" => {
            let id = op
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let params = op.get("params").unwrap_or(&Value::Null);
            match id {
                "command.batch" => {
                    let n = params
                        .get("steps")
                        .or_else(|| params.get("commands"))
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Action::of("running", "ran", &count(n, "step", "steps"))
                }
                "image.place" => Action::new("placing an image", "placed an image"),
                "file.import" => Action::new("importing a file", "imported a file"),
                "" => Action::new("running a command", "ran a command"),
                id => Action::of("running", "ran", id),
            }
        }
        _ => Action::new("working", "worked"),
    }
}

/// The path of `url` as an agent's caption names a page it opens: `/checkout`, or the host for
/// its root, cut to 40 characters.
pub fn page_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let name = match path {
        "" => host.to_string(),
        path => format!("/{path}"),
    };
    let name = match name.chars().count() > 40 {
        true => format!("{}…", name.chars().take(39).collect::<String>()),
        false => name,
    };
    if name.is_empty() {
        url.to_string()
    } else {
        name
    }
}

/// What an agent's browser op does, in words, as the app tells it without the page: the page's
/// overlay names the element too ("clicking Sign up"), and has the last word for the ops it
/// draws.
pub fn browser_action(op: &crate::browser::BrowserOp) -> Action {
    use crate::browser::BrowserOp as Op;
    match op {
        Op::Navigate { url } => Action::of("opening", "opened", &page_name(url)),
        Op::Back => Action::new("going back", "went back"),
        Op::Forward => Action::new("going forward", "went forward"),
        Op::Reload => Action::new("reloading the page", "reloaded the page"),
        Op::Click { .. } => Action::new("clicking", "clicked"),
        Op::Hover { .. } => Action::new("pointing", "pointed"),
        Op::Type { .. } => Action::new("typing", "typed"),
        Op::Press { key } => Action::of("pressing", "pressed", key),
        Op::Select { .. } => Action::new("choosing an option", "chose an option"),
        Op::Scroll { .. } => Action::new("scrolling", "scrolled"),
        Op::Screenshot { .. } => Action::new("taking a screenshot", "took a screenshot"),
        Op::Snapshot => Action::new("reading the page", "read the page"),
        Op::Query { .. } | Op::Settled { .. } => Action::new("waiting", "waited"),
        Op::Evaluate { .. } => Action::new("running a script", "ran a script"),
        Op::Resize { .. } => Action::new("resizing the page", "resized the page"),
        Op::Inspect { .. } => Action::new("inspecting", "inspected"),
        Op::Design { .. } | Op::ExportDesign { .. } | Op::ExportStatus => {
            Action::new("reading the design", "read the design")
        }
        Op::State => Action::new("checking the page", "checked the page"),
        Op::Console { .. } | Op::Network { .. } => Action::new("reading the logs", "read the logs"),
        Op::Annotations { .. } => Action::new("reading your notes", "read your notes"),
        Op::Devtools | Op::Panel { .. } => Action::new("working", "worked"),
    }
}

/// Whether what an agent's browser op did is worth saying to a screen reader: what changes the
/// page (opening one, clicking, typing), not what only reads it, which an agent does many times
/// a second while it waits for a page.
pub fn browser_op_changes_the_page(op: &crate::browser::BrowserOp) -> bool {
    use crate::browser::BrowserOp as Op;
    matches!(
        op,
        Op::Navigate { .. }
            | Op::Back
            | Op::Forward
            | Op::Reload
            | Op::Click { .. }
            | Op::Hover { .. }
            | Op::Type { .. }
            | Op::Press { .. }
            | Op::Select { .. }
            | Op::Scroll { .. }
            | Op::Resize { .. }
    )
}

/// How many steps of a `command.batch` make it one that takes noticeable time.
pub const SLOW_BATCH: usize = 20;
/// How many elements of an `add` make it one that takes noticeable time.
pub const SLOW_ADD: usize = 50;

/// Whether an agent's canvas op takes noticeable time, and so shows a placeholder where its
/// result will land until it does: a Mermaid diagram's layout, a canvas loaded in, a file
/// imported or an image placed, a `command.batch` of [`SLOW_BATCH`] steps or more, and an `add`
/// of [`SLOW_ADD`] elements or more, or of an image.
pub fn slow_canvas_op(op: &Value) -> bool {
    let name = op.get("op").and_then(Value::as_str).unwrap_or_default();
    match name {
        "mermaid" | "replace" => true,
        "add" => {
            let items = elements(op);
            items.len() >= SLOW_ADD
                || items
                    .iter()
                    .any(|e| e.get("type").and_then(Value::as_str) == Some("image"))
        }
        "command" => {
            let id = op
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let params = op.get("params").unwrap_or(&Value::Null);
            match id {
                "image.place" | "file.import" => true,
                "command.batch" => params
                    .get("steps")
                    .or_else(|| params.get("commands"))
                    .and_then(Value::as_array)
                    .is_some_and(|steps| steps.len() >= SLOW_BATCH),
                _ => false,
            }
        }
        _ => false,
    }
}
