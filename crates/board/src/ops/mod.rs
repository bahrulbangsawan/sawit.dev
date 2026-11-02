//! The canvas's control ops: the `{"op": ...}` messages of the agent wire contract (spec, "Agent
//! wire contract"), run on a [`BoardDoc`] and answered as JSON.
//!
//! Every answer is `{ok: true, ...}` or `{ok: false, error}`, and an op never panics: a failure of
//! the engine, or an op that is malformed, is an `error` the agent can read, and so is a bug that
//! panics all the same ([`run_guarded`], spec D15). The ops are `get`,
//! `add`, `update`, `delete`, `clear`, `replace`, `export_svg`, `export_png`, `command`,
//! `commands`, `pointer`, `mermaid`, `key` and `type`; the others of the contract get their own
//! answer, "<op> is not supported yet".
//!
//! Each op that draws, changes or removes layers (all but `command`, which is the engine's, and
//! the input ops below) is one undo step, and when the engine refuses a part of it, none of it is
//! done. A layer is named by the id an agent gave it when it drew it, or by its node id
//! ([`skeleton::resolve`]): an agent's id wins.
//!
//! The `pointer`, `key` and `type` ops are input: they take the paths of the pointer and the
//! keyboard on the canvas (`crate::input`, and FigCraft's text editor for what is typed), so what
//! an agent does with them is what a mouse or a keyboard does there, undo steps included: what is
//! typed is part of its text edit's one step.
//! What they ask of the view is left in `session.ui_requests`, as after the same input in the
//! window, for the view to carry out.
//!
//! Every other op that changes the document first ends the text the user is typing in on the
//! canvas, as a click elsewhere does, unless it is a command that acts on that text
//! ([`end_text_edit_before_op`], ruling R-TEXTEDIT): what was typed is an undo step of its own,
//! and the op is one after it. While the user holds a drag on the canvas, or draws a path with the
//! Pen, such an op is refused instead and changes nothing ([`waits_for_the_drag`]): the drag's
//! next move would take it back, and the path's undo step would take it too.
//!
//! A canvas holds one document, and the view saves the one on show over the canvas's file, so no
//! `command` op may leave another document open or on show. The ones that do that by name are
//! refused up front ([`HANDLED_BY_SAWIT`], as the steps of a `command.batch` too); any other
//! command that does it anyway (`file.recover`, `version.duplicate`, `branch.create`...) is undone
//! and answers an error. A `command` op that fails also leaves the file's version history and
//! branches as they were ([`FileHistory`]): they are part of the file, and the engine keeps them
//! outside undo.
//!
//! Nor may a `command` op write over a canvas file. An engine command that writes where its `path`
//! says (`file.export`, `variable.export`...: [`WRITES_TO_PATH`]) is refused, as the steps of a
//! `command.batch` too, when `path` names a canvas file (`.sawit` or `.excalidraw`) or a copy
//! Sawit keeps of one ([`is_canvas_file`]): with the app closed nothing else holds the board. So
//! is a batch step of one that takes its params from another step's result, which names a path
//! only as the batch runs.
//!
//! Nor may it have the engine read a file that never ends. An engine command that reads the file
//! its params name (`file.import`, `image.place`...: [`READS_FROM_PATH`]) reads it whole, as it
//! finds it, on the canvas's UI thread: it is refused when that is a named pipe, a device such as
//! `/dev/zero` (a cloned repository can link a file to one), or a file past
//! [`crate::store::MAX_FILE`], and as a batch step when the file's name is taken from another
//! step's result. `replace` checks the canvas file it loads the same way.
//!
//! It also holds the document's side of the commands Sawit carries out itself instead of the
//! engine, which the canvas's menus and keys run ([`export_page`], [`frames_pdf`], [`copy_as`],
//! [`import_file`]).

use std::fmt::Display;

use figcraft_engine::guard;
use serde_json::{Value, json};

use crate::doc::BoardDoc;

mod command;
mod copy_as;
mod diagram;
mod elements;
mod export;
mod files;
mod gesture;
mod input_ops;
mod refusal;
mod replace;
mod routes;
mod screen;
#[cfg(test)]
mod tests;
mod work;

use command::{command, commands};
use diagram::mermaid;
use elements::{add, clear, delete, get, update};
use export::{export_png, export_svg};
use gesture::before_op;
use input_ops::{key, pointer, type_text};
use replace::replace;

pub use diagram::{PreparedDiagram, prepare_diagram, run_prepared};
pub use files::{engine_name, is_canvas_file, is_sawit_file};
pub use routes::{STEPS as ROUTE_STEPS, viewports as route_viewports};
pub use screen::{SCREEN_SIZE, export_with_pictures};
pub use work::{PROGRESS_UNSHOWN, WORK_SIZE, agent_refusal, work_frame, work_of, work_steps};

// The document's side of the commands Sawit carries out itself (spec, "Menu policy": UI-handled
// ids): the files File ▸ Export and Import from file… make and read, and what Edit ▸ Copy as
// copies. The canvas's view (crates/app/src/board/commands/) asks for files and writes them, and
// puts what is copied on the system clipboard.
pub use copy_as::{COPY_PNG_MAX_SIDE, Copied, copy_as, copy_png_scale};
pub use export::{export_page, frames_pdf};
pub use files::import_file;

/// The command that runs other commands as its steps.
const BATCH: &str = "command.batch";

/// Runs `op` (an object whose `op` is the op's name) on `board` and answers it: `{ok: true, ...}`,
/// or `{ok: false, error}`.
///
/// - `get` answers `{ok, elements}`: a summary ([`summary::summarize`]) of each top-level layer of
///   the page on show, back to front.
/// - `add` (`{elements: [Skeleton]}`) draws the elements ([`skeleton::apply`]) and answers
///   `{ok, added, ids, notes?}`: the node id of each element drawn, in the order they were given,
///   and a note for each one skipped. A skipped element has no id, so `ids[i]` is `elements[i]`
///   only when nothing before it was skipped. What an element leaves out is its default: a missing
///   `x` or `y` is 0, and an arrow or a line with no points, size or end is drawn 100 long. On a
///   design file it draws text, frames and images only: shapes, stickies, lines and ink are a
///   board's, and the refusal names the commands that draw them there instead.
/// - `update` (`{elements: [{id, ...}]}`) changes the layers the ids name, and answers
///   `{ok, updated, missing, notes?}`: how many it found, and the ids it didn't. An element can
///   change:
///   - `x`, `y`, `width` and `height`: the layer's box, in page coordinates, as `get` gives it;
///   - `text` (or `label.text`): a connector's label, the note of a sticky, the text of a shape or
///     of a text layer;
///   - `append`: text added to the end of that text (after a `text` the element sets too);
///   - `name`: the layer's name, a section's title;
///   - `backgroundColor` and `strokeColor`, by the rules `add` colours by ([`skeleton::set_colors`]).
///
///   The rest of an element is ignored. So is a `text` that a layer has no place for (ink, a
///   section), with a note.
/// - `delete` (`{ids}`) answers `{ok, deleted, missing}`. A connector's label goes with it.
/// - `clear` deletes every top-level layer of the page on show, and answers `{ok}`.
/// - `replace` (`{scene}` or `{path}`) loads a canvas in place of the canvas's pages and answers
///   `{ok, elements, notes?}`: `elements` as `get` gives them, and `notes` on what the importer
///   left out. The new canvas is an Excalidraw scene (`scene`, or an `.excalidraw` file at `path`,
///   which [`excalidraw::import`] draws), or the `.sawit` file at `path`, or a copy Sawit kept of
///   one there (`canvas.sawit.bak`..., [`is_sawit_file`]). A `scene` wins over a `path`. It runs
///   in the canvas's own session, so the canvas stays what it is (a board, with its file), and
///   undo brings the old pages back. A scene that has no `elements`, and a file
///   that can't be read, are errors that change nothing; a file that is no regular file (a named
///   pipe, a device) is one at once, unread ("replace: couldn't read <path>: ...").
/// - `export_svg` and `export_png` take no parameters and answer `{ok, svg}` and
///   `{ok, png_base64}`: the whole page on show as one file, whatever is selected, at 1×
///   ([`export_page`]). The PNG is encoded as base64. Nothing in the document changes. A page with
///   nothing on it has nothing to export, and that is the error, as is any other the engine gives.
/// - `command` (`{command, params?}`) runs engine command `command` and answers `{ok, result}`,
///   or the engine's own `error`. It refuses the commands Sawit handles itself, as the steps of a
///   `command.batch` too ("<id> is handled by Sawit"), and the commands that write a file where
///   their `path` says when that is a canvas file ("<id> would overwrite a canvas file"), or, in a
///   batch, is taken from another step's result ("<id> in a command.batch needs its params
///   written out: ..."). So are the commands that read the file their params name, when that is
///   no regular file of at most 4 GiB ("<id> can't read <path>: ..."), or, in a batch, is taken
///   from another step's result. A command that would leave the canvas with another document
///   open or on show is undone and answers "<id> would open or switch documents; the canvas
///   keeps its one document".
/// - `commands` answers `{ok, commands: [{id, label, params}]}`: the engine's commands that
///   `command` runs and that the canvas doesn't hide, with the parameters each documents.
/// - `pointer` (`{kind, x, y, shift?, alt?, cmd?}`) sends the tools a press (`down`), a move with
///   the button down (`move`), its release (`up`), a double-click (`double`) or a move without a
///   button (`hover`) at page point (`x`, `y`), with the modifier keys given held, and answers
///   `{ok, selection, tool}`: the ids selected then, and the tool on.
/// - `mermaid` (`{source, x?, y?}`) draws `source`, a diagram in Mermaid syntax of any type Pele
///   draws, as board shapes that can be edited (a chart as one picture, `crate::mermaid::WAYS`), in
///   one undo step, and answers `{ok, added, ids}`, with `notes` on what couldn't be drawn. The
///   diagram's top left corner goes to `x`, `y`; what they leave out is 100 px right of what the
///   page holds and level with its top, or 0 on an empty page. Every error starts "mermaid: ".
///   On a design file only a chart, which comes in as one picture, can be drawn.
/// - `key` (`{key, shift?, alt?, cmd?, ctrl?}`) presses `key` on the canvas, with the modifier
///   keys given held, as the keyboard does with the canvas's default keys
///   ([`input::route_default_key`]), and answers `{ok, handled}`: whether the canvas took the
///   key. The default keys are Sawit's, not the user's keymap.json: a key Sawit keeps (⌘J, the
///   terminal) or a command the canvas doesn't offer isn't the canvas's. `key` is FigCraft's name
///   for it: a letter or a digit, `Escape`, `Enter`, `Backspace`, `Delete`, `ArrowLeft`... The
///   shortcut of a command Sawit carries out itself (save, export, import, copy as) is taken,
///   though the op carries out nothing for it.
/// - `type` (`{text}`) types `text` into the text being edited, in place of its selection, as the
///   keyboard types it, and answers `{ok}`; the edit goes on. With no text edited it answers the
///   error "not editing text". A sticky or a text layer is edited once the pointer or a key
///   starts it, as with a mouse and a keyboard: the sticky tool's click, a double-click, Enter.
///
/// - `begin` (`{name, frame, steps?, kind?}`) makes a named frame for a component an agent is
///   about to build, marked as a new work's, and answers `{ok, work, frame, page, name, steps}`;
///   `progress` (`{work, step?, status?, message?}`) checks the work is on the canvas and answers
///   that nothing shows its progress, which the app shows instead ([`work`]). An op that names a
///   `work` runs inside that work's frame: `add`, `update` and `mermaid` place from the frame's
///   corner, and what `add` and `mermaid` draw goes into the frame. `update` takes `append` too,
///   text added to the end of a layer's. A `begin` with `kind: "screen"` makes a UI screen's frame
///   (1440 × 900 when the frame names no size), its folder's name free of the op's `taken`, and
///   answers `screen` and `viewport` too; `screen` (`{work?, screen?}`) finds a screen by the work
///   that made it or by name ([`screen`]). Their files are the caller's ([`crate::screens`]). A
///   `begin` with `kind: "routes"` makes a section instead, right of what the page holds when the
///   frame names no place.
/// - `routes` (`{server, routes, viewports?, name?, x?, y?, work?}`) lays a dev server's routes
///   out as route frames, a row per route and a frame per viewport, in a section named for the
///   server (a routes work's, with `work`), in one undo step, and answers `{ok, section, page,
///   name, server, frames}`; a page that isn't on this Mac is refused ([`routes`],
///   [`crate::routes`]).
///
/// While the user holds a drag on the canvas, every op that changes the document (`add`,
/// `update`, `delete`, `clear`, `replace`, `mermaid`, `begin`, `routes` and `command`) answers
/// "<op>: finish the drag on the canvas first" and changes nothing, and while the Pen draws a path
/// "<op>: finish the path the Pen draws on the canvas first" ([`waits_for_the_drag`]).
pub fn run(board: &mut BoardDoc, op: &Value) -> Value {
    run_guarded(board, op, run_unguarded)
}

/// Runs op `op` on `board` with `run`, and answers a panic in it as an error, `<op>: <what
/// panicked>`, as the engine answers one in a command ([`guard::run`], spec D15, CANVAS-CORE-9).
/// The app runs ops in the window's callbacks, where a panic can't unwind: it aborted the app with
/// every tab. What an op changes in the document is done in groups that roll back a panic of their
/// own ([`crate::doc::grouped`]); one outside them, in Sawit's own reading of the op, has changed
/// nothing yet.
fn run_guarded(
    board: &mut BoardDoc,
    op: &Value,
    run: impl FnOnce(&mut BoardDoc, &Value) -> Value,
) -> Value {
    let name = op.get("op").and_then(Value::as_str).unwrap_or("op");
    guard::run(name, || Ok(run(board, op))).unwrap_or_else(failure)
}

/// [`run`], without its guard against a panic.
fn run_unguarded(board: &mut BoardDoc, op: &Value) -> Value {
    let name = op.get("op").and_then(Value::as_str);
    if let Some(refused) = name.and_then(|name| before_op(board, name, op)) {
        return refused;
    }
    match name {
        Some("begin") => work::begin(board, op),
        Some("progress") => work::progress(board, op),
        Some("screen") => screen::screen(board, op),
        Some("routes") => routes::routes(board, op),
        _ => work::in_work(board, op, run_named),
    }
}

/// [`run_unguarded`]'s ops that act on the document as it is, once a work's frame is found.
fn run_named(board: &mut BoardDoc, op: &Value) -> Value {
    match op.get("op").and_then(Value::as_str) {
        Some("get") => get(board),
        Some("add") => add(board, op),
        Some("update") => update(board, op),
        Some("delete") => delete(board, op),
        Some("clear") => clear(board),
        Some("replace") => replace(board, op),
        Some("export_svg") => export_svg(board),
        Some("export_png") => export_png(board),
        Some("command") => command(board, op),
        Some("commands") => commands(),
        Some("pointer") => pointer(board, op),
        Some("mermaid") => mermaid(board, op),
        Some("key") => key(board, op),
        Some("type") => type_text(board, op),
        Some(other) => failure(format!("{other} is not supported yet")),
        None => failure(r#"an op needs a name, like {"op": "get"}"#),
    }
}

/// `{ok: false, error}`.
fn failure(error: impl Display) -> Value {
    json!({"ok": false, "error": error.to_string()})
}

/// `{ok: true, <key>: value}`, with `value` moved in: `json!` would serialize it into a copy of
/// its own, and a page's elements or a command's result can be large.
fn success(key: &str, value: Value) -> Value {
    let mut reply = json!({"ok": true});
    reply[key] = value;
    reply
}
