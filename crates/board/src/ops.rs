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
//! `command.batch` too, when `path` names a canvas file (`.sawit`, `.figcraft` as it was before,
//! or `.excalidraw`) or a copy Sawit keeps of one ([`is_canvas_file`]): with the app closed
//! nothing else holds the board. So is a batch step of one that takes its params from another
//! step's result, which names a path only as the batch runs.
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

use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_doc::{BoardItem, Document, ExportSetting, NodeId};
use figcraft_engine::cmd::file::export_area;
use figcraft_engine::codegen::{self, CodeLang};
use figcraft_engine::params::id_of;
use figcraft_engine::{
    EngineError, PointerKind, Result as EngineResult, Session, UiRequest, guard, textedit,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::design::export::{ExportRow, export_bytes, exported_file};
use crate::doc::{BoardDoc, BoardError, DocKind};
use crate::input::{self, KeyOutcome, Mods};
use crate::skeleton::{self, Applied, Label, Skeleton};
use crate::{clipboard, excalidraw, policy, summary};
use sawit_core::mermaid::{Palette, render};

/// The commands the `command` op refuses: they switch, open, close or write documents behind the
/// canvas's store. Sawit does these itself.
///
/// This is the cheap first check, and the ids can't be a complete list: [`Documents`] catches what
/// else a command does to the session's documents.
const HANDLED_BY_SAWIT: &[&str] = &[
    "file.new",
    "file.newBoard",
    "file.newFromTemplate",
    "file.open",
    "file.openRecent",
    "file.close",
    "file.duplicate",
    "file.save",
    "file.saveAs",
    "window.new",
    "doc.nextTab",
    "doc.previousTab",
];

/// The commands that open a document in the session. The `command` op never leaves one open
/// ([`Documents`] undoes it), so none of these can succeed through it.
const OPENS_DOCUMENTS: &[&str] = &["file.recover", "version.duplicate", "branch.open"];

/// The engine's commands that write a file where their `path` parameter says. The `command` op
/// refuses them when that is a canvas file ([`is_canvas_file`]): one mistaken export would replace
/// the board, and with the app closed nothing holds another copy of it.
///
/// These are all of them at the pinned FigCraft revision, found by reading what writes in the
/// engine: every write goes through `file_access::write`. `file.save` and `file.saveAs` are the
/// others, and Sawit's own ([`HANDLED_BY_SAWIT`]). `file.exportLayers` and `data.fill` write into a
/// folder (`dir`) instead, but name every file after a layer and add its format's extension (`png`,
/// `jpg`, `webp`, `svg`, `pdf` or `xml`), so none of the files they write is a canvas file, and
/// they need no check.
const WRITES_TO_PATH: &[&str] = &[
    "file.export",
    "file.exportSvg",
    "file.exportFramesPdf",
    "variable.export",
    "component.playground",
    "codeConnect.export",
];

/// The engine's commands that read a file one of their params names, with those params: `path`,
/// and `variables` when it is text that isn't JSON (the local variables JSON is then read from the
/// file it names). The engine reads such a file whole, as it finds it (`file_access::read`): a
/// named pipe blocks the read for ever, and a link to a device such as `/dev/zero`, which a cloned
/// repository can hold, reads until the Mac runs out of memory, on the canvas's UI thread or in
/// sawit-mcp. So the `command` op checks the file first ([`unreadable`]).
///
/// These are all of them at the pinned FigCraft revision, found by reading what reads in the
/// engine: every read goes through `file_access`, and the other readers check that what they read
/// is a file of a size they allow first (`read_capped`: `text.loadFont`, `codeConnect.import`, the
/// images and variables beside an imported file; a library's `stat`; a plugin's `read_file`).
/// `file.open` and `file.openRecent` are Sawit's own ([`HANDLED_BY_SAWIT`]), and `file.revert`
/// reads the document's own file, which the canvas's document has none of.
const READS_FROM_PATH: &[(&str, &[&str])] = &[
    ("file.import", &["path", "variables"]),
    ("file.recover", &["path"]),
    ("image.place", &["path"]),
    ("variable.import", &["variables"]),
    ("collab.recoverFile", &["path"]),
];

/// The ends of the names of the files a canvas is kept in: the one it is saved to (`.sawit`, and
/// `.figcraft`, its name before), and the old one that it was migrated from.
const CANVAS_EXTENSIONS: [&str; 3] = [".sawit", ".figcraft", ".excalidraw"];

/// What goes after that to name a copy Sawit keeps of a canvas file, or a file it writes one
/// through: the backups of the store (`.bak` and `.del.bak`, the newest of each kind, and
/// `.lossy.bak`; [`OLDER_BACKUPS`] names the older ones), and the files a save is staged in
/// (`.tmp`, and `.<name>.sawit-tmp`, the name Sawit's saves were staged at before CANVAS-CORE-1).
const CANVAS_COMPANIONS: [&str; 6] = ["", ".bak", ".del.bak", ".lossy.bak", ".tmp", ".sawit-tmp"];

/// What follows a canvas file's name in the names its saves are staged at now,
/// `.<name>.sawit-tmp-<process>-<n>` ([`sawit_core::files::staging_file`]).
const STAGED: &str = ".sawit-tmp-";

/// What follows a canvas file's name in the names of its older backups, before the number:
/// `<name>.bak.<n>` ([`crate::store::backup_file`]) and `<name>.del.bak.<n>`
/// ([`crate::store::deletion_backup_file`]).
const OLDER_BACKUPS: [&str; 2] = [".bak.", ".del.bak."];

/// Whether `path` names a canvas file: a `.sawit`, `.figcraft` or `.excalidraw` file, or one of
/// the copies and staging files that Sawit keeps beside one.
///
/// Only the name at the end of `path` counts, in any case (the files of a Mac are not told apart
/// by it), and the whole of its end: `canvas.sawit.svg` is an SVG file, not a canvas. So an
/// export to an image path is never refused. A staging file's name goes on after the canvas
/// file's, with what makes it of one save alone.
pub fn is_canvas_file(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy().to_lowercase();
    let older_backup = |extension: &str| {
        let numbered = name.trim_end_matches(|c: char| c.is_ascii_digit());
        (OLDER_BACKUPS.iter()).any(|older| {
            numbered.len() < name.len() && numbered.ends_with(&format!("{extension}{older}"))
        })
    };
    CANVAS_EXTENSIONS.iter().any(|extension| {
        (CANVAS_COMPANIONS.iter())
            .any(|companion| name.ends_with(&format!("{extension}{companion}")))
            || name.contains(&format!("{extension}{STAGED}"))
            || older_backup(extension)
    })
}

/// The command that runs other commands as its steps.
const BATCH: &str = "command.batch";

/// The clipboard's commands ([`policy::CLIPBOARD`]) that change layers when the engine runs them:
/// the `command` op ends the text being typed in before them ([`command_keeps_text_edit`]).
const ENDS_TEXT_EDIT_IN_THE_ENGINE: [&str; 4] = [
    "edit.cut",
    "edit.paste",
    "edit.pasteOver",
    "edit.pasteReplace",
];

/// How far right of what the page holds the `mermaid` op puts a diagram that names no place.
const MERMAID_GAP: f64 = 100.0;

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
///   which [`excalidraw::import`] draws), or the `.sawit` (or `.figcraft`) file at `path`, or a
///   copy Sawit kept of one there (`canvas.sawit.bak`..., [`is_sawit_file`]). A `scene` wins over a
///   `path`. It runs in the canvas's own session, so the canvas stays what it is (a board, with
///   its file), and undo brings the old pages back. A scene that has no `elements`, and a file
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
/// - `mermaid` (`{source, x?, y?}`) draws Mermaid `source` as board shapes that can be edited, in
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
/// While the user holds a drag on the canvas, every op that changes the document (`add`,
/// `update`, `delete`, `clear`, `replace`, `mermaid` and `command`) answers
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
    if let Some(name) = name
        && let Some(wait) = waits_for_the_drag(board, name)
    {
        return failure(format!("{name}: {wait}"));
    }
    if let Some(name) = name
        && let Err(error) = end_text_edit_before_op(board, name, op)
    {
        return failure(format!("{name}: {error}"));
    }
    match name {
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

/// The ops that change the document, which wait for a drag on the canvas to end
/// ([`waits_for_the_drag`]).
const CHANGES_THE_DOCUMENT: [&str; 7] = [
    "add", "update", "delete", "clear", "replace", "mermaid", "command",
];

/// What op `name` has to wait for on the canvas, if anything, said for its refusal: it changes
/// the document, and the user holds the button down there with a tool (a move, a resize, a shape
/// being drawn, a marquee...), as the engine's own `tool.place` refuses to place mid-drag; or a
/// gesture is open with no button held, which only the user ends: the path the Pen draws, between
/// its clicks, until Enter or Escape.
///
/// A gesture keeps the document it began with, and each move of it builds the document again
/// from that one (`Session::preview`), as an Escape puts that one back. So what an op did in
/// between was gone at the next move, though the op had answered ok (Plan 8 final review I-1).
/// And while a gesture is open no command is an undo step of its own: an op between the Pen's
/// clicks joined the path's step, and the ⌘Z after Enter took both away (CANVAS-CORE-8).
/// Refused, the op changes nothing, and the agent can send it again once the gesture is over.
/// The reads (`get`, `commands`, the exports) change nothing either, and the input of `pointer`,
/// `key` and `type` meets the gesture as the mouse and the keyboard do, so they go through. The
/// text being typed in is a gesture too, which the op ends first instead
/// ([`end_text_edit_before_op`]).
fn waits_for_the_drag(board: &BoardDoc, name: &str) -> Option<&'static str> {
    if !CHANGES_THE_DOCUMENT.contains(&name) {
        return None;
    }
    let session = &board.session;
    if session.tool.busy() {
        return Some("finish the drag on the canvas first");
    }
    let open = (session.ds()).is_ok_and(|ds| ds.interaction.is_some() && ds.text_edit.is_none());
    match (open, session.tool.pen.is_some()) {
        (false, _) => None,
        (true, true) => Some("finish the path the Pen draws on the canvas first"),
        (true, false) => Some("finish what is being drawn on the canvas first"),
    }
}

/// Ends the text being typed in on the canvas before op `name` (`op`) changes the document, as the
/// canvas's chrome and menus end it before their commands (ruling R-TEXTEDIT): what was typed
/// becomes an undo step of its own ("Edit text"), and the op one after it.
///
/// The engine types inside one gesture, which only the end of the edit commits, and while it is
/// open no command is an undo step of its own. An agent's op inside it joined the typing's step,
/// so the user's ⌘Z took both. An op that drops the edit without ending it (`select.none`,
/// `page.select`, deleting the text being typed in, a `replace`, whose old pages go) left the
/// gesture open, and the user's next click on a layer cancelled it: the typing and every op since
/// were gone, though each answered ok, and the autosave wrote that (Plan 7 final review I-2).
///
/// - `add`, `update`, `delete`, `clear`, `replace` and `mermaid` end the edit.
/// - `command` ends it unless the command acts on the text being typed in, or changes nothing
///   ([`command_keeps_text_edit`]: the Text menu's commands, and a copy), or is a
///   `command.batch` whose every step does. A command refused up front runs nothing, and ends
///   nothing either ([`refused`]).
/// - The reads (`get`, `commands`, the exports) leave the edit alone, and so does the input of
///   `pointer`, `key` and `type`, which meets the edit as the mouse and the keyboard do.
///
/// With no text being typed in, it does nothing.
fn end_text_edit_before_op(board: &mut BoardDoc, name: &str, op: &Value) -> Result<(), BoardError> {
    let ends = match name {
        "add" | "update" | "delete" | "clear" | "replace" | "mermaid" => true,
        "command" => op.get("command").and_then(Value::as_str).is_some_and(|id| {
            let params = op.get("params").unwrap_or(&Value::Null);
            refused(id, params).is_none() && !command_keeps_text_edit(id, params)
        }),
        _ => false,
    };
    if ends {
        input::end_text_edit(board)
    } else {
        Ok(())
    }
}

/// Whether command `id`, with `params`, runs with the text being typed in still open, as the
/// engine runs it: it acts on that text (the Text menu's commands, [`policy::keeps_text_edit`]),
/// or it is a copy, which changes nothing, or it is a `command.batch` whose every step is one of
/// these, however deeply batches are nested.
///
/// The canvas's keys and menus keep the edit open for a cut and the pastes too, because they send
/// them to the text's characters. The engine acts on layers instead (CANVAS-CORE-2): its cut, and
/// its paste to replace, delete the text being typed in without ending its edit, which left the
/// "Edit text" gesture open for the next click to cancel, and its pastes joined the typing's undo
/// step. They end the edit first.
fn command_keeps_text_edit(id: &str, params: &Value) -> bool {
    if id != BATCH {
        return policy::keeps_text_edit(id) && !ENDS_TEXT_EDIT_IN_THE_ENGINE.contains(&id);
    }
    let steps = params.get("commands").and_then(Value::as_array);
    steps.is_some_and(|steps| {
        steps.iter().all(|step| {
            let id = step
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            command_keeps_text_edit(id, step.get("params").unwrap_or(&Value::Null))
        })
    })
}

fn get(board: &BoardDoc) -> Value {
    match summaries(board) {
        Ok(elements) => json!({"ok": true, "elements": elements}),
        Err(error) => failure(error),
    }
}

/// The summaries of the layers of the page on show.
fn summaries(board: &BoardDoc) -> Result<Vec<Value>, BoardError> {
    let ds = board.session.ds()?;
    Ok(summary::summarize(&ds.doc, ds.page))
}

/// The top-level layers of the page on show, a connector's label among them.
fn page_layers(board: &BoardDoc) -> Result<Vec<NodeId>, BoardError> {
    let ds = board.session.ds()?;
    let page = ds.doc.node(ds.page);
    Ok(page.map(|page| page.children.clone()).unwrap_or_default())
}

/// An id as text: a string that isn't empty, or a number (the engine's node ids are numbers).
fn id_text(id: &Value) -> Option<String> {
    match id {
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

/// The layer `id` names, for an op: an agent's id, else a node id ([`skeleton::resolve`]), if it
/// is a layer of a page. The page is no element, though its node id is a number like the others'.
fn layer_named(doc: &Document, id: &str) -> Option<NodeId> {
    let layer = skeleton::resolve(doc, id)?;
    let is_page = doc.node(layer)?.is_page();
    (!is_page && doc.page_of(layer).is_some()).then_some(layer)
}

fn add(board: &mut BoardDoc, op: &Value) -> Value {
    let items = match skeletons(op) {
        Ok(items) => items,
        Err(error) => return failure(format!("add: {error}")),
    };
    match skeleton::apply(board, &items, (0.0, 0.0)) {
        Ok(applied) => added(&applied),
        Err(error) if needs_a_board(board, &error.to_string()) => {
            failure(format!("add: {error}. {ADD_ON_A_DESIGN_FILE}"))
        }
        Err(error) => failure(format!("add: {error}")),
    }
}

/// What `add`'s refusal goes on to say when an element is one only a board has and the canvas is a
/// design file: what it draws there, and the engine's commands that draw the rest (SMOKE-3).
const ADD_ON_A_DESIGN_FILE: &str = "On a design file add draws text, frames and images; draw \
     shapes and lines with the command op: shape.rectangle, shape.ellipse, shape.line or \
     shape.arrow";

/// What `mermaid` answers when the diagram's shapes or connectors can't be drawn because the canvas
/// is a design file, and what to do instead (SMOKE-3). A chart, which comes in as one picture, can.
const MERMAID_ON_A_DESIGN_FILE: &str = "mermaid: this canvas is a design file, and a diagram's \
     shapes and connectors are for boards: bring the diagram in as an SVG file with the command \
     op's file.import instead";

/// Whether `error` is the engine refusing a board's command because the canvas is a design file
/// ([`crate::doc::BOARDS_ONLY`]).
fn needs_a_board(board: &BoardDoc, error: &str) -> bool {
    board.kind() == DocKind::Design && error.contains(crate::doc::BOARDS_ONLY)
}

/// The elements of an `add`, each read as a [`Skeleton`].
fn skeletons(op: &Value) -> Result<Vec<Skeleton>, String> {
    let items = op.get("elements").and_then(Value::as_array).ok_or(
        r#"it needs the elements to draw, like {"elements": [{"type": "rectangle", "x": 0, "y": 0}]}"#,
    )?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Skeleton::deserialize(item).map_err(|error| format!("elements[{index}]: {error}"))
        })
        .collect()
}

/// `{ok, added, ids}` for what `apply` made, and the `notes` on what it skipped when it skipped
/// something.
fn added(applied: &Applied) -> Value {
    let mut reply = json!({"ok": true, "added": applied.ids.len(), "ids": applied.ids});
    if !applied.notes.is_empty() {
        reply["notes"] = json!(applied.notes);
    }
    reply
}

/// One element of an `update`: the layer it names, and what it changes.
struct Change {
    id: String,
    fields: Fields,
}

/// What an `update` element can change. What it leaves out stays as it is, and any other key is
/// ignored.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Fields {
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    text: Option<String>,
    label: Option<Label>,
    name: Option<String>,
    #[serde(rename = "backgroundColor")]
    background_color: Option<String>,
    #[serde(rename = "strokeColor")]
    stroke_color: Option<String>,
}

impl Change {
    fn read(item: &Value) -> Result<Self, String> {
        let id = item
            .get("id")
            .and_then(id_text)
            .ok_or("it needs the `id` of a layer")?;
        let fields = Fields::deserialize(item).map_err(|error| error.to_string())?;
        Ok(Self { id, fields })
    }

    /// The text to set: `text`, else the `label`'s, as for a shape in a skeleton.
    fn text(&self) -> Option<&str> {
        let label = self.fields.label.as_ref().map(|label| label.text.as_str());
        self.fields.text.as_deref().or(label)
    }
}

/// The elements of an `update`.
fn changes(op: &Value) -> Result<Vec<Change>, String> {
    let items = op
        .get("elements")
        .and_then(Value::as_array)
        .ok_or(r#"it needs the changes to make, like {"elements": [{"id": "a", "x": 10}]}"#)?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Change::read(item).map_err(|error| format!("elements[{index}]: {error}"))
        })
        .collect()
}

fn update(board: &mut BoardDoc, op: &Value) -> Value {
    let changes = match changes(op) {
        Ok(changes) => changes,
        Err(error) => return failure(format!("update: {error}")),
    };
    let (mut updated, mut missing, mut notes) = (0, Vec::new(), Vec::new());
    let result = crate::doc::grouped(&mut board.session, "Update elements", |session| {
        for (index, change) in changes.iter().enumerate() {
            let Some(layer) = layer_named(session.doc()?, &change.id) else {
                missing.push(change.id.clone());
                continue;
            };
            change_layer(session, layer, index, change, &mut notes).map_err(|error| {
                EngineError::Other(format!("elements[{index}] ({}): {error}", change.id))
            })?;
            updated += 1;
        }
        Ok(())
    });
    match result {
        Ok(()) => {
            let mut reply = json!({"ok": true, "updated": updated, "missing": missing});
            if !notes.is_empty() {
                reply["notes"] = json!(notes);
            }
            reply
        }
        Err(error) => failure(format!("update: {error}")),
    }
}

/// The numbers of `pairs` that are given, as params.
fn numbers<const N: usize>(pairs: [(&str, Option<f64>); N]) -> Map<String, Value> {
    pairs
        .into_iter()
        .filter_map(|(key, number)| Some((key.to_string(), json!(number?))))
        .collect()
}

/// Makes `change`, element `index` of an `update`, to `layer`: its size, then its place (so that
/// it ends with the top-left it was given), its text, its name (which wins over what the text
/// would name the layer) and its colours.
///
/// A place is in page coordinates, the ones `get` shows and `add` takes, whatever the layer is
/// inside of: `node.set`'s `x` and `y` are the parent's, and a layer put into a frame by its
/// `frameId` would land elsewhere.
fn change_layer(
    session: &mut Session,
    layer: NodeId,
    index: usize,
    change: &Change,
    notes: &mut Vec<String>,
) -> EngineResult<()> {
    let fields = &change.fields;
    let size = numbers([("width", fields.width), ("height", fields.height)]);
    if !size.is_empty() {
        session.execute("node.set", &json!({"ids": [layer], "props": size}))?;
    }
    let mut at = numbers([("x", fields.x), ("y", fields.y)]);
    if !at.is_empty() {
        at.insert("ids".into(), json!([layer]));
        session.execute("transform.moveTo", &Value::Object(at))?;
    }
    if let Some(text) = change.text()
        && !set_text(session, layer, text)?
    {
        let kind = summary::element_type(session.doc()?, layer);
        notes.push(format!(
            "elements[{index}]: `text` is ignored, because a layer of type {kind} has no text"
        ));
    }
    if let Some(name) = &fields.name {
        let props = json!({"name": name});
        session.execute("node.set", &json!({"ids": [layer], "props": props}))?;
    }
    if fields.background_color.is_some() || fields.stroke_color.is_some() {
        skeleton::set_colors(
            session,
            layer,
            fields.background_color.as_deref(),
            fields.stroke_color.as_deref(),
        )?;
    }
    Ok(())
}

/// Sets the text of `layer`: the label of a connector (an empty one removes it), the note of a
/// sticky, the text of a shape or of a text layer. `false` when the layer has no text to set.
fn set_text(session: &mut Session, layer: NodeId, text: &str) -> EngineResult<bool> {
    enum Place {
        Label,
        Layer(NodeId),
        Nowhere,
    }
    let place = {
        let doc = session.doc()?;
        match doc.node(layer).and_then(|node| node.board.as_ref()) {
            Some(BoardItem::Connector(_) | BoardItem::Label { .. }) => Place::Label,
            _ => summary::text_layer(doc, layer).map_or(Place::Nowhere, Place::Layer),
        }
    };
    match place {
        Place::Label => {
            session.execute("board.label", &json!({"id": layer, "text": text}))?;
        }
        Place::Layer(text_layer) => {
            let props = json!({"characters": text});
            session.execute("node.set", &json!({"ids": [text_layer], "props": props}))?;
        }
        Place::Nowhere => return Ok(false),
    }
    Ok(true)
}

fn delete(board: &mut BoardDoc, op: &Value) -> Value {
    let ids = match id_list(op) {
        Ok(ids) => ids,
        Err(error) => return failure(format!("delete: {error}")),
    };
    let doc = match board.doc() {
        Ok(doc) => doc,
        Err(error) => return failure(format!("delete: {error}")),
    };
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    for id in ids {
        match layer_named(&doc, &id) {
            Some(layer) if !found.contains(&layer) => found.push(layer),
            Some(_) => {}
            None => missing.push(id),
        }
    }
    // Without layers the engine's delete takes the selection, so it isn't asked.
    if !found.is_empty() {
        let selection = board.session.selection();
        if let Err(error) = board.execute("edit.delete", &json!({"ids": found})) {
            return failure(format!("delete: {error}"));
        }
        // The selection is the user's: what the deletion left of it stays selected.
        if !selection.is_empty() {
            board.session.set_selection(selection);
        }
    }
    json!({"ok": true, "deleted": found.len(), "missing": missing})
}

/// The `ids` of a `delete`, each an id as [`id_text`] reads it.
fn id_list(op: &Value) -> Result<Vec<String>, String> {
    let ids = op
        .get("ids")
        .and_then(Value::as_array)
        .ok_or(r#"it needs the ids to delete, like {"ids": ["a", "b"]}"#)?;
    ids.iter()
        .enumerate()
        .map(|(index, id)| id_text(id).ok_or_else(|| format!("ids[{index}] is not an id")))
        .collect()
}

fn clear(board: &mut BoardDoc) -> Value {
    let layers = match page_layers(board) {
        Ok(layers) => layers,
        Err(error) => return failure(format!("clear: {error}")),
    };
    // Without layers the engine's delete takes the selection, so it isn't asked.
    if layers.is_empty() {
        return json!({"ok": true});
    }
    let cleared = crate::doc::grouped(&mut board.session, "Clear canvas", |session| {
        session.execute("edit.delete", &json!({"ids": layers}))
    });
    match cleared {
        Ok(_) => json!({"ok": true}),
        Err(error) => failure(format!("clear: {error}")),
    }
}

fn replace(board: &mut BoardDoc, op: &Value) -> Value {
    let incoming = match incoming(op) {
        Ok(incoming) => incoming,
        Err(error) => return failure(error),
    };
    if let Err(error) = replace_pages(board, &incoming.import) {
        return failure(format!("replace: {error}"));
    }
    let mut reply = get(board);
    if !incoming.notes.is_empty() {
        reply["notes"] = json!(incoming.notes);
    }
    reply
}

/// What a `replace` brings in.
struct Incoming {
    /// The params of the `file.import` that loads it.
    import: Value,
    /// What the Excalidraw importer left out of a scene.
    notes: Vec<String>,
}

/// What the `replace` op `op` brings in: the scene it carries, else the file its `path` names
/// (read here when it is an Excalidraw one). Anything that can't be taken is the reply's error,
/// before the canvas is touched.
///
/// A canvas file is read whole and as it is found, so it is checked first as the canvas checks
/// its own file ([`sawit_core::files::check_regular`]): a named pipe blocked the read for ever, and
/// a link to a device such as `/dev/zero` read until the Mac ran out of memory, on the UI thread
/// (CANVAS-CORE-6). Only a regular file of at most [`crate::store::MAX_FILE`] bytes is taken. A
/// `.figcraft` file is read by the engine's `file.import` where it is; a `.sawit` file, or a copy
/// Sawit keeps of a canvas file, is read here and handed to it by its bytes ([`kept_copy`]).
fn incoming(op: &Value) -> Result<Incoming, String> {
    if let Some(scene) = op.get("scene").filter(|scene| !scene.is_null()) {
        return from_scene(scene).map_err(|error| format!("replace: {error}"));
    }
    let Some(path) = op.get("path").and_then(Value::as_str) else {
        return Err(
            r#"replace needs the scene or the file to load, like {"op": "replace", "path": "/path/to/board.sawit"}"#
                .into(),
        );
    };
    let extension = Path::new(path).extension().and_then(OsStr::to_str);
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some(figcraft_format::EXTENSION) => {
            sawit_core::files::check_regular(Path::new(path), crate::store::MAX_FILE)
                .map_err(|error| format!("replace: couldn't read {path}: {error}"))?;
            Ok(Incoming {
                import: json!({"path": path}),
                notes: Vec::new(),
            })
        }
        Some("excalidraw") => {
            let scene = excalidraw::read_file(Path::new(path))
                .map_err(|error| format!("replace: {error}"))?;
            from_scene(&scene).map_err(|error| format!("replace: {path}: {error}"))
        }
        _ if is_sawit_file(Path::new(path)) => kept_copy(path),
        _ => Err(format!(
            "replace: {path} is not a .sawit or .excalidraw file"
        )),
    }
}

/// Whether `path` names a Sawit canvas file: a `.sawit` file (or a `.figcraft` one, the
/// extension canvas files had before), or a copy Sawit keeps of one under another name, the
/// backups of the store (`<name>.sawit.bak` and `<name>.sawit.del.bak`, the older `.bak.<n>` and
/// `.del.bak.<n>`, and the `.lossy.bak` of a file opened with notes:
/// [`sawit_core::nyawit::is_backup_suffix`]). Only the name counts, in any case.
pub fn is_sawit_file(path: &Path) -> bool {
    use sawit_core::files::{CANVAS_EXTENSION, LEGACY_CANVAS_EXTENSION};
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy().to_lowercase();
    [CANVAS_EXTENSION, LEGACY_CANVAS_EXTENSION]
        .iter()
        .any(|extension| {
            let extension = format!(".{extension}");
            name.rfind(&extension).is_some_and(|at| {
                let kept = &name[at + extension.len()..];
                kept.is_empty() || sawit_core::nyawit::is_backup_suffix(kept)
            })
        })
}

/// The name the engine is given for the bytes of a file called `name` that it imports
/// (`file.import` with `data`). It tells a file's kind by its extension, and knows a canvas file
/// only by its format's own, `.figcraft` ([`figcraft_format::EXTENSION`]): a `.sawit` file is
/// handed over as `<stem>.figcraft`, and a copy Sawit keeps of a canvas file ([`is_sawit_file`]:
/// `canvas.sawit.bak`...) as `<name>.figcraft`, where the engine would place either as an image
/// (MCP-6). Any other name is as it is.
pub fn engine_name(name: &str) -> String {
    let file = Path::new(name);
    let extension = file.extension().and_then(OsStr::to_str);
    let is = |wanted: &str| extension.is_some_and(|ext| ext.eq_ignore_ascii_case(wanted));
    if is(sawit_core::files::CANVAS_EXTENSION) {
        let engine = file.with_extension(figcraft_format::EXTENSION);
        return engine.to_string_lossy().into_owned();
    }
    if is(figcraft_format::EXTENSION) || !is_sawit_file(file) {
        return name.to_string();
    }
    format!("{name}.{}", figcraft_format::EXTENSION)
}

/// A `.sawit` file, or a copy Sawit keeps of a canvas file under another name ([`is_sawit_file`]:
/// `canvas.sawit.bak`...), as what `file.import` loads: its bytes, named as the engine knows a
/// canvas file ([`engine_name`]). It is read as a canvas file is checked: only a regular file of
/// at most [`crate::store::MAX_FILE`] (CANVAS-CORE-6). A file that isn't there, or isn't one, is
/// the error.
fn kept_copy(path: &str) -> Result<Incoming, String> {
    let file = Path::new(path);
    let bytes = sawit_core::files::read_regular(file, crate::store::MAX_FILE)
        .map_err(|error| format!("replace: couldn't read {path}: {error}"))?;
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    Ok(Incoming {
        import: json!({"data": STANDARD.encode(bytes), "name": engine_name(&name)}),
        notes: Vec::new(),
    })
}

/// An Excalidraw `scene` as something the engine imports: it is drawn on a board of its own, and
/// that board is the canvas file that comes in, named as the engine knows one. So a scene replaces
/// the canvas the way a file does, as pages, and what it is drawn on is as the importer makes it
/// (a light page, which the scene's own colours are made for).
fn from_scene(scene: &Value) -> Result<Incoming, BoardError> {
    let (drawn, notes) = excalidraw::to_board("Canvas", scene)?;
    let file = drawn.to_bytes()?;
    Ok(Incoming {
        import: json!({"data": STANDARD.encode(file), "name": engine_name("scene.sawit")}),
        notes,
    })
}

/// Replaces the pages of the canvas with those that the `file.import` of `import` brings in (a
/// canvas file or its bytes), in the canvas's own session as one undo step: they are added to
/// the document, then the old ones are deleted. An import that fails, a file that can't be read
/// say, fails the step, and the canvas is as it was.
///
/// The canvas keeps its document, so it stays a board (a design file's frames come in as layers of
/// one), keeps its file and what is saved of it, and undo brings the old pages back.
fn replace_pages(board: &mut BoardDoc, import: &Value) -> Result<(), BoardError> {
    crate::doc::grouped(&mut board.session, "Replace canvas", |session| {
        let old = session.doc()?.pages.clone();
        session.execute("file.import", import)?;
        for page in old {
            session.execute("page.delete", &json!({"id": page}))?;
        }
        Ok(())
    })?;
    Ok(())
}

fn commands() -> Value {
    let commands: Vec<Value> = figcraft_engine::cmd::all_commands()
        .iter()
        .filter(|command| listed(command.id))
        .map(|command| json!({"id": command.id, "label": command.label, "params": command.params}))
        .collect();
    json!({"ok": true, "commands": commands})
}

/// Whether the `commands` op offers command `id`: the canvas doesn't hide it, and the `command`
/// op runs it to a result. That one rule leaves out what Sawit does itself ([`HANDLED_BY_SAWIT`],
/// the save and the like) and what only opens a document ([`OPENS_DOCUMENTS`]), so every command
/// listed is one an agent can call.
fn listed(id: &str) -> bool {
    !policy::is_hidden(id) && handled(id).is_none() && !OPENS_DOCUMENTS.contains(&id)
}

/// The command in [`HANDLED_BY_SAWIT`] that `id` is, if it is one.
fn handled(id: &str) -> Option<&'static str> {
    HANDLED_BY_SAWIT
        .iter()
        .find(|handled| **handled == id)
        .copied()
}

fn command(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(id) = op.get("command").and_then(Value::as_str) else {
        return failure(
            r#"a command op needs a command id, like {"op": "command", "command": "view.zoomIn"}"#,
        );
    };
    let params = match op.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(params) => params.clone(),
    };
    if let Some(refusal) = refused(id, &params) {
        return failure(refusal);
    }
    let params = match canvas_files_by_bytes(id, params) {
        Ok(params) => params,
        Err(error) => return failure(error),
    };
    let history = FileHistory::of(&board.session);
    let ran = if id == BATCH {
        batch(board, &params)
    } else {
        single(board, id, &params)
    };
    match ran {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => {
            if let Some(history) = history {
                history.restore(&mut board.session);
            }
            failure(error)
        }
    }
}

/// The version history and branches of the canvas's file, as a command op found them.
///
/// The engine keeps them outside undo, so neither its rollback of a failed command or group nor
/// [`Documents::restore`] puts them back: a refused `branch.create` has already added its branch,
/// with two copies of the document, before the guard closes the branch's tab. A command op that
/// fails puts them back, so the canvas's next save writes nothing of what the op tried.
struct FileHistory {
    /// The uid of the canvas's document, which holds them.
    uid: u64,
    versions: Vec<figcraft_format::Version>,
    branches: Vec<figcraft_format::Branch>,
}

impl FileHistory {
    /// Those of the document on show, the canvas's. `None` without one.
    fn of(session: &Session) -> Option<Self> {
        let ds = session.ds().ok()?;
        Some(Self {
            uid: ds.uid,
            versions: ds.versions.clone(),
            branches: ds.branches.clone(),
        })
    }

    /// Puts them back on the document they were read from.
    fn restore(self, session: &mut Session) {
        if let Some(ds) = session.docs.iter_mut().find(|ds| ds.uid == self.uid) {
            ds.versions = self.versions;
            ds.branches = self.branches;
        }
    }
}

/// Why a command op has no result.
#[derive(Debug, Error)]
enum Failed {
    /// The engine refused the command, or it failed.
    #[error(transparent)]
    Engine(#[from] BoardError),
    /// The command would have left the canvas with another document open or on show; the canvas is
    /// as it was.
    #[error("{0} would open or switch documents; the canvas keeps its one document")]
    Switched(String),
}

/// Why the `command` op won't run `id` with `params`, if it won't. `id` itself, or a step of a
/// `command.batch`, however deeply batches are nested, is one that Sawit handles
/// ([`HANDLED_BY_SAWIT`]), or that would write over a canvas file ([`WRITES_TO_PATH`]), or read a
/// file that never ends ([`READS_FROM_PATH`]), or a step that writes where its params say and
/// takes any of them from another step's result ([`refers_to_a_step`]), or reads where they say
/// and takes that from one: what that names isn't known until the batch runs (CANVAS-CORE-7).
fn refused(id: &str, params: &Value) -> Option<Refusal> {
    refused_in(id, params, false)
}

/// [`refused`] for `id`, a step of a `command.batch` when `in_batch` says so. Such a step's params
/// can be taken from another step's result as the batch runs: a writer's are then no text to
/// check, and the step is refused. A command's own params, outside a batch, are never resolved.
/// Nor are a nested batch's: the engine passes them on as written, and the nested batch puts its
/// own steps' results in place as they run, so its steps are checked as the steps of a batch.
fn refused_in(id: &str, params: &Value, in_batch: bool) -> Option<Refusal> {
    if let Some(handled) = handled(id) {
        return Some(Refusal::Handled(handled));
    }
    if id == BATCH {
        let steps = params.get("commands")?.as_array()?;
        return steps.iter().find_map(|step| {
            let id = step.get("command")?.as_str()?;
            refused_in(id, step.get("params").unwrap_or(&Value::Null), true)
        });
    }
    if let Some((_, names)) = READS_FROM_PATH.iter().find(|(reader, _)| *reader == id) {
        return unreadable(id, names, params, in_batch);
    }
    if !WRITES_TO_PATH.contains(&id) {
        return None;
    }
    if in_batch && refers_to_a_step(params) {
        return Some(Refusal::Unchecked(id.to_string()));
    }
    let path = params.get("path")?.as_str()?;
    is_canvas_file(Path::new(path)).then(|| Refusal::Overwrites(id.to_string()))
}

/// Why `id`, a command that reads the file one of its params `names` names ([`READS_FROM_PATH`]),
/// won't run with `params`, if it won't (as a step of a batch when `in_batch` says so):
/// - in a batch, the params, or one of `names` in them, are taken from another step's result:
///   what the step would read isn't known before the batch runs. Its other params may be taken
///   so: an image placed from its bytes, in a frame that an earlier step made, reads no file;
/// - a file named is no regular file (a named pipe, a device, a folder), or holds more than
///   [`crate::store::MAX_FILE`] bytes ([`sawit_core::files::check_regular`], CANVAS-CORE-6).
///
/// A file that isn't there, or can't be opened, is the engine's to report, as it always was.
fn unreadable(id: &str, names: &[&str], params: &Value, in_batch: bool) -> Option<Refusal> {
    let named_by_a_step = is_a_reference(params)
        || (names.iter()).any(|name| params.get(name).is_some_and(refers_to_a_step));
    if in_batch && named_by_a_step {
        return Some(Refusal::UncheckedRead(id.to_string()));
    }
    names.iter().find_map(|name| {
        let path = params.get(name)?.as_str()?;
        // Local variables JSON given as its text, not as a file's name.
        if *name == "variables" && path.trim_start().starts_with('{') {
            return None;
        }
        let error =
            sawit_core::files::check_regular(Path::new(path), crate::store::MAX_FILE).err()?;
        let refused = matches!(
            error.kind(),
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::FileTooLarge
        );
        refused.then(|| Refusal::Unreadable {
            command: id.to_string(),
            path: path.to_string(),
            error: error.to_string(),
        })
    })
}

/// Whether `value` is a reference to another step's result, which FigCraft's `command.batch` puts
/// in its place as it runs the step: `{"$step": n}` or `{"$ref": name}`, each with an optional
/// `field`. Any object with either key is one.
fn is_a_reference(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.contains_key("$step") || map.contains_key("$ref"))
}

/// Whether `params`, or anything in them, is a reference to another step's result
/// ([`is_a_reference`]).
fn refers_to_a_step(params: &Value) -> bool {
    match params {
        Value::Object(map) => is_a_reference(params) || map.values().any(refers_to_a_step),
        Value::Array(items) => items.iter().any(refers_to_a_step),
        _ => false,
    }
}

/// Why a command op is refused before anything runs ([`refused`]).
#[derive(Debug, Error, PartialEq)]
enum Refusal {
    /// The command is one Sawit carries out itself.
    #[error("{0} is handled by Sawit")]
    Handled(&'static str),
    /// The command, by its id, would write over a canvas file.
    #[error("{0} would overwrite a canvas file")]
    Overwrites(String),
    /// A step of a batch that writes where its params say takes them from another step's result,
    /// so whether it writes a canvas file isn't known before the batch runs.
    #[error(
        "{0} in a command.batch needs its params written out: taken from another step's result, \
         what it writes can't be checked for a canvas file before the batch runs"
    )]
    Unchecked(String),
    /// A step of a batch that reads the file its params name takes that name from another step's
    /// result, so whether it is a file that can be read isn't known before the batch runs.
    #[error(
        "{0} in a command.batch needs its params written out: taken from another step's result, \
         the file it reads can't be checked before the batch runs"
    )]
    UncheckedRead(String),
    /// The command, by its id, would read the file at `path`, which is no regular file or is
    /// past the most that is read from one file (`error`).
    #[error("{command} can't read {path}: {error}")]
    Unreadable {
        command: String,
        path: String,
        error: String,
    },
}

/// The documents a session holds, as a command op found them: their uids in order, and the uid of
/// the one on show.
///
/// A uid names a document for good, because the engine gives every document it makes a new one. So
/// closing the canvas's document and opening another in its place, which leaves the count and the
/// index on show as they were, still reads as the change it is.
#[derive(Debug, PartialEq)]
struct Documents {
    uids: Vec<u64>,
    shown: Option<u64>,
}

impl Documents {
    fn of(session: &Session) -> Self {
        Self {
            uids: session.docs.iter().map(|doc| doc.uid).collect(),
            shown: session.ds().ok().map(|doc| doc.uid),
        }
    }

    /// Closes the documents opened since, and shows the one that was on show again.
    fn restore(&self, session: &mut Session) {
        session.docs.retain(|doc| self.uids.contains(&doc.uid));
        session.active = self
            .shown
            .and_then(|uid| session.docs.iter().position(|doc| doc.uid == uid))
            .unwrap_or(0);
    }
}

/// `params` of command `id`, with the Sawit canvas file that a `file.import` names by its `path`
/// (a `.sawit` file, or a copy Sawit keeps of a canvas file: [`is_sawit_file`]) handed to the
/// engine by its bytes instead, named as the engine knows a canvas file ([`engine_name`]): it
/// tells a file's kind by its extension, and would place one as an image. The steps of a
/// `command.batch` are read the same way, a nested one's too; a path that another step's result
/// gives is the engine's. A file that can't be read is the error, and nothing runs.
fn canvas_files_by_bytes(id: &str, mut params: Value) -> Result<Value, String> {
    if id == BATCH {
        let steps = params.get_mut("commands").and_then(Value::as_array_mut);
        for step in steps.into_iter().flatten() {
            let Some(id) = step
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            if let Some(params) = step.get_mut("params") {
                *params = canvas_files_by_bytes(&id, params.take())?;
            }
        }
        return Ok(params);
    }
    let path = params.get("path").and_then(Value::as_str);
    let Some(path) = path.filter(|path| id == "file.import" && engine_name(path) != *path) else {
        return Ok(params);
    };
    let file = Path::new(path);
    let bytes = sawit_core::files::read_regular(file, crate::store::MAX_FILE)
        .map_err(|error| format!("file.import: couldn't read {path}: {error}"))?;
    let name = engine_name(&file.file_name().unwrap_or_default().to_string_lossy());
    if let Some(params) = params.as_object_mut() {
        params.remove("path");
        params.insert("data".into(), json!(STANDARD.encode(bytes)));
        params.insert("name".into(), json!(name));
    }
    Ok(params)
}

/// Runs command `id`, other than a batch. If it left other documents open, or another on show,
/// those are closed again and the canvas's document is shown: nothing but `file.close` removes the
/// canvas's own document outside a batch, and Sawit refuses that one.
///
/// Not run in a group of its own: `edit.undo` and `edit.redo` aren't undo steps, and a group would
/// record them as new ones and drop what can be redone.
fn single(board: &mut BoardDoc, id: &str, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let result = board.execute(id, params);
    if Documents::of(&board.session) != before {
        before.restore(&mut board.session);
        return Err(Failed::Switched(id.into()));
    }
    Ok(result?)
}

/// Runs a `command.batch` in a group that fails when its steps left other documents open, or
/// another on show: the engine then puts back the documents, the one on show and the canvas's own
/// document, so nothing of the batch stays.
///
/// The group takes the label the batch's own group does (`label`, else "Batch"), so a batch that
/// stays is the one undo step it always was.
fn batch(board: &mut BoardDoc, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let label = figcraft_engine::params::string(params, "label").unwrap_or_else(|| "Batch".into());
    let mut switched = false;
    let result = crate::doc::grouped(&mut board.session, &label, |session| {
        let result = session.execute(BATCH, params)?;
        if Documents::of(session) != before {
            switched = true;
            return Err(EngineError::Disabled(
                BATCH.into(),
                "it would open or switch documents".into(),
            ));
        }
        Ok(result)
    });
    if switched {
        return Err(Failed::Switched(BATCH.into()));
    }
    Ok(result.map_err(BoardError::from)?)
}

/// The kinds of the `pointer` op, as an error lists them.
const POINTER_KINDS: &str = "down, move, up, double or hover";

fn pointer(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(name) = op.get("kind").and_then(Value::as_str) else {
        return failure(format!("a pointer op needs a kind: {POINTER_KINDS}"));
    };
    let kind = match name {
        "down" => PointerKind::Down,
        "move" => PointerKind::Move,
        "up" => PointerKind::Up,
        "double" => PointerKind::DoubleClick,
        "hover" => PointerKind::Hover,
        other => return failure(format!("{other:?} is no pointer kind: use {POINTER_KINDS}")),
    };
    let number = |key: &str| op.get(key).and_then(Value::as_f64);
    let (Some(x), Some(y)) = (number("x"), number("y")) else {
        return failure("a pointer op needs x and y, a point on the page");
    };
    let held = |key: &str| op.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mods = Mods {
        shift: held("shift"),
        alt: held("alt"),
        cmd: held("cmd"),
        ctrl: false,
    };
    let event = match board.session.ds() {
        Ok(ds) => input::page_pointer_event(kind, (x, y), &ds.view, mods),
        Err(error) => return failure(error),
    };
    let requests = match input::pointer(board, &event) {
        Ok(requests) => requests,
        Err(error) => return failure(error),
    };
    leave_for_the_view(board, &requests);
    let selection: Vec<String> = (board.session.selection().iter())
        .map(ToString::to_string)
        .collect();
    json!({"ok": true, "selection": selection, "tool": board.session.tool.tool.id()})
}

fn key(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(key) = op
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
    else {
        return failure(r#"a key op needs a key, like {"op": "key", "key": "Escape"}"#);
    };
    let held = |modifier: &str| op.get(modifier).and_then(Value::as_bool).unwrap_or(false);
    let mods = Mods {
        shift: held("shift"),
        alt: held("alt"),
        cmd: held("cmd"),
        ctrl: held("ctrl"),
    };
    let outcome = match input::route_default_key(board, key, mods) {
        Ok(outcome) => outcome,
        Err(error) => return failure(error),
    };
    let requests = std::mem::take(&mut board.session.ui_requests);
    leave_for_the_view(board, &requests);
    json!({"ok": true, "handled": outcome != KeyOutcome::Unhandled})
}

fn type_text(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(text) = op.get("text").and_then(Value::as_str) else {
        return failure(r#"a type op needs the text to type, like {"op": "type", "text": "hi"}"#);
    };
    if !board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()) {
        return failure("not editing text");
    }
    let session = &mut board.session;
    match guard::run("ui.type", || textedit::insert(session, text)) {
        Ok(()) => json!({"ok": true}),
        Err(error) => failure(error),
    }
}

/// Carries out what an op's input asked of the view that needs none
/// ([`input::handle_requests`]), and leaves the rest in `session.ui_requests`, ahead of any made
/// since, as the same input from the window leaves them for the view.
fn leave_for_the_view(board: &mut BoardDoc, requests: &[UiRequest]) {
    let mut rest = input::handle_requests(board, requests);
    rest.append(&mut board.session.ui_requests);
    board.session.ui_requests = rest;
}

fn mermaid(board: &mut BoardDoc, op: &Value) -> Value {
    let source = op.get("source").and_then(Value::as_str);
    let Some(source) = source.filter(|source| !source.trim().is_empty()) else {
        return failure("mermaid: `source` must be a non-empty string");
    };
    match draw_mermaid(board, source, op) {
        Ok(applied) => {
            let mut reply = json!({"ok": true, "added": applied.ids.len(), "ids": applied.ids});
            if !applied.notes.is_empty() {
                reply["notes"] = json!(applied.notes);
            }
            reply
        }
        Err(error) if needs_a_board(board, &error) => failure(MERMAID_ON_A_DESIGN_FILE),
        Err(error) => failure(format!("mermaid: {error}")),
    }
}

/// Renders `source`, makes board shapes of it and draws them where `op` says.
///
/// Rendering blocks, and the app answers ops on its UI thread, because the canvas answers
/// synchronously: well under a millisecond for a typical diagram, about 60 ms the first time,
/// while Pele loads, and at most 5 s, when `sawit_core::mermaid` stops a runaway diagram, which
/// isn't tried a second time ([`second_try`]).
fn draw_mermaid(board: &mut BoardDoc, source: &str, op: &Value) -> Result<Applied, String> {
    let palette = Palette::default();
    let diagram = match render(source, &palette) {
        Ok(diagram) => diagram,
        // A diagram that fails both tries is reported in the words of the first.
        Err(first) => match second_try(source, &first) {
            Some(again) => render(&again, &palette).map_err(|_| first)?,
            None => return Err(first),
        },
    };
    let items = crate::mermaid::to_skeletons(&diagram)?;
    let (left, top, _, _) = skeleton::bounds(&items).ok_or("the diagram is empty")?;
    let (x, y) = diagram_corner(board, op).map_err(|error| error.to_string())?;
    skeleton::apply(board, &items, (x - left, y - top)).map_err(|error| error.to_string())
}

/// The Mermaid `source` to try again, after its first render failed with `error`: double quotes
/// trip up some diagram types, so a source that has them is tried once more with single ones, as
/// Excalidraw's own Mermaid import does. Not a diagram stopped for taking too long, which the
/// second try took as long again: the window froze for twice the render's time limit, 10 s
/// (CANVAS-CORE-10).
fn second_try(source: &str, error: &str) -> Option<String> {
    let again = source.contains('"') && !sawit_core::mermaid::took_too_long(error);
    again.then(|| source.replace('"', "'"))
}

/// Where the `mermaid` op puts a diagram's top left corner: at the op's `x` and `y`. What it leaves
/// out is [`MERMAID_GAP`] right of what the page on show holds and level with its top, or 0 on an
/// empty page.
fn diagram_corner(board: &BoardDoc, op: &Value) -> Result<(f64, f64), BoardError> {
    let page = board.session.page()?;
    let held = board.doc()?.page_content_bounds(page);
    let (x, y) = held.map_or((0.0, 0.0), |held| (held.x1 + MERMAID_GAP, held.y0));
    let given = |key| op.get(key).and_then(Value::as_f64);
    Ok((given("x").unwrap_or(x), given("y").unwrap_or(y)))
}

/// The scale the `export_svg` and `export_png` ops export at. They take no parameters (spec,
/// "Agent wire contract"), so it is 1×.
const EXPORT_OP_SCALE: f64 = 1.0;

/// The `export_svg` op: the page on show as one SVG file, answered as text.
fn export_svg(board: &mut BoardDoc) -> Value {
    match export_page(board, "svg", EXPORT_OP_SCALE).and_then(svg_text) {
        Ok(svg) => json!({"ok": true, "svg": svg}),
        Err(error) => failure(error),
    }
}

/// The `export_png` op: the page on show as one PNG file, answered as base64.
fn export_png(board: &mut BoardDoc) -> Value {
    match export_page(board, "png", EXPORT_OP_SCALE) {
        Ok(png) => json!({"ok": true, "png_base64": STANDARD.encode(png)}),
        Err(error) => failure(error),
    }
}

// The document's side of the commands Sawit carries out itself (spec, "Menu policy": UI-handled
// ids): the files File ▸ Export and Import from file… make and read, and what Edit ▸ Copy as
// copies. The canvas's view (crates/app/src/board/commands.rs) asks for files and writes them, and
// puts what is copied on the system clipboard.

/// The longest side, in pixels, of the image Copy as PNG puts on the clipboard
/// ([`copy_png_scale`]).
pub const COPY_PNG_MAX_SIDE: f64 = 4096.0;
/// The scale Copy as PNG wants, as FigCraft's own app copies at, before [`copy_png_scale`] caps it.
const COPY_PNG_SCALE: f64 = 2.0;
/// The least scale `file.export` takes: it raises any less to it, which would make a PNG longer
/// than [`COPY_PNG_MAX_SIDE`] (ruling P6-C14).
const LEAST_EXPORT_SCALE: f64 = 0.01;

/// What Edit ▸ Copy as puts on the system clipboard ([`copy_as`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Copied {
    /// The layers' text, CSS or SVG.
    Text(String),
    /// A PNG file.
    Png(Vec<u8>),
}

/// The page on show as one `format` file at `scale`: its top-level layers exported together,
/// whatever is selected, as [`export_bytes`] exports layers (`format` is the file's extension:
/// `png`, `jpg`, `svg`, `pdf`, `webp` or `xml`). It is what File ▸ Export makes with nothing
/// selected.
///
/// Nothing in the document changes, its selection included. A page with nothing on it has nothing
/// to export: that is the error, as is a format that is none of those.
pub fn export_page(board: &mut BoardDoc, format: &str, scale: f64) -> Result<Vec<u8>, BoardError> {
    let layers = page_layers(board)?;
    let row = ExportRow {
        format: format.to_string(),
        scale,
    };
    export_bytes(board, &layers, &row)
}

/// The frames of the page on show as one PDF, a page for each visible top-level frame in reading
/// order: `file.exportFramesPdf`, which answers with the file as base64. Nothing in the document
/// changes. A page without a visible frame is the engine's refusal, which says so ("the page has
/// no visible frames").
pub fn frames_pdf(board: &mut BoardDoc) -> Result<Vec<u8>, BoardError> {
    exported_file(&board.execute("file.exportFramesPdf", &json!({}))?)
}

/// The scale Copy as PNG exports layers at whose export area is `bounds` (its width and height on
/// the page): `wanted`, or less, for an area that would be more than [`COPY_PNG_MAX_SIDE`] pixels
/// long at it, so that its longest side is that long.
pub fn copy_png_scale(bounds: (f64, f64), wanted: f64) -> f64 {
    let longest = bounds.0.max(bounds.1);
    if longest * wanted > COPY_PNG_MAX_SIDE {
        COPY_PNG_MAX_SIDE / longest
    } else {
        wanted
    }
}

/// What Edit ▸ Copy as `id` copies: `edit.copyAsText`, `edit.copyAsCss`, `edit.copyAsSvg` or
/// `edit.copyAsPng`, as FigCraft's own app copies it.
///
/// - Text: the text of each selected layer, a line each: a text layer's, a sticky's or a shape's,
///   a connector's label's, or the text layers' inside a frame or a group.
/// - CSS: the first selected layer's, as Dev Mode writes it.
/// - SVG: the selected layers together, as one SVG file.
/// - PNG: the selected layers together, as one PNG at 2×, or at the scale that makes its longest
///   side [`COPY_PNG_MAX_SIDE`] pixels long when 2× would make it longer ([`copy_png_scale`]). An
///   area too long even at the least scale the engine exports at is refused.
///
/// With nothing selected, SVG and PNG copy the page's top-level layers, as File ▸ Export exports
/// them; text and CSS have nothing to copy. A selection without text has no text to copy. That is
/// the error, as is any other `id`. Nothing in the document changes.
pub fn copy_as(board: &mut BoardDoc, id: &str) -> Result<Copied, BoardError> {
    let selection = board.session.selection();
    match id {
        "edit.copyAsText" => text_of(board, &selection).map(Copied::Text),
        "edit.copyAsCss" => css_of(board, &selection).map(Copied::Text),
        "edit.copyAsSvg" | "edit.copyAsPng" => {
            let (ids, what) = if selection.is_empty() {
                (page_layers(board)?, "page")
            } else {
                (selection, "selection")
            };
            if id == "edit.copyAsSvg" {
                svg_of(board, &ids).map(Copied::Text)
            } else {
                png_of(board, &ids, what).map(Copied::Png)
            }
        }
        _ => Err(BoardError::Engine(format!("`{id}` is no Copy as command"))),
    }
}

/// The error of a Copy as that needs a selection, with nothing selected: it doesn't apply then,
/// as the engine's commands on the selection don't ([`BoardError::Unavailable`]).
fn nothing_selected() -> BoardError {
    BoardError::Unavailable {
        command: "edit.copyAs".to_string(),
        reason: "nothing is selected".to_string(),
    }
}

/// The text of layers `ids`, a line each, for Copy as text ([`copy_as`]). Each text is copied
/// once, however many of the layers hold it, and an empty one not at all.
fn text_of(board: &BoardDoc, ids: &[NodeId]) -> Result<String, BoardError> {
    if ids.is_empty() {
        return Err(nothing_selected());
    }
    let doc = board.doc()?;
    let is_text = |layer: &NodeId| {
        doc.node(*layer)
            .is_some_and(|node| node.kind.text().is_some())
    };
    let mut texts: Vec<NodeId> = Vec::new();
    for &id in ids {
        let held = match summary::text_layer(&doc, id) {
            Some(text) => vec![text],
            None => doc.subtree(id).into_iter().filter(is_text).collect(),
        };
        for text in held {
            if !texts.contains(&text) {
                texts.push(text);
            }
        }
    }
    let lines: Vec<&str> = (texts.iter())
        .filter_map(|text| Some(doc.node(*text)?.kind.text()?.characters.as_str()))
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(BoardError::Engine("the selection has no text".to_string()));
    }
    Ok(lines.join("\n"))
}

/// The CSS of the first of layers `ids`, as Dev Mode writes it, for Copy as CSS ([`copy_as`]).
fn css_of(board: &BoardDoc, ids: &[NodeId]) -> Result<String, BoardError> {
    let first = *ids.first().ok_or_else(nothing_selected)?;
    let doc = board.doc()?;
    Ok(guard::run("ui.copyAsCss", || {
        Ok(codegen::code_for(&doc, first, CodeLang::Css))
    })?)
}

/// Layers `ids` together as one SVG file, for Copy as SVG ([`copy_as`]).
fn svg_of(board: &mut BoardDoc, ids: &[NodeId]) -> Result<String, BoardError> {
    let row = ExportRow {
        format: "svg".to_string(),
        scale: 1.0,
    };
    svg_text(export_bytes(board, ids, &row)?)
}

/// The SVG file `bytes` as text.
fn svg_text(bytes: Vec<u8>) -> Result<String, BoardError> {
    String::from_utf8(bytes)
        .map_err(|_| BoardError::Engine("the SVG export isn't text".to_string()))
}

/// Layers `ids` together as one PNG, for Copy as PNG ([`copy_as`]): at 2×, or smaller, so that its
/// longest side is at most [`COPY_PNG_MAX_SIDE`] pixels. `what` they are (the selection, the page)
/// names them when they are too long for that.
fn png_of(board: &mut BoardDoc, ids: &[NodeId], what: &str) -> Result<Vec<u8>, BoardError> {
    let doc = board.doc()?;
    let setting = ExportSetting::default();
    let area = guard::run("ui.copyAsPng", || Ok(export_area(&doc, ids, &setting)))?;
    // Without an area the layers paint nothing, and the export says so.
    let scale = area.map_or(COPY_PNG_SCALE, |area| {
        copy_png_scale((area.width(), area.height()), COPY_PNG_SCALE)
    });
    if scale < LEAST_EXPORT_SCALE {
        return Err(BoardError::Engine(format!(
            "The {what} is too large to copy as PNG"
        )));
    }
    let row = ExportRow {
        format: "png".to_string(),
        scale,
    };
    export_bytes(board, ids, &row)
}

/// Imports the file at `path` into the page on show, as File ▸ Import from file… does
/// (`file.import`), or with `place` as Place image… does (`file.placeImage`), which takes only
/// images. The layer it made, which is then selected.
///
/// The import lands in the middle of the view, as a paste does ([`clipboard::paste`]): an image or
/// an SVG file is centred there. A design file (`.sawit`, `.figcraft`, `.fig`, `.sketch`,
/// `.penpot`) adds its pages after the canvas's own instead, and makes no layer on the page on
/// show. One undo step. A file that can't be read, or that the engine can't import, is the error,
/// which doesn't name the file, and nothing changes.
pub fn import_file(
    board: &mut BoardDoc,
    path: &Path,
    place: bool,
) -> Result<Vec<NodeId>, BoardError> {
    // A regular file only: a pipe or a device would never be read to its end (CANVAS-CORE-6).
    let bytes = sawit_core::files::read_regular(path, crate::store::MAX_FILE)
        .map_err(|error| BoardError::Io(error.to_string()))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let svg =
        (Path::new(name.as_ref()).extension()).is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));
    let mut params = if svg && !place {
        let view = &board.session.ds()?.view;
        json!({"x": view.cx, "y": view.cy, "center": true})
    } else {
        clipboard::centred(board, &bytes)?
    };
    params["data"] = json!(STANDARD.encode(&bytes));
    let command = if place {
        // The layer's name, which an import takes from the file's.
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        params["name"] = json!(stem);
        "file.placeImage"
    } else {
        // Its extension says what the file is, a canvas file's as the engine knows it.
        params["name"] = json!(engine_name(&name));
        "file.import"
    };
    let made = board.execute(command, &params)?;
    Ok(id_of(&made["id"]).into_iter().collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use figcraft_color::{Color, Paint};
    use figcraft_doc::{BoardItem, Document, NodeId};
    use figcraft_engine::textedit;
    use serde_json::json;

    use super::*;
    use crate::doc::DocKind;
    use crate::policy;

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// The elements `get` lists, after checking that it answered.
    fn elements(board: &mut BoardDoc) -> Vec<Value> {
        let reply = run(board, &json!({"op": "get"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        reply["elements"].as_array().unwrap().clone()
    }

    /// Sends command op `id` with `params`.
    fn send(board: &mut BoardDoc, id: &str, params: Value) -> Value {
        run(
            board,
            &json!({"op": "command", "command": id, "params": params}),
        )
    }

    /// The uids of the session's documents, in order, and the uid of the one on show.
    fn documents(board: &BoardDoc) -> (Vec<u64>, u64) {
        let session = &board.session;
        (
            session.docs.iter().map(|doc| doc.uid).collect(),
            session.ds().unwrap().uid,
        )
    }

    /// A second board in `dir`, the kind of file `file.recover` opens, named as the engine knows a
    /// canvas file (`.figcraft`), for the engine's commands that read it where it is. It has two
    /// stickies, so it can't be taken for a canvas that has one.
    fn other_board_file(dir: &std::path::Path) -> String {
        let mut other = BoardDoc::new_board("Other").unwrap();
        for x in [40, 400] {
            other
                .execute("board.sticky", &json!({"x": x, "y": 40, "text": "other"}))
                .unwrap();
        }
        let path = dir.join("other.figcraft");
        std::fs::write(&path, other.to_bytes().unwrap()).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// The answer to a command that would have left the canvas with another document.
    fn would_switch(id: &str) -> Value {
        json!({
            "ok": false,
            "error": format!(
                "{id} would open or switch documents; the canvas keeps its one document"
            ),
        })
    }

    /// The answer to a command that Sawit handles itself.
    fn handled_by_sawit(id: &str) -> Value {
        json!({"ok": false, "error": format!("{id} is handled by Sawit")})
    }

    #[test]
    fn get_lists_top_level_layers() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "one"}))
            .unwrap();
        board
            .execute("board.sticky", &json!({"x": 300, "y": 200, "text": "two"}))
            .unwrap();
        // Each sticky has a text layer of its own inside it, which `get` doesn't list.
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let top_level = doc.node(page).unwrap().children.clone();
        assert_eq!(top_level.len(), 2);
        assert!(
            top_level
                .iter()
                .all(|id| !doc.node(*id).unwrap().children.is_empty())
        );

        let elements = elements(&mut board);
        assert_eq!(elements.len(), 2, "{elements:?}");
        for element in &elements {
            let id = element["id"].as_str().expect("an id string");
            assert!(id.parse::<u64>().is_ok(), "{id} is not a number");
            assert_eq!(element["type"], json!("sticky"));
        }
        let ids: Vec<String> = top_level.iter().map(ToString::to_string).collect();
        assert_eq!(elements[0]["id"], json!(ids[0]));
        assert_eq!(elements[1]["id"], json!(ids[1]));
    }

    #[test]
    fn get_gives_each_layer_its_name_and_page_bounds() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 300, "y": 200, "text": "two"}))
            .unwrap();
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let id = doc.node(page).unwrap().children[0];
        let bounds = doc.world_bounds(id).unwrap();

        let elements = elements(&mut board);
        let element = &elements[0];
        assert_eq!(element["id"], json!(id.to_string()));
        assert_eq!(element["name"], json!(doc.node(id).unwrap().name));
        assert_eq!(element["x"], json!(bounds.x0));
        assert_eq!(element["y"], json!(bounds.y0));
        assert_eq!(element["width"], json!(bounds.width()));
        assert_eq!(element["height"], json!(bounds.height()));
        // Where the sticky was put, and a size it has.
        assert!((bounds.x0 - 300.0).abs() < 1e-6 && (bounds.y0 - 200.0).abs() < 1e-6);
        assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
    }

    #[test]
    fn get_lists_only_the_page_on_show() {
        let mut board = board();
        assert!(elements(&mut board).is_empty());
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        assert_eq!(elements(&mut board).len(), 1);
        // A new page is empty and on show; the sticky is on the first page.
        board.execute("page.add", &json!({})).unwrap();
        assert!(elements(&mut board).is_empty());
        board.execute("page.select", &json!({"index": 0})).unwrap();
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn command_runs_engine_commands() {
        let mut board = board();
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(!reply["result"]["id"].is_null(), "{reply}");
        // It ran on the board.
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn command_params_are_optional() {
        let mut board = board();
        let zoom = |board: &BoardDoc| board.session.ds().unwrap().view.zoom;
        let before = zoom(&board);
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "view.zoomIn"}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let once = zoom(&board);
        assert!(once > before);
        // `null` is no params either.
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "view.zoomIn", "params": null}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(zoom(&board) > once);
    }

    #[test]
    fn command_errors_are_reported() {
        let mut board = board();
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "no.such.command"}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("no.such.command"), "{error}");
        assert!(reply.get("result").is_none());

        // The engine refusing a command's params is reported too, with its own words, and leaves
        // the board alone.
        let before = board.doc().unwrap();
        for params in [
            json!({"x": 0, "y": 0, "color": "not a colour"}),
            json!({"x": 0, "y": 0, "no_such_param": 1}),
        ] {
            let reply = run(
                &mut board,
                &json!({"op": "command", "command": "board.sticky", "params": params}),
            );
            assert_eq!(reply["ok"], json!(false), "{params}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("board.sticky"), "{error}");
        }
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert!(elements(&mut board).is_empty());
    }

    #[test]
    fn a_command_op_without_a_command_id_is_reported() {
        let mut board = board();
        for op in [
            json!({"op": "command"}),
            json!({"op": "command", "command": 5}),
            json!({"op": "command", "params": {"x": 0, "y": 0}}),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("command"), "{error}");
        }
        assert!(elements(&mut board).is_empty());
    }

    #[test]
    fn commands_that_switch_or_write_documents_are_refused() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        let before = board.doc().unwrap();
        let revision = board.revision();
        for id in [
            "file.new",
            "file.newBoard",
            "file.newFromTemplate",
            "file.open",
            "file.openRecent",
            "file.close",
            "file.duplicate",
            "file.save",
            "file.saveAs",
            "window.new",
            "doc.nextTab",
            "doc.previousTab",
        ] {
            let reply = run(&mut board, &json!({"op": "command", "command": id}));
            assert_eq!(
                reply,
                json!({"ok": false, "error": format!("{id} is handled by Sawit")})
            );
        }
        // None of them reached the engine: the board is the one it was.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.revision(), revision);
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn refused_commands_are_ones_the_policy_hides_or_hands_to_sawit() {
        for id in HANDLED_BY_SAWIT {
            assert!(
                policy::is_hidden(id) || policy::is_ui(id),
                "{id} is neither hidden nor Sawit's own in the command policy"
            );
        }
    }

    #[test]
    fn commands_that_sawit_handles_are_refused_as_batch_steps_too() {
        let mut board = board();
        let before = board.doc().unwrap();
        for id in HANDLED_BY_SAWIT {
            let reply = send(
                &mut board,
                "command.batch",
                json!({"commands": [
                    {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                    {"command": id},
                ]}),
            );
            assert_eq!(reply, handled_by_sawit(id));
        }
        // Nothing ran, not even the sticky that came before the refused step.
        assert_eq!(board.session.docs.len(), 1);
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));
        assert!(elements(&mut board).is_empty());
    }

    #[test]
    fn a_save_inside_nested_batches_is_refused_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("copy.sawit");
        let mut board = board();
        let before = board.doc().unwrap();

        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "command.batch", "params": {"commands": [
                    {"command": "file.saveAs", "params": {"path": target}},
                ]}},
            ]}),
        );

        assert_eq!(reply, handled_by_sawit("file.saveAs"));
        assert!(!target.exists(), "the save wrote {}", target.display());
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        // The canvas's document was never given a path, so file.revert has nothing to read.
        assert_eq!(board.session.ds().unwrap().path, None);
    }

    /// The answer to a command that would have written over a canvas file (ruling P7-X3).
    fn would_overwrite(id: &str) -> Value {
        json!({"ok": false, "error": format!("{id} would overwrite a canvas file")})
    }

    /// The engine's commands that write a file where their `path` says, as read in FigCraft's
    /// engine at the pinned revision: everything it writes goes through `file_access::write`.
    const WRITERS: [&str; 6] = [
        "file.export",
        "file.exportSvg",
        "file.exportFramesPdf",
        "variable.export",
        "component.playground",
        "codeConnect.export",
    ];

    /// A project's `.nyawit` folder in `dir`, holding a file of each name a canvas, or a copy of
    /// one, goes by, with its own path as its bytes. Their paths.
    fn canvas_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let folder = dir.join(".nyawit");
        std::fs::create_dir_all(&folder).unwrap();
        let staging = sawit_core::files::staging_file(&folder.join("canvas.sawit"), 0);
        let mut files: Vec<_> = [
            "canvas.sawit",
            "canvas.excalidraw",
            "canvas.sawit.bak",
            "canvas.sawit.del.bak",
            "canvas.sawit.lossy.bak",
            "canvas.sawit.tmp",
            "canvas.excalidraw.bak",
            "canvas.excalidraw.lossy.bak",
            // The canvas file by the name it had before, and a backup of it.
            "canvas.figcraft",
            "canvas.figcraft.bak",
            // A file of the user's own, and in capitals, which is the same file on a Mac.
            "Design.sawit",
            "OLD.EXCALIDRAW",
        ]
        .map(|name| folder.join(name))
        .into();
        files.push(staging);
        for file in &files {
            std::fs::write(file, file.to_string_lossy().as_bytes()).unwrap();
        }
        files
    }

    #[test]
    fn canvas_files_are_told_by_the_end_of_their_name() {
        use std::path::Path;
        for name in [
            "canvas.sawit",
            "/project/.nyawit/canvas.sawit",
            "relative/Board.Sawit",
            "OLD.EXCALIDRAW",
            "canvas.sawit.bak",
            "canvas.sawit.lossy.bak",
            "canvas.sawit.tmp",
            ".canvas.sawit.sawit-tmp",
            ".canvas.sawit.sawit-tmp-412-3",
            ".Design.Sawit.sawit-tmp-1-0",
            "canvas.sawit.bak.1",
            "canvas.sawit.bak.4",
            "Design.SAWIT.BAK.12",
            "canvas.sawit.del.bak",
            "canvas.sawit.del.bak.1",
            "home.Sawit.DEL.BAK.4",
            "canvas.excalidraw.bak",
            "canvas.excalidraw.lossy.bak",
            "canvas.excalidraw.tmp",
            // The extension canvas files had before, and the copies kept of one.
            "/project/.nyawit/canvas.figcraft",
            "relative/Board.FigCraft",
            "canvas.figcraft.bak",
            "canvas.figcraft.del.bak.1",
            ".canvas.figcraft.sawit-tmp-412-3",
        ] {
            assert!(is_canvas_file(Path::new(name)), "{name}");
        }
        // An export to an image or a text path is not one, whatever its name holds.
        for name in [
            "page.png",
            "page.svg",
            "canvas.sawit.svg",
            "canvas.figcraft.svg",
            "canvas.excalidraw.png",
            "sawit",
            "notes.sawitx",
            "canvas.bak",
            "canvas.tmp",
            "canvas.sawit.bak.",
            "canvas.sawit.bak.png",
            "canvas.sawit.bak.1.svg",
            "canvas.sawit.del.bak.",
            "canvas.sawit.del.bak.png",
            "canvas.del.bak",
            "/project/.sawit/page.png",
            "",
            "/",
            "..",
        ] {
            assert!(!is_canvas_file(Path::new(name)), "{name}");
        }
        // The files a save of the canvas is staged in, wherever it is.
        for n in [0, 1, u64::MAX] {
            let staging = sawit_core::files::staging_file(Path::new("/p/.nyawit/canvas.sawit"), n);
            assert!(is_canvas_file(&staging), "{}", staging.display());
        }
    }

    #[test]
    fn the_guarded_writers_are_the_audited_engine_commands() {
        assert_eq!(WRITES_TO_PATH, WRITERS);
        // A rename in the engine would leave a stale id here, and the command unguarded.
        for id in WRITES_TO_PATH {
            assert!(figcraft_engine::cmd::find_command(id).is_some(), "{id}");
            assert!(!HANDLED_BY_SAWIT.contains(id), "{id} is refused already");
        }
    }

    /// The names of what is in `dir`, in order.
    fn names_in(dir: &std::path::Path) -> Vec<String> {
        let entries = std::fs::read_dir(dir).unwrap();
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn an_export_aimed_at_a_canvas_file_is_refused_and_the_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let files = canvas_files(dir.path());
        let folder = dir.path().join(".nyawit");
        let read = |files: &[std::path::PathBuf]| -> Vec<Vec<u8>> {
            files.iter().map(|f| std::fs::read(f).unwrap()).collect()
        };
        let (bytes, names) = (read(&files), names_in(&folder));
        let mut board = board();
        square_at(&mut board, 0.0);

        for id in WRITERS {
            for file in &files {
                let reply = send(&mut board, id, json!({"path": file}));
                assert_eq!(reply, would_overwrite(id), "{id} to {}", file.display());
            }
        }

        // Every file is as it was, byte for byte, and nothing was left beside them.
        assert_eq!(read(&files), bytes);
        assert_eq!(names_in(&folder), names);
    }

    #[test]
    fn exports_to_other_paths_still_write() {
        let dir = tempfile::tempdir().unwrap();
        let canvas = canvas_files(dir.path());
        let mut board = board();
        square_at(&mut board, 0.0);

        // What the name ends in decides, not what it holds: `.sawit.svg` is an SVG file.
        let writes = [
            ("file.export", "page.png"),
            ("file.exportSvg", "page.svg"),
            ("file.exportSvg", "canvas.sawit.svg"),
            ("file.export", "design.sawit.png"),
            ("variable.export", "variables.json"),
            ("codeConnect.export", "links.json"),
        ];
        for (id, name) in writes {
            let path = dir.path().join(name);
            let reply = send(&mut board, id, json!({"path": path}));
            assert_eq!(reply["ok"], json!(true), "{id} to {name}: {reply}");
            assert!(path.is_file(), "{id} wrote no {name}");
        }
        let page = std::fs::read(dir.path().join("page.png")).unwrap();
        assert!(page.starts_with(b"\x89PNG"), "{:02x?}", &page[..8]);
        let svg = std::fs::read_to_string(dir.path().join("canvas.sawit.svg")).unwrap();
        assert!(svg.starts_with("<svg"), "{svg}");
        // And the canvas files beside them are as they were.
        for file in &canvas {
            let path = file.to_string_lossy();
            assert_eq!(std::fs::read(file).unwrap(), path.as_bytes());
        }
    }

    #[test]
    fn a_batch_step_that_would_overwrite_a_canvas_file_is_refused_before_anything_runs() {
        let dir = tempfile::tempdir().unwrap();
        let canvas = canvas_files(dir.path()).remove(0);
        let was = std::fs::read(&canvas).unwrap();
        let mut board = board();
        let before = board.doc().unwrap();

        for id in WRITERS {
            let reply = send(
                &mut board,
                "command.batch",
                json!({"commands": [
                    {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                    {"command": "command.batch", "params": {"commands": [
                        {"command": id, "params": {"path": canvas}},
                    ]}},
                ]}),
            );
            assert_eq!(reply, would_overwrite(id));
        }

        // Not even the sticky before the refused step was made, and the file is as it was.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));
        assert_eq!(std::fs::read(&canvas).unwrap(), was);
    }

    /// CANVAS-CORE-7: a step of a `command.batch` can take its params from an earlier step's
    /// result (`{"$ref": name, "field": …}`, `{"$step": n}`), which the engine puts in place as the
    /// batch runs. A writer's path taken so is no text to check before then, and went through: the
    /// batch below wrote SVG over the canvas file. Such a writer is refused, and nothing runs.
    #[test]
    fn a_batch_writer_whose_path_comes_from_another_step_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let canvas = canvas_files(dir.path()).remove(0);
        let was = std::fs::read(&canvas).unwrap();
        let mut board = board();
        send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let before = board.doc().unwrap();
        let by_name = json!({"$ref": "pages", "field": "0/name"});
        let by_step = json!({"$step": 1, "field": "0/name"});
        for params in [
            json!({"path": by_name}),
            json!({"path": by_step}),
            json!({"$ref": "pages", "field": "0"}),
            json!({"format": "svg", "path": [by_name]}),
        ] {
            let reply = send(
                &mut board,
                "command.batch",
                json!({"commands": [
                    {"command": "page.rename", "params": {"name": canvas}},
                    {"command": "page.list", "as": "pages"},
                    {"command": "file.exportSvg", "params": params},
                ]}),
            );
            assert_eq!(reply["ok"], json!(false), "{params}: {reply}");
            let error = reply["error"].as_str().unwrap();
            assert!(
                error.starts_with("file.exportSvg in a command.batch needs its params written out"),
                "{params}: {error}"
            );
            assert_eq!(std::fs::read(&canvas).unwrap(), was, "{params}");
        }
        // So is such a writer in a nested batch, whose references are to the nested batch's own
        // steps.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "command.batch", "params": {"commands": [
                    {"command": "page.rename", "params": {"name": canvas}},
                    {"command": "page.list", "as": "pages"},
                    {"command": "file.exportSvg", "params": {"path": by_name}},
                ]}},
            ]}),
        );
        let error = reply["error"].as_str().unwrap_or_default();
        assert!(
            error.starts_with("file.exportSvg in a command.batch needs its params written out"),
            "{reply}"
        );
        assert_eq!(std::fs::read(&canvas).unwrap(), was);
        // A nested batch's own params are passed on as written, never taken from a step: one that
        // is a reference holds no steps, and the engine refuses it.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "page.list", "as": "pages"},
                {"command": "command.batch", "params": {"$ref": "pages", "field": "0"}},
            ]}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        assert_eq!(std::fs::read(&canvas).unwrap(), was);
        // Outside a batch nothing is taken from a step, so nothing is refused for it: a path that
        // is no text writes nothing (the engine gives the export back instead).
        let top = send(
            &mut board,
            "file.exportSvg",
            json!({"path": {"$ref": "pages"}}),
        );
        assert_eq!(top["ok"], json!(true), "{top}");
        assert!(top["result"]["data"].is_string(), "{top}");
        assert_eq!(std::fs::read(&canvas).unwrap(), was);
        // None of them changed the document.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        // A writer of a path written out, as a step's own text, still runs.
        let svg = dir.path().join("page.svg");
        let written = send(
            &mut board,
            "command.batch",
            json!({"commands": [{"command": "file.exportSvg", "params": {"path": svg}}]}),
        );
        assert_eq!(written["ok"], json!(true), "{written}");
        assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));
    }

    /// A nested `command.batch` gets its params as written: the engine puts no other step's result
    /// in them. The nested batch puts its own steps' results in place as they run, so one whose
    /// steps refer to each other is an ordinary batch, and runs, as it does in FigCraft.
    #[test]
    fn a_nested_batch_whose_steps_refer_to_each_other_runs() {
        let mut board = board();
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "command.batch", "params": {"commands": [
                    {"command": "board.sticky", "params": {"x": 0, "y": 0}, "as": "inner"},
                    {"command": "node.set", "params": {"id": {"$ref": "inner"}, "props": {"name": "renamed"}}},
                ]}},
            ]}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let sticky = id_of(&reply["result"][0][0]["id"]).unwrap();
        assert_eq!(board.doc().unwrap().node(sticky).unwrap().name, "renamed");
    }

    /// A 1 × 1 PNG, as base64.
    const DOT: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    /// The engine's commands that read the file a parameter names, each with its params naming
    /// `path` there: every reader of `READS_FROM_PATH`, and each parameter it reads a file from.
    fn reads_of(path: &std::path::Path) -> Vec<(&'static str, Value)> {
        vec![
            ("file.import", json!({"path": path})),
            (
                "file.import",
                json!({"data": DOT, "name": "dot.png", "variables": path}),
            ),
            ("file.recover", json!({"path": path})),
            ("image.place", json!({"path": path})),
            ("variable.import", json!({"variables": path})),
            (
                "collab.recoverFile",
                json!({"path": path, "session": 1, "client": 1, "seq": 1}),
            ),
        ]
    }

    /// The engine reads the file a command's params name whole, as it finds it, on the canvas's
    /// UI thread (`file.import {path}`, `image.place {path}`, `variable.import {variables}`...): a
    /// named pipe blocked it for ever, and a link to `/dev/zero` read until the Mac ran out of
    /// memory, as `replace` did (CANVAS-CORE-6). Each is refused at once, as a command and as a
    /// step of a batch, before anything runs; and so is a file past what is read from one file.
    #[test]
    fn a_command_that_reads_a_pipe_or_a_device_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("pipe.png");
        crate::store::pipe_at(&pipe);
        let batched = |id: &str, params: &Value| {
            json!({"op": "command", "command": "command.batch", "params": {"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 300}},
                {"command": id, "params": params},
            ]}})
        };
        let refused_at_once = |path: &std::path::Path, why: &str| {
            let shown = path.display().to_string();
            // Each is sent to a board of its own, and what failed is told all at once: each one
            // that hangs is seen.
            let failed: Vec<String> = reads_of(path)
                .into_iter()
                .flat_map(|(id, params)| {
                    let alone = json!({"op": "command", "command": id, "params": params});
                    [(id, alone), (id, batched(id, &params))]
                })
                .filter_map(|(id, op)| {
                    let Some((reply, unchanged)) = on_a_board_of_its_own(op.clone()) else {
                        return Some(format!("{op}: no answer within 5 s"));
                    };
                    let error = reply["error"].as_str().unwrap_or_default();
                    let refused = error.starts_with(&format!("{id} can't read {shown}: {why}"));
                    (!refused || !unchanged).then(|| format!("{op}: {reply}"))
                })
                .collect();
            assert!(failed.is_empty(), "{}", failed.join("\n"));
        };
        // The pipe first: a device that never ends is read only once it is refused.
        refused_at_once(&pipe, "not a regular file");
        let zero = dir.path().join("zero.png");
        std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
        refused_at_once(&zero, "not a regular file");
        let big = dir.path().join("big.png");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(crate::store::MAX_FILE + 1).unwrap();
        refused_at_once(&big, "it holds more than 4096 MB");

        // A file that is one is read, and one that isn't there is the engine's to report.
        let png = dir.path().join("dot.png");
        std::fs::write(&png, STANDARD.decode(DOT).unwrap()).unwrap();
        let mut board = board();
        let placed = send(&mut board, "image.place", json!({"path": png}));
        assert_eq!(placed["ok"], json!(true), "{placed}");
        let missing = dir.path().join("missing.png");
        let reply = send(&mut board, "image.place", json!({"path": missing}));
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().unwrap();
        assert!(!error.contains("can't read"), "{error}");
    }

    /// A step of a batch that reads a file can take the file's name from another step's result,
    /// as a writer can take its path (CANVAS-CORE-7): that name can't be checked before the batch
    /// runs, so the step is refused, and nothing runs. Its other params may come from a step: an
    /// image placed from its bytes in a frame an earlier step made runs.
    #[test]
    fn a_batch_reader_whose_file_comes_from_another_step_is_refused() {
        let mut board = design();
        let before = board.doc().unwrap();
        let by_name = json!({"$ref": "pages", "field": "0/name"});
        let steps = [
            ("file.import", json!({"path": by_name})),
            (
                "file.import",
                json!({"data": DOT, "name": "dot.png", "variables": by_name}),
            ),
            ("file.recover", json!({"$ref": "pages", "field": "0"})),
            ("image.place", json!({"path": [by_name]})),
            ("variable.import", json!({"variables": by_name})),
            (
                "collab.recoverFile",
                json!({"path": {"$step": 0, "field": "0/name"}}),
            ),
        ];
        for (id, params) in steps {
            let reply = send(
                &mut board,
                "command.batch",
                json!({"commands": [
                    {"command": "page.list", "as": "pages"},
                    {"command": "command.batch", "params": {"commands": [
                        {"command": "page.list", "as": "pages"},
                        {"command": id, "params": params},
                    ]}},
                ]}),
            );
            let error = reply["error"].as_str().unwrap_or_default();
            let expected = format!("{id} in a command.batch needs its params written out");
            assert!(error.starts_with(&expected), "{params}: {reply}");
        }
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));

        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "node.create", "as": "frame",
                 "params": {"type": "frame", "x": 0, "y": 0, "width": 100, "height": 100}},
                {"command": "image.place", "params": {"data": DOT, "parent": {"$ref": "frame"}}},
            ]}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let frame = id_of(&reply["result"][0]["id"]).unwrap();
        let image = id_of(&reply["result"][1]["id"]).unwrap();
        assert_eq!(
            board.doc().unwrap().node(image).unwrap().parent,
            Some(frame)
        );
    }

    #[test]
    fn the_guarded_readers_are_engine_commands_that_nothing_else_refuses() {
        for (id, _) in READS_FROM_PATH {
            assert!(figcraft_engine::cmd::find_command(id).is_some(), "{id}");
            assert!(!HANDLED_BY_SAWIT.contains(id), "{id} is refused already");
            assert!(!WRITES_TO_PATH.contains(id), "{id} is a writer");
        }
        let mut readers: Vec<&str> = reads_of(std::path::Path::new("/x"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        readers.dedup();
        let guarded: Vec<&str> = READS_FROM_PATH.iter().map(|(id, _)| *id).collect();
        assert_eq!(readers, guarded);
    }

    /// `file.exportLayers` and `data.fill` write into a folder, naming each file after its layer
    /// and then adding its format's extension, so they can't make a canvas file's name even from
    /// the worst layer name: no check is needed, which is why they aren't among `WRITERS`.
    #[test]
    fn exports_into_a_folder_never_make_a_canvas_file() {
        let dir = tempfile::tempdir().unwrap();
        let canvas = canvas_files(dir.path());
        let folder = dir.path().join(".nyawit");
        let mut board = design();
        let frame = made(
            &mut board,
            "node.create",
            json!({"type": "frame", "x": 0, "y": 0, "width": 100, "height": 100}),
        );
        let name = json!({"ids": [frame], "props": {"name": "canvas.sawit"}});
        board.execute("node.set", &name).unwrap();
        let png = json!({"ids": [frame], "setting": {"format": "png"}});
        board.execute("export.add", &png).unwrap();

        let reply = send(&mut board, "file.exportLayers", json!({"dir": folder}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let rows = json!([{"name": "canvas.sawit"}]);
        let fill = json!({"id": frame, "rows": rows, "dir": folder, "format": "svg"});
        let reply = send(&mut board, "data.fill", fill);
        assert_eq!(reply["ok"], json!(true), "{reply}");

        // What they made ends in an image extension, and the canvas files are as they were.
        let names = names_in(&folder);
        assert!(names.iter().any(|n| n == "canvas.sawit.png"), "{names:?}");
        assert!(names.iter().any(|n| n.ends_with(".svg")), "{names:?}");
        for file in &canvas {
            let path = file.to_string_lossy();
            assert_eq!(std::fs::read(file).unwrap(), path.as_bytes());
        }
    }

    #[test]
    fn a_recovered_file_would_replace_the_canvas_and_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        let mut board = board();
        let reply = send(
            &mut board,
            "board.sticky",
            json!({"x": 0, "y": 0, "text": "mine"}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        let reply = send(&mut board, "file.recover", json!({"path": other}));

        assert_eq!(reply, would_switch("file.recover"));
        // The canvas holds its one document, on show and as it was: `get` lists its one sticky,
        // not the two of the recovered file.
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(documents(&board), documents_before);
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn a_batch_that_opens_a_document_is_undone_whole() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        let mut board = board();
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "file.recover", "params": {"path": other}},
            ]}),
        );

        assert_eq!(reply, would_switch("command.batch"));
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(documents(&board), documents_before);
        // The sticky the batch made before it opened the file went with it: the canvas is the
        // very document it was, and its history has no step for the batch.
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
        assert!(elements(&mut board).is_empty());
        assert_eq!(board.session.undo_depth(), (0, 0));
    }

    #[test]
    fn a_batch_that_opens_a_document_and_switches_back_is_refused_too() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        let mut board = board();
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        // At its end the canvas's document is on show again, but a second one is open.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "file.recover", "params": {"path": other}},
                {"command": "doc.switch", "params": {"index": 0}},
            ]}),
        );

        assert_eq!(reply, would_switch("command.batch"));
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(documents(&board), documents_before);
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
    }

    #[test]
    fn other_commands_that_open_a_document_are_refused_too() {
        let mut board = board();
        let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        // A version of the canvas, for `version.duplicate` to open.
        let reply = send(&mut board, "file.saveVersion", json!({"name": "First"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        for (id, params) in [
            ("version.duplicate", json!({"index": 0})),
            ("branch.create", json!({"name": "Try it"})),
        ] {
            let reply = send(&mut board, id, params);
            assert_eq!(reply, would_switch(id));
            assert_eq!(documents(&board), documents_before, "{id}");
            assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()), "{id}");
        }
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// How many versions and branches the canvas's file has: what its next save would write of
    /// them.
    fn history(board: &BoardDoc) -> (usize, usize) {
        let ds = board.session.ds().unwrap();
        (ds.versions.len(), ds.branches.len())
    }

    #[test]
    fn a_failed_command_op_leaves_no_version_or_branch_to_save() {
        let mut board = board();
        let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert_eq!(reply["ok"], json!(true), "{reply}");

        // `branch.create` adds its branch (two copies of the document) to the canvas's file
        // before it opens it in a tab. The guard closes the tab, and the branch goes with it.
        let reply = send(&mut board, "branch.create", json!({"name": "Try it"}));
        assert_eq!(reply, would_switch("branch.create"));
        assert_eq!(history(&board), (0, 0));

        // A batch the guard refuses: what its steps added to the file goes with it too.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "file.saveVersion", "params": {"name": "First"}},
                {"command": "branch.create", "params": {"name": "Try it"}},
            ]}),
        );
        assert_eq!(reply, would_switch("command.batch"));
        assert_eq!(history(&board), (0, 0));

        // So does a batch that fails on a step of its own: none of it is done.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "file.saveVersion", "params": {"name": "First"}},
                {"command": "no.such.command"},
            ]}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        assert_eq!(history(&board), (0, 0));

        // A command that goes through keeps what it adds.
        let reply = send(&mut board, "file.saveVersion", json!({"name": "Kept"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(history(&board), (1, 0));
        // And a refusal after it leaves that version where it is.
        let reply = send(&mut board, "branch.create", json!({}));
        assert_eq!(reply, would_switch("branch.create"));
        assert_eq!(history(&board), (1, 0));
        assert_eq!(board.session.ds().unwrap().versions[0].name, "Kept");
    }

    /// A session whose one document is a board called "Canvas", made by the engine itself.
    fn session_with_a_canvas() -> Session {
        let mut session = Session::empty();
        session
            .execute("file.newBoard", &json!({"name": "Canvas"}))
            .unwrap();
        session
    }

    #[test]
    fn documents_are_told_apart_by_uid_not_by_count_and_position() {
        let mut session = session_with_a_canvas();
        let before = Documents::of(&session);
        assert_eq!(Documents::of(&session), before);

        // Close the canvas's document and open another in its place: still one document, and
        // still the first on show.
        session
            .execute("file.newBoard", &json!({"name": "Other"}))
            .unwrap();
        session
            .execute("file.close", &json!({"index": 0, "force": true}))
            .unwrap();
        assert_eq!((session.docs.len(), session.active), (1, 0));
        assert_ne!(Documents::of(&session), before);
    }

    #[test]
    fn restoring_shows_the_canvas_wherever_it_now_is() {
        let mut session = session_with_a_canvas();
        let before = Documents::of(&session);

        // Two more documents, the canvas's moved behind them, the first of them on show.
        for name in ["One", "Two"] {
            session
                .execute("file.newBoard", &json!({"name": name}))
                .unwrap();
        }
        session
            .execute("file.moveTab", &json!({"from": 0, "to": 2}))
            .unwrap();
        session.execute("doc.switch", &json!({"index": 0})).unwrap();
        assert_eq!((session.docs.len(), session.active), (3, 0));
        assert_ne!(Documents::of(&session), before);

        before.restore(&mut session);
        assert_eq!(Documents::of(&session), before);
        assert_eq!((session.docs.len(), session.active), (1, 0));
        assert_eq!(session.doc().unwrap().name, "Canvas");
    }

    #[test]
    fn a_failure_that_opens_nothing_keeps_the_engines_own_words() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.figcraft");
        let mut board = board();
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        let reply = send(&mut board, "file.recover", json!({"path": missing}));
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("missing.figcraft"), "{error}");
        assert!(!error.contains("would open or switch"), "{error}");

        // A step that fails rolls the batch back, and the step's error is what is answered.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "no.such.command"},
            ]}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("no.such.command"), "{error}");
        assert!(!error.contains("would open or switch"), "{error}");

        assert_eq!(documents(&board), documents_before);
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));
    }

    #[test]
    fn command_ops_undo_and_redo_what_an_op_did() {
        let mut board = board();
        let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(elements(&mut board).len(), 1);

        let reply = send(&mut board, "edit.undo", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(elements(&mut board).is_empty());

        // Undoing isn't an edit of its own: the step can be redone.
        let reply = send(&mut board, "edit.redo", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn a_batch_is_one_undo_step_with_its_own_label() {
        let mut board = board();
        let stickies = |label: Option<&str>| {
            let steps = json!([
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "board.sticky", "params": {"x": 300, "y": 0}},
            ]);
            match label {
                Some(label) => json!({"label": label, "commands": steps}),
                None => json!({"commands": steps}),
            }
        };

        let reply = send(&mut board, "command.batch", stickies(None));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["result"].as_array().map(Vec::len), Some(2));
        assert_eq!(board.session.undo_depth(), (1, 0));
        assert_eq!(board.session.undo_labels(5), ["Batch"]);

        let reply = send(&mut board, "command.batch", stickies(Some("Two more")));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(board.session.undo_depth(), (2, 0));
        assert_eq!(board.session.undo_labels(5), ["Two more", "Batch"]);
        assert_eq!(elements(&mut board).len(), 4);

        // One undo takes back one batch.
        let reply = send(&mut board, "edit.undo", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(elements(&mut board).len(), 2);
    }

    #[test]
    fn a_batch_that_opens_nothing_runs_as_the_engine_alone_would_run_it() {
        let sticky = json!({"command": "board.sticky", "params": {"x": 0, "y": 0}});
        let (undo, redo) = (
            json!({"command": "edit.undo"}),
            json!({"command": "edit.redo"}),
        );
        let nested = json!({"command": "command.batch", "params": {
            "label": "Inner",
            "commands": [sticky, sticky],
        }});
        let broken = json!({"command": "no.such.command"});
        let batches = [
            json!({"commands": []}),
            json!({"commands": [sticky, sticky]}),
            json!({"label": "Named", "commands": [sticky]}),
            json!({"commands": [sticky, undo]}),
            json!({"commands": [sticky, undo, redo]}),
            json!({"commands": [nested, sticky]}),
            json!({"commands": [sticky, broken]}),
            json!({"onError": "continue", "commands": [sticky, broken, sticky]}),
        ];

        for batch in batches {
            let (mut through_the_op, mut engine_alone) = (board(), board());
            let reply = send(&mut through_the_op, "command.batch", batch.clone());
            let expected = match engine_alone.execute("command.batch", &batch) {
                Ok(result) => json!({"ok": true, "result": result}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            };
            assert_eq!(reply, expected, "{batch}");

            // The same undo steps (and what can be redone), history of commands and layers.
            let state = |board: &mut BoardDoc| {
                (
                    board.session.undo_depth(),
                    board.session.undo_labels(10),
                    board.session.journal.clone(),
                    elements(board).len(),
                )
            };
            assert_eq!(
                state(&mut through_the_op),
                state(&mut engine_alone),
                "{batch}"
            );
        }
    }

    /// A design file, which takes no board objects.
    fn design() -> BoardDoc {
        let bytes = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
        BoardDoc::from_bytes(&bytes).unwrap()
    }

    /// Sends `op`, which has to answer `{ok: true, ...}`.
    fn ask(board: &mut BoardDoc, op: Value) -> Value {
        let reply = run(board, &op);
        assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
        reply
    }

    /// Sends `op`, which has to answer `{ok: false, error}`, and gives the error.
    fn refusal(board: &mut BoardDoc, op: Value) -> String {
        let reply = run(board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        let error = reply["error"].as_str().expect("an error string");
        error.to_string()
    }

    /// Adds `elements` with the `add` op and gives the ids it answered.
    fn add(board: &mut BoardDoc, elements: Value) -> Vec<String> {
        let reply = ask(board, json!({"op": "add", "elements": elements}));
        let ids = reply["ids"].as_array().expect("ids");
        ids.iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect()
    }

    /// The element `get` lists under `id`.
    fn listed(board: &mut BoardDoc, id: &str) -> Value {
        let found = elements(board).into_iter().find(|e| e["id"] == json!(id));
        found.unwrap_or_else(|| panic!("{id} is not listed"))
    }

    /// The layers of the page on show, labels among them.
    fn layers_on_page(board: &BoardDoc) -> usize {
        let ds = board.session.ds().unwrap();
        ds.doc.node(ds.page).unwrap().children.len()
    }

    /// How many connector labels the document holds.
    fn labels(board: &BoardDoc) -> usize {
        let doc = board.doc().unwrap();
        doc.nodes()
            .filter(|n| matches!(n.board, Some(BoardItem::Label { .. })))
            .count()
    }

    /// The first solid fill and stroke of layer `id`, as `#RRGGBB`.
    fn paints(board: &BoardDoc, id: &str) -> (Option<String>, Option<String>) {
        let doc = board.doc().unwrap();
        let node = doc.node(id.parse().unwrap()).unwrap();
        let first = |paints: &[Paint]| paints.iter().find_map(Paint::solid_color);
        let (fill, stroke) = (first(&node.fills), first(&node.strokes));
        (fill.map(Color::to_css_hex), stroke.map(Color::to_css_hex))
    }

    #[test]
    fn get_summarises_types_and_text() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "sticky", "id": "s", "x": 0, "y": 0, "text": "hi"},
                {"type": "diamond", "id": "d", "x": 400, "y": 0},
                {"type": "arrow", "start": {"id": "s"}, "end": {"id": "d"}, "label": {"text": "yes"}},
            ]),
        );

        // Three elements: the connector's label is a layer of its own, but not an element (P7-C4).
        assert_eq!(layers_on_page(&board), 4);
        let elements = elements(&mut board);
        assert_eq!(elements.len(), 3, "{elements:?}");
        let types: Vec<&str> = elements
            .iter()
            .map(|e| e["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["sticky", "diamond", "connector"]);
        let listed_ids: Vec<&str> = elements.iter().map(|e| e["id"].as_str().unwrap()).collect();
        assert_eq!(listed_ids, ids);

        assert_eq!(elements[0]["text"], json!("hi"));
        assert!(elements[1].get("text").is_none(), "{}", elements[1]);
        // The connector says what it joins, and its text is its label's.
        assert_eq!(elements[2]["from"], json!(ids[0]));
        assert_eq!(elements[2]["to"], json!(ids[1]));
        assert_eq!(elements[2]["text"], json!("yes"));
        for element in &elements {
            for key in ["x", "y", "width", "height"] {
                assert!(element[key].is_number(), "{key} of {element}");
            }
        }
    }

    #[test]
    fn add_returns_ids_in_order() {
        let mut board = board();
        // Made in another order (shapes before text, text before what joins them), answered in
        // this one.
        let reply = ask(
            &mut board,
            json!({"op": "add", "elements": [
                {"type": "arrow", "start": {"id": "a"}, "end": {"id": "b"}},
                {"type": "text", "x": 0, "y": -50, "text": "Title"},
                {"type": "rectangle", "id": "b", "x": 300, "y": 0, "width": 100, "height": 60},
                {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 100, "height": 60},
            ]}),
        );
        assert_eq!(reply["added"], json!(4));
        assert!(reply.get("notes").is_none(), "{reply}");
        let ids: Vec<String> = reply["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids.len(), 4);
        let at = |board: &mut BoardDoc, id: &String| {
            let element = listed(board, id);
            (element["type"].clone(), element["x"].clone())
        };
        assert_eq!(at(&mut board, &ids[0]).0, json!("connector"));
        assert_eq!(at(&mut board, &ids[1]), (json!("text"), json!(0.0)));
        assert_eq!(at(&mut board, &ids[2]), (json!("rectangle"), json!(300.0)));
        assert_eq!(at(&mut board, &ids[3]), (json!("rectangle"), json!(0.0)));
        // The arrow joins the two rectangles by the ids the agent gave them.
        let connector = listed(&mut board, &ids[0]);
        assert_eq!(connector["from"], json!(ids[3]));
        assert_eq!(connector["to"], json!(ids[2]));
    }

    #[test]
    fn add_notes_what_it_skips() {
        let mut board = board();
        let reply = ask(
            &mut board,
            json!({"op": "add", "elements": [
                {"type": "hologram", "x": 0, "y": 0},
                {"type": "rectangle", "x": 0, "y": 0},
            ]}),
        );
        // Skipped with a note, not an error, and the rest is drawn.
        assert_eq!(reply["added"], json!(1));
        assert_eq!(reply["ids"].as_array().map(Vec::len), Some(1));
        let notes = reply["notes"].as_array().expect("notes");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].as_str().unwrap().contains("hologram"), "{notes:?}");
        assert_eq!(elements(&mut board).len(), 1);

        // Nothing to add adds nothing.
        let reply = ask(&mut board, json!({"op": "add", "elements": []}));
        assert_eq!(reply, json!({"ok": true, "added": 0, "ids": []}));
    }

    #[test]
    fn add_reports_malformed_elements() {
        let mut board = board();
        let before = board.doc().unwrap();
        for op in [
            json!({"op": "add"}),
            json!({"op": "add", "elements": null}),
            json!({"op": "add", "elements": "rectangle"}),
            json!({"op": "add", "elements": {"type": "rectangle"}}),
            json!({"op": "add", "elements": ["rectangle"]}),
            json!({"op": "add", "elements": [{"type": "rectangle", "x": "far away"}]}),
            json!({"op": "add", "elements": [{"type": "rectangle", "x": 0, "y": 0}, 5]}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.contains("elements"), "{op}: {error}");
        }
        // The engine refusing an element fails the whole call, and says which one.
        let error = refusal(
            &mut board,
            json!({"op": "add", "elements": [
                {"type": "rectangle", "x": 0, "y": 0},
                {"type": "rectangle", "x": 0, "y": 0, "width": 0},
            ]}),
        );
        assert!(error.contains("elements[1]"), "{error}");
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));
    }

    /// SMOKE-3: what only a board has (shapes, stickies, lines, ink) doesn't go on a design file.
    /// The refusal says so in Sawit's words, with what does go there and the commands that draw
    /// the rest, instead of sending the agent to File ▸ New board, which the canvas hides (and no
    /// op makes a design file a board: `replace` keeps the canvas what it is). Text, frames and
    /// images still come in.
    #[test]
    fn add_to_a_design_file_is_refused() {
        let mut design = design();
        let before = design.doc().unwrap();
        let error = refusal(
            &mut design,
            json!({"op": "add", "elements": [{"type": "rectangle", "x": 0, "y": 0}]}),
        );
        assert_eq!(
            error,
            "add: elements[0] (rectangle): command `board.shape` is not available right now: this \
             is for boards, and this canvas is a design file. On a design file add draws text, \
             frames and images; draw shapes and lines with the command op: shape.rectangle, \
             shape.ellipse, shape.line or shape.arrow"
        );
        for kind in ["ellipse", "diamond", "sticky", "arrow", "line", "freedraw"] {
            let element = json!({"type": kind, "x": 0, "y": 0, "points": [[0, 0], [10, 10]]});
            let error = refusal(&mut design, json!({"op": "add", "elements": [element]}));
            assert!(
                error.contains("this canvas is a design file. On a design file add draws"),
                "{kind}: {error}"
            );
            assert!(!error.contains("New board"), "{kind}: {error}");
        }
        assert!(Arc::ptr_eq(&before, &design.doc().unwrap()));
        for kind in ["text", "frame"] {
            let element = json!({"type": kind, "x": 0, "y": 0, "width": 100, "text": "kept"});
            ask(&mut design, json!({"op": "add", "elements": [element]}));
        }
        assert_eq!(elements(&mut design).len(), 2);
    }

    /// SMOKE-3: a Mermaid diagram drawn as shapes and connectors doesn't go on a design file, and
    /// the refusal says why in Sawit's words, not the engine's for one of the diagram's own
    /// elements. A chart, which comes in as one picture, still does.
    #[test]
    fn mermaid_on_a_design_file_says_why() {
        let mut design = design();
        let before = design.doc().unwrap();
        let error = refusal(
            &mut design,
            json!({"op": "mermaid", "source": "graph TD; A-->B; B-->C"}),
        );
        assert_eq!(
            error,
            "mermaid: this canvas is a design file, and a diagram's shapes and connectors are for \
             boards: bring the diagram in as an SVG file with the command op's file.import instead"
        );
        assert!(Arc::ptr_eq(&before, &design.doc().unwrap()));
        let pie = "pie title Pets\n \"Dogs\" : 386\n \"Cats\" : 85";
        ask(&mut design, json!({"op": "mermaid", "source": pie}));
        assert_eq!(elements(&mut design).len(), 1);
    }

    /// SMOKE-3: a board's command run on a design file by the `command` op says what the canvas
    /// is, in Sawit's words, too.
    #[test]
    fn a_boards_command_on_a_design_file_says_what_the_canvas_is() {
        let mut design = design();
        let reply = send(&mut design, "board.sticky", json!({"x": 0, "y": 0}));
        assert_eq!(
            reply["error"],
            json!(
                "command `board.sticky` is not available right now: this is for boards, and this \
                 canvas is a design file"
            ),
            "{reply}"
        );
    }

    #[test]
    fn update_moves_and_relabels() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([{"type": "sticky", "id": "s", "x": 0, "y": 20, "text": "Old"}]),
        );

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": ids[0], "x": 500, "text": "New"}]}),
        );

        assert_eq!(reply, json!({"ok": true, "updated": 1, "missing": []}));
        let element = listed(&mut board, &ids[0]);
        assert_eq!(element["x"], json!(500.0));
        // What the element didn't name stays.
        assert_eq!(element["y"], json!(20.0));
        assert_eq!(element["text"], json!("New"));
    }

    #[test]
    fn update_reports_missing() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([{"type": "rectangle", "id": "box", "x": 0, "y": 0}]),
        );
        let before = board.doc().unwrap();

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "nope", "x": 5}]}),
        );
        assert_eq!(
            reply,
            json!({"ok": true, "updated": 0, "missing": ["nope"]})
        );
        // Nothing changed, so there is no step to undo.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (1, 0));

        // What is there is updated, what isn't is named, in the order it was given. The page
        // is no element, though its node id is a number like the others'.
        let page = board.doc().unwrap().first_page().unwrap().to_string();
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "gone", "x": 1}, {"id": "box", "x": 5}, {"id": page, "x": 5}, {"id": ids[0], "y": 7},
            ]}),
        );
        assert_eq!(reply["updated"], json!(2));
        assert_eq!(reply["missing"], json!(["gone", page]));
        let element = listed(&mut board, &ids[0]);
        assert_eq!(
            (element["x"].clone(), element["y"].clone()),
            (json!(5.0), json!(7.0))
        );
    }

    #[test]
    fn update_changes_size_text_name_and_colours() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "box", "x": 0, "y": 0, "width": 100, "height": 60,
                 "label": {"text": "Old"}},
                {"type": "sticky", "id": "note", "x": 300, "y": 0, "text": "Old"},
                {"type": "text", "id": "title", "x": 0, "y": -100, "text": "Old"},
                {"type": "frame", "id": "area", "x": 0, "y": 300, "width": 200, "height": 100,
                 "name": "Old"},
                {"type": "arrow", "id": "link", "start": {"id": "box"}, "end": {"id": "note"}},
            ]),
        );

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "box", "x": 20, "y": 40, "width": 140, "height": 80, "text": "Box",
                 "backgroundColor": "#2f9e44", "strokeColor": "#e03131"},
                {"id": "note", "label": {"text": "Note"}, "backgroundColor": "pink"},
                {"id": "title", "text": "Title", "strokeColor": "#e03131"},
                {"id": "area", "name": "Area", "width": 250},
                {"id": "link", "text": "uses", "strokeColor": "#1971c2"},
            ]}),
        );
        assert_eq!(reply, json!({"ok": true, "updated": 5, "missing": []}));

        let shape = listed(&mut board, &ids[0]);
        assert_eq!(
            [&shape["x"], &shape["y"], &shape["width"], &shape["height"]],
            [&json!(20.0), &json!(40.0), &json!(140.0), &json!(80.0)]
        );
        assert_eq!(shape["text"], json!("Box"));
        assert_eq!(shape["color"], json!("#2F9E44"));
        // The stroke is the outline's, the layer inside the shape.
        let doc = board.doc().unwrap();
        let outline = doc.node(ids[0].parse().unwrap()).unwrap().children[0];
        assert_eq!(
            paints(&board, &outline.to_string()).1.as_deref(),
            Some("#E03131")
        );

        let sticky = listed(&mut board, &ids[1]);
        assert_eq!(sticky["text"], json!("Note"));
        assert_eq!(sticky["color"], json!("#F7B9DA"));

        // A text layer's colour is its `strokeColor` (P7-C24).
        let text = listed(&mut board, &ids[2]);
        assert_eq!(text["text"], json!("Title"));
        assert_eq!(text["color"], json!("#E03131"));
        assert_eq!(paints(&board, &ids[2]).1, None);

        let frame = listed(&mut board, &ids[3]);
        assert_eq!(frame["name"], json!("Area"));
        assert_eq!(frame["width"], json!(250.0));

        // A connector's text is a label, made on the way.
        let connector = listed(&mut board, &ids[4]);
        assert_eq!(connector["text"], json!("uses"));
        assert_eq!(connector["color"], json!("#1971C2"));
        assert_eq!(labels(&board), 1);
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "link", "text": ""}]}),
        );
        assert_eq!(reply["updated"], json!(1));
        assert!(listed(&mut board, &ids[4]).get("text").is_none());
        assert_eq!(labels(&board), 0);
    }

    #[test]
    fn update_colours_by_the_rules_apply_uses() {
        // One helper colours what `add` draws and what `update` changes (P7-C12), so naming the
        // colours when drawing and naming them afterwards give the same document.
        let bare = json!([
            {"type": "rectangle", "x": 0, "y": 0},
            {"type": "rectangle", "x": 300, "y": 0},
            {"type": "sticky", "x": 600, "y": 0},
            {"type": "text", "x": 0, "y": 300, "text": "t"},
            {"type": "arrow", "x": 0, "y": 400, "width": 100, "height": 0},
            {"type": "freedraw", "x": 0, "y": 500, "points": [[0, 0], [30, 40], [60, 0]]},
            {"type": "frame", "x": 0, "y": 600, "width": 200, "height": 100},
        ]);
        let colours = [
            json!({"backgroundColor": "#74c0fc", "strokeColor": "#e03131"}),
            json!({"backgroundColor": "transparent", "strokeColor": "#2f9e44"}),
            json!({"backgroundColor": "violet"}),
            json!({"backgroundColor": "#ff0000", "strokeColor": "#1971c2"}),
            // A connector has no fill: the background would recolour its line, so it is ignored.
            json!({"backgroundColor": "#ffc9c9", "strokeColor": "#9c36b5"}),
            json!({"backgroundColor": "#ffc9c9", "strokeColor": "#e8590c"}),
            json!({"backgroundColor": "#d3f9d8", "strokeColor": "#2f9e44"}),
        ];

        let mut drawn = board();
        let mut elements = bare.clone();
        for (element, colours) in elements.as_array_mut().unwrap().iter_mut().zip(&colours) {
            element
                .as_object_mut()
                .unwrap()
                .extend(colours.as_object().unwrap().clone());
        }
        add(&mut drawn, elements);

        let mut changed = board();
        let ids = add(&mut changed, bare);
        let patches: Vec<Value> = ids
            .iter()
            .zip(&colours)
            .map(|(id, colours)| {
                let mut patch = colours.clone();
                patch["id"] = json!(id);
                patch
            })
            .collect();
        ask(&mut changed, json!({"op": "update", "elements": patches}));

        let (left, right) = (drawn.doc().unwrap(), changed.doc().unwrap());
        for (a, b) in left.nodes().zip(right.nodes()) {
            assert_eq!(a, b, "layer {} ({}) differs", a.id, a.name);
        }
        assert!(
            *left == *right,
            "the layers are alike but the documents differ"
        );
        // And the colours are not nothing: the connector's line is its stroke, not its background.
        assert_eq!(paints(&changed, &ids[4]).1.as_deref(), Some("#9C36B5"));
        assert_eq!(paints(&changed, &ids[1]).0, None);
    }

    #[test]
    fn update_places_layers_in_page_coordinates() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "frame", "id": "area", "x": 100, "y": 100, "width": 400, "height": 300},
                {"type": "rectangle", "id": "inner", "x": 150, "y": 160, "width": 50, "height": 50,
                 "frameId": "area"},
            ]),
        );
        let inner: NodeId = ids[1].parse().unwrap();
        let doc = board.doc().unwrap();
        assert_eq!(
            doc.node(inner).unwrap().parent,
            Some(ids[0].parse().unwrap())
        );
        // It is inside the frame, so `get` lists the frame alone.
        assert_eq!(elements(&mut board).len(), 1);

        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "inner", "x": 300, "width": 80}]}),
        );

        // Where `add` put it is where `update` puts it: the page's coordinates, not the frame's.
        let bounds = board.doc().unwrap().world_bounds(inner).unwrap();
        assert_eq!(
            [bounds.x0, bounds.y0, bounds.width(), bounds.height()],
            [300.0, 160.0, 80.0, 50.0]
        );
    }

    #[test]
    fn update_finds_layers_by_agent_id_and_by_node_id() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 0, "y": 200},
                {"type": "rectangle", "id": "c", "x": 0, "y": 400},
            ]),
        );
        let number: u64 = ids[2].parse().unwrap();

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "a", "x": 11}, {"id": ids[1], "x": 22}, {"id": number, "x": 33},
            ]}),
        );
        assert_eq!(reply, json!({"ok": true, "updated": 3, "missing": []}));
        let xs: Vec<Value> = ids
            .iter()
            .map(|id| listed(&mut board, id)["x"].clone())
            .collect();
        assert_eq!(xs, [json!(11.0), json!(22.0), json!(33.0)]);

        // An agent's id wins over a node id that looks the same (a node is called "1" too).
        add(
            &mut board,
            json!([{"type": "rectangle", "id": ids[0], "x": 0, "y": 600}]),
        );
        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": ids[0], "x": 99}]}),
        );
        assert_eq!(listed(&mut board, &ids[0])["x"], json!(11.0));
        let theirs = elements(&mut board)
            .into_iter()
            .find(|e| e["y"] == json!(600.0))
            .unwrap();
        assert_eq!(theirs["x"], json!(99.0));
    }

    #[test]
    fn update_is_one_undo_step_and_fails_whole() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 0, "y": 200},
            ]),
        );
        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "a", "x": 50, "text": "A"}, {"id": "b", "x": 60}]}),
        );
        // Two elements, three changes, one step.
        assert_eq!(board.session.undo_depth(), (2, 0));
        assert_eq!(board.session.undo_labels(1), ["Update elements"]);
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(listed(&mut board, &ids[0])["x"], json!(0.0));
        assert_eq!(listed(&mut board, &ids[1])["x"], json!(0.0));
        assert!(listed(&mut board, &ids[0]).get("text").is_none());

        // What the engine refuses fails the call whole, naming the element: the first element's
        // change is not kept.
        let before = board.doc().unwrap();
        let steps = board.session.undo_depth();
        let error = refusal(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "a", "x": 50}, {"id": "b", "backgroundColor": "not a colour"},
            ]}),
        );
        assert!(
            error.contains("elements[1]") && error.contains("b"),
            "{error}"
        );
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), steps);
    }

    #[test]
    fn update_reports_malformed_input() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "rectangle", "id": "a", "x": 0, "y": 0}]),
        );
        let before = board.doc().unwrap();
        for op in [
            json!({"op": "update"}),
            json!({"op": "update", "elements": "a"}),
            json!({"op": "update", "elements": [{"x": 5}]}),
            json!({"op": "update", "elements": [{"id": ""}]}),
            json!({"op": "update", "elements": [{"id": null, "x": 5}]}),
            json!({"op": "update", "elements": ["a"]}),
            json!({"op": "update", "elements": [{"id": "a", "x": "far"}]}),
            json!({"op": "update", "elements": [{"id": "a", "text": 5}]}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.contains("elements"), "{op}: {error}");
        }
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    }

    #[test]
    fn update_notes_text_that_a_layer_cannot_take() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "freedraw", "x": 0, "y": 0, "points": [[0, 0], [30, 40], [60, 0]]},
                {"type": "rectangle", "x": 200, "y": 0},
            ]),
        );
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": ids[0], "text": "ink can't say it", "x": 5}, {"id": ids[1], "text": "a shape can"},
            ]}),
        );
        // It is not an error: the rest of the element, and the others, are updated.
        assert_eq!(reply["updated"], json!(2));
        let notes = reply["notes"].as_array().expect("notes");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].as_str().unwrap().contains("elements[0]"),
            "{notes:?}"
        );
        assert_eq!(listed(&mut board, &ids[0])["x"], json!(5.0));
        assert_eq!(listed(&mut board, &ids[1])["text"], json!("a shape can"));
    }

    #[test]
    fn the_users_selection_survives_update_and_delete() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 300, "y": 0},
                {"type": "rectangle", "id": "c", "x": 600, "y": 0},
            ]),
        );
        let nodes: Vec<NodeId> = ids.iter().map(|id| id.parse().unwrap()).collect();
        board.session.set_selection(vec![nodes[0], nodes[2]]);

        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "b", "x": 5, "text": "B", "backgroundColor": "red"}]}),
        );
        assert_eq!(board.session.selection(), [nodes[0], nodes[2]]);

        // What is deleted leaves the selection, and what isn't deleted stays in it.
        ask(&mut board, json!({"op": "delete", "ids": ["b", "c"]}));
        assert_eq!(board.session.selection(), [nodes[0]]);
    }

    #[test]
    fn delete_removes_connector_label() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 400, "y": 0},
                {"type": "arrow", "id": "ab", "start": {"id": "a"}, "end": {"id": "b"},
                 "label": {"text": "yes"}},
            ]),
        );
        assert_eq!(labels(&board), 1);
        assert_eq!(layers_on_page(&board), 4);

        let reply = ask(&mut board, json!({"op": "delete", "ids": [ids[2]]}));

        assert_eq!(reply, json!({"ok": true, "deleted": 1, "missing": []}));
        // The label went with its connector, and the shapes it joined stay.
        assert_eq!(labels(&board), 0);
        assert_eq!(layers_on_page(&board), 2);
        let types: Vec<String> = elements(&mut board)
            .iter()
            .map(|e| e["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(types, ["rectangle", "rectangle"]);
        // One step takes it back, label and all.
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!((labels(&board), layers_on_page(&board)), (1, 4));
    }

    #[test]
    fn delete_reports_missing_and_counts_what_it_found() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 300, "y": 0},
                {"type": "rectangle", "id": "c", "x": 600, "y": 0},
            ]),
        );
        let page = board.doc().unwrap().first_page().unwrap().to_string();
        let number: u64 = ids[2].parse().unwrap();

        // By an agent's id, by a node id, twice the same layer, a layer that isn't there, and the
        // page, which is no element.
        let reply = ask(
            &mut board,
            json!({"op": "delete", "ids": ["a", ids[0], "nope", page, number, "b"]}),
        );

        assert_eq!(reply["deleted"], json!(3));
        assert_eq!(reply["missing"], json!(["nope", page]));
        assert!(elements(&mut board).is_empty());
        assert_eq!(board.doc().unwrap().pages.len(), 1);

        // Nothing found, nothing done.
        let before = board.doc().unwrap();
        let reply = ask(&mut board, json!({"op": "delete", "ids": ["x", "y"]}));
        assert_eq!(
            reply,
            json!({"ok": true, "deleted": 0, "missing": ["x", "y"]})
        );
        let reply = ask(&mut board, json!({"op": "delete", "ids": []}));
        assert_eq!(reply, json!({"ok": true, "deleted": 0, "missing": []}));
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    }

    #[test]
    fn delete_never_falls_back_to_the_selection() {
        // The engine's delete takes the selection when it is given no layers.
        let mut board = board();
        let ids = add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        board.session.set_selection(vec![ids[0].parse().unwrap()]);
        for ids in [json!([]), json!(["nope"])] {
            ask(&mut board, json!({"op": "delete", "ids": ids}));
        }
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn delete_reports_malformed_input() {
        let mut board = board();
        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        for op in [
            json!({"op": "delete"}),
            json!({"op": "delete", "ids": "a"}),
            json!({"op": "delete", "ids": ["a", {"id": "b"}]}),
            json!({"op": "delete", "ids": [null]}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.contains("ids"), "{op}: {error}");
        }
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn clear_is_one_undo_step() {
        let mut board = board();
        add(
            &mut board,
            json!([
                {"type": "sticky", "id": "s", "x": 0, "y": 0, "text": "hi"},
                {"type": "ellipse", "id": "e", "x": 400, "y": 0},
                {"type": "arrow", "start": {"id": "s"}, "end": {"id": "e"}, "label": {"text": "yes"}},
                {"type": "freedraw", "x": 0, "y": 500, "points": [[0, 0], [30, 40], [60, 0]]},
            ]),
        );
        let (layers, steps) = (layers_on_page(&board), board.session.undo_depth().0);
        assert_eq!((elements(&mut board).len(), layers), (4, 5));

        let reply = ask(&mut board, json!({"op": "clear"}));

        assert_eq!(reply, json!({"ok": true}));
        assert!(elements(&mut board).is_empty());
        // Every layer is gone, the connector's label too, in one step.
        assert_eq!(layers_on_page(&board), 0);
        assert_eq!(board.session.undo_depth(), (steps + 1, 0));
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(layers_on_page(&board), layers);
        assert_eq!(elements(&mut board).len(), 4);
        assert_eq!(board.session.undo_depth(), (steps, 1));
    }

    #[test]
    fn clear_of_an_empty_page_changes_nothing_and_only_that_page_is_cleared() {
        let mut board = board();
        let before = board.doc().unwrap();
        // The selection is empty too, which is what the engine's delete would fall back to.
        assert_eq!(ask(&mut board, json!({"op": "clear"})), json!({"ok": true}));
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));

        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        board.execute("page.add", &json!({})).unwrap();
        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        ask(&mut board, json!({"op": "clear"}));
        assert!(elements(&mut board).is_empty());
        board.execute("page.select", &json!({"index": 0})).unwrap();
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// A board file of two pages, a sticky on each.
    fn two_page_file(dir: &std::path::Path) -> String {
        let mut other = BoardDoc::new_board("Two pages").unwrap();
        other
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "first"}))
            .unwrap();
        other
            .execute("page.add", &json!({"name": "Second"}))
            .unwrap();
        other
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "second"}))
            .unwrap();
        let path = dir.join("two.sawit");
        std::fs::write(&path, other.to_bytes().unwrap()).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// The board of [`other_board_file`] as a `.sawit` file in `dir`: its path.
    fn other_sawit_file(dir: &std::path::Path) -> String {
        let path = dir.join("other.sawit");
        std::fs::copy(other_board_file(dir), &path).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// A canvas file by the extension it had before loads as it always did, the engine reading it
    /// where it is; a `.sawit` one is handed over by its bytes, named as the engine knows a canvas
    /// file, which it would otherwise place as an image.
    #[test]
    fn replace_loads_a_canvas_file_by_either_extension() {
        let dir = tempfile::tempdir().unwrap();
        for path in [other_board_file(dir.path()), other_sawit_file(dir.path())] {
            let mut board = board();
            let reply = ask(&mut board, json!({"op": "replace", "path": path}));
            let now = elements(&mut board);
            assert_eq!(now.len(), 2, "{path}: {now:?}");
            assert!(now.iter().all(|e| e["text"] == json!("other")), "{now:?}");
            assert_eq!(reply["elements"], json!(now));
        }
        assert_eq!(engine_name("other.sawit"), "other.figcraft");
        assert_eq!(engine_name("Other.SAWIT"), "Other.figcraft");
        assert_eq!(
            engine_name("canvas.sawit.bak.2"),
            "canvas.sawit.bak.2.figcraft"
        );
        assert_eq!(
            engine_name("canvas.figcraft.bak"),
            "canvas.figcraft.bak.figcraft"
        );
        for name in ["other.figcraft", "page.png", "logo.svg", "notes.sawitx"] {
            assert_eq!(engine_name(name), name);
        }
    }

    /// `file.import` run as a command reads a `.sawit` file it names as a canvas file, as the
    /// steps of a batch do: its pages come in after the canvas's own, where the engine alone would
    /// have placed the file as an image. A `.sawit` file that isn't there is the error.
    #[test]
    fn file_import_of_a_sawit_file_brings_its_pages_in() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_sawit_file(dir.path());
        let mut board = board();
        let reply = send(&mut board, "file.import", json!({"path": other}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(board.doc().unwrap().pages.len(), 2, "{reply}");

        let batch = json!({"commands": [{"command": "file.import", "params": {"path": other}}]});
        let reply = send(&mut board, "command.batch", batch);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(board.doc().unwrap().pages.len(), 3, "{reply}");

        let missing = dir.path().join("missing.sawit");
        let reply = send(&mut board, "file.import", json!({"path": missing}));
        let error = reply["error"].as_str().unwrap_or_default();
        assert!(error.starts_with("file.import: couldn't read"), "{reply}");
        assert_eq!(board.doc().unwrap().pages.len(), 3);
    }

    #[test]
    fn replace_loads_a_sawit_file_as_one_undo_step() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_sawit_file(dir.path());
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        // Two pages of its own: both are replaced.
        board
            .execute("page.add", &json!({"name": "Notes"}))
            .unwrap();
        board.execute("page.select", &json!({"index": 0})).unwrap();
        let (uid, steps) = (board.session.ds().unwrap().uid, board.session.undo_depth());

        let reply = ask(&mut board, json!({"op": "replace", "path": other}));

        // The canvas shows the file's page, and says what is on it as `get` does.
        let now = elements(&mut board);
        assert_eq!(now.len(), 2, "{now:?}");
        assert!(now.iter().all(|e| e["text"] == json!("other")), "{now:?}");
        assert_eq!(reply["elements"], json!(now));
        assert_eq!(board.doc().unwrap().pages.len(), 1);
        assert!(board.session.selection().is_empty());
        // In the canvas's own session: one document, the one it had, and still a board.
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(board.session.ds().unwrap().uid, uid);
        assert_eq!(board.kind(), DocKind::Board);
        // One step: undo brings the old board back, pages and all, and redo the file's.
        assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
        assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(board.doc().unwrap().pages.len(), 2);
        let back = elements(&mut board);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0]["text"], json!("mine"));
        ask(&mut board, json!({"op": "command", "command": "edit.redo"}));
        assert_eq!(elements(&mut board).len(), 2);
    }

    #[test]
    fn replace_takes_every_page_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = two_page_file(dir.path());
        let mut board = board();
        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));

        ask(&mut board, json!({"op": "replace", "path": path}));

        // The first page is on show, and the old page is gone.
        let doc = board.doc().unwrap();
        let names: Vec<&str> = doc
            .pages
            .iter()
            .map(|page| doc.node(*page).unwrap().name.as_str())
            .collect();
        assert_eq!(names, ["Page 1", "Second"]);
        let now = elements(&mut board);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0]["text"], json!("first"));
    }

    #[test]
    fn replace_keeps_a_design_file_a_board() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = design();
        file.execute(
            "node.create",
            &json!({"type": "frame", "x": 0, "y": 0, "name": "Screen"}),
        )
        .unwrap();
        let path = dir.path().join("design.sawit");
        std::fs::write(&path, file.to_bytes().unwrap()).unwrap();
        let mut board = board();

        let reply = ask(&mut board, json!({"op": "replace", "path": path}));

        assert_eq!(board.kind(), DocKind::Board);
        assert_eq!(reply["elements"][0]["name"], json!("Screen"));
        assert_eq!(reply["elements"][0]["type"], json!("frame"));
    }

    #[test]
    fn replace_of_a_missing_or_bad_file_is_an_error_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.sawit");
        std::fs::write(&bad, b"not a canvas file").unwrap();
        let missing = dir.path().join("missing.sawit");
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let (before, steps) = (board.doc().unwrap(), board.session.undo_depth());

        let error = refusal(&mut board, json!({"op": "replace", "path": missing}));
        assert!(
            error.starts_with("replace:") && error.contains("missing.sawit"),
            "{error}"
        );
        let error = refusal(&mut board, json!({"op": "replace", "path": bad}));
        assert!(error.starts_with("replace:"), "{error}");

        // Not a new, empty board over the canvas: it is as it was.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), steps);
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// Runs `op` on a board with a sticky, on a thread of its own
    /// ([`crate::store::within_seconds`]), and gives its reply, and whether the board's document is
    /// still the one it had: `None` when it hadn't answered within 5 s.
    fn on_a_board_of_its_own(op: Value) -> Option<(Value, bool)> {
        crate::store::within_seconds(move || {
            let mut board = board();
            send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
            let before = board.doc().unwrap();
            let reply = run(&mut board, &op);
            (reply, Arc::ptr_eq(&before, &board.doc().unwrap()))
        })
    }

    /// CANVAS-CORE-6, for an agent's load (`canvas_load`): a `replace` of a canvas file has
    /// the engine read the file `path` names, whole and as it finds it, on the canvas's UI thread
    /// (with the app closed, in sawit-mcp, holding every other canvas call back). A named pipe
    /// blocked it for ever, and a link a cloned repository holds to `/dev/zero` read until the Mac
    /// ran out of memory. Both are refused at once, as when a canvas opens such a file, and the
    /// canvas is as it was; an `.excalidraw` file is, as before.
    #[test]
    fn replace_of_a_pipe_or_a_device_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("board.sawit");
        crate::store::pipe_at(&pipe);
        // A canvas file by the extension it had before, which the engine reads where it is.
        let before = dir.path().join("board.figcraft");
        crate::store::pipe_at(&before);
        let scene = dir.path().join("scene.excalidraw");
        crate::store::pipe_at(&scene);
        let refused_at_once = |paths: &[&std::path::Path]| {
            let failed: Vec<String> = (paths.iter())
                .filter_map(|path| {
                    let shown = path.display().to_string();
                    let replaced = on_a_board_of_its_own(json!({"op": "replace", "path": path}));
                    let Some((reply, unchanged)) = replaced else {
                        return Some(format!("{shown}: no answer within 5 s"));
                    };
                    let error = reply["error"].as_str().unwrap_or_default();
                    let refused = error.starts_with(&format!("replace: couldn't read {shown}"))
                        && error.contains("not a regular file");
                    (!refused || !unchanged).then(|| format!("{shown}: {reply}"))
                })
                .collect();
            assert!(failed.is_empty(), "{}", failed.join("\n"));
        };
        // The pipes first: a device that never ends is read only once they are refused.
        refused_at_once(&[&pipe, &before, &scene]);
        let zero = dir.path().join("zero.sawit");
        std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
        refused_at_once(&[&zero]);
    }

    /// The scene of the Excalidraw fixture: nine layers on the page, and an embed it leaves out.
    fn scene() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/excalidraw/basic.excalidraw"
        ))
        .unwrap()
    }

    #[test]
    fn replace_loads_an_excalidraw_scene_as_one_undo_step() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        // Two pages of its own: both are replaced.
        board
            .execute("page.add", &json!({"name": "Notes"}))
            .unwrap();
        board.execute("page.select", &json!({"index": 0})).unwrap();
        let (uid, steps) = (board.session.ds().unwrap().uid, board.session.undo_depth());

        let reply = ask(&mut board, json!({"op": "replace", "scene": scene()}));

        // The canvas shows the scene, and says what is on it as `get` does, and what was left out.
        let now = elements(&mut board);
        assert_eq!(now.len(), 9, "{now:?}");
        assert_eq!(reply["elements"], json!(now));
        assert_eq!(reply["notes"], json!(["skipped 1 embed"]));
        assert_eq!(board.doc().unwrap().pages.len(), 1);
        assert!(board.session.selection().is_empty());
        // In the canvas's own session: one document, the one it had, and still a board.
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(board.session.ds().unwrap().uid, uid);
        assert_eq!(board.kind(), DocKind::Board);
        // One step: undo brings the old board back, pages and all, and redo the scene.
        assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
        assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(board.doc().unwrap().pages.len(), 2);
        let back = elements(&mut board);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0]["text"], json!("mine"));
        ask(&mut board, json!({"op": "command", "command": "edit.redo"}));
        assert_eq!(elements(&mut board).len(), 9);
    }

    #[test]
    fn a_replaced_scene_keeps_what_it_joins_and_the_ids_it_had() {
        let mut board = board();
        let reply = ask(&mut board, json!({"op": "replace", "scene": scene()}));
        let now = reply["elements"].as_array().unwrap();
        let of = |kind: &str| now.iter().find(|e| e["type"] == json!(kind)).unwrap();
        // The arrow still joins the rectangle and the ellipse, which are in the new canvas.
        let arrow = now.iter().find(|e| e.get("from").is_some()).unwrap();
        assert_eq!(arrow["from"], of("rectangle")["id"]);
        assert_eq!(arrow["to"], of("ellipse")["id"]);
        // The picture and the section came through the engine's import.
        assert_eq!(of("image")["width"], json!(120.0));
        assert_eq!(of("section")["name"], json!("Group A"));
        let doc = board.doc().unwrap();
        let section = doc
            .node(of("section")["id"].as_str().unwrap().parse().unwrap())
            .unwrap();
        assert_eq!(section.children.len(), 1);
        // The ids the elements had in Excalidraw name them still.
        let rectangle = of("rectangle")["id"].as_str().unwrap().to_string();
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "box", "text": "Renamed"}]}),
        );
        assert_eq!(reply["updated"], json!(1), "{reply}");
        assert_eq!(listed(&mut board, &rectangle)["text"], json!("Renamed"));
    }

    #[test]
    fn replace_loads_an_excalidraw_file() {
        let dir = tempfile::tempdir().unwrap();
        let text = include_str!("../tests/fixtures/excalidraw/basic.excalidraw");
        for name in ["board.excalidraw", "BOARD.Excalidraw"] {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            let mut board = board();
            add(
                &mut board,
                json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
            );

            let reply = ask(&mut board, json!({"op": "replace", "path": path}));

            assert_eq!(reply["elements"].as_array().unwrap().len(), 9, "{name}");
            assert_eq!(reply["notes"], json!(["skipped 1 embed"]));
            assert_eq!(elements(&mut board).len(), 9);
            assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
            // The file is read, never written.
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn a_scene_wins_over_a_path_and_an_empty_one_clears_the_canvas() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let reply = ask(
            &mut board,
            json!({"op": "replace", "scene": {"elements": []}, "path": "/nowhere/a.sawit"}),
        );
        assert_eq!(reply, json!({"ok": true, "elements": []}));
        assert!(elements(&mut board).is_empty());
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(elements(&mut board).len(), 1);
        // `null` is no scene: the path is what counts.
        let error = refusal(
            &mut board,
            json!({"op": "replace", "scene": null, "path": "/nowhere/a.sawit"}),
        );
        assert!(error.contains("a.sawit"), "{error}");
    }

    #[test]
    fn replace_of_a_bad_scene_or_excalidraw_file_is_an_error_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            path.to_string_lossy().into_owned()
        };
        let not_json = write("not-json.excalidraw", "{ not json");
        let no_elements = write("no-elements.excalidraw", r#"{"type": "excalidraw"}"#);
        // A file with nothing in it opens blank as the project canvas (the migration), but nobody
        // loads nothing on purpose: `replace` refuses it, as `canvas_load` always has.
        let empty = write("empty.excalidraw", "");
        let spaces = write("spaces.excalidraw", " \n");
        let bom = write("bom.excalidraw", "\u{feff}");
        let missing = dir.path().join("missing.excalidraw");
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let (before, steps) = (board.doc().unwrap(), board.session.undo_depth());

        for (op, says) in [
            (json!({"op": "replace", "scene": {}}), "`elements`"),
            (json!({"op": "replace", "scene": [1, 2]}), "`elements`"),
            (json!({"op": "replace", "scene": "scene"}), "`elements`"),
            (
                json!({"op": "replace", "path": missing}),
                "missing.excalidraw",
            ),
            (
                json!({"op": "replace", "path": not_json}),
                "isn't valid JSON",
            ),
            (json!({"op": "replace", "path": no_elements}), "`elements`"),
            (json!({"op": "replace", "path": empty}), "isn't valid JSON"),
            (json!({"op": "replace", "path": spaces}), "isn't valid JSON"),
            (json!({"op": "replace", "path": bom}), "isn't valid JSON"),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.starts_with("replace: "), "{op}: {error}");
            assert!(error.contains(says), "{op}: {error}");
        }
        // The file that is no scene is named, not only said to be one.
        let error = refusal(&mut board, json!({"op": "replace", "path": no_elements}));
        assert!(error.contains("no-elements.excalidraw"), "{error}");

        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), steps);
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn replace_needs_a_scene_or_a_file_of_a_kind_it_knows() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let before = board.doc().unwrap();
        // No scene and no path, or a path that isn't text, runs nothing.
        for op in [
            json!({"op": "replace"}),
            json!({"op": "replace", "path": 5}),
            json!({"op": "replace", "scene": null}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.starts_with("replace needs"), "{op}: {error}");
        }
        for path in ["/tmp/board.png", "/tmp/board.bak", "/tmp/board"] {
            let error = refusal(&mut board, json!({"op": "replace", "path": path}));
            let expected = format!("replace: {path} is not a .sawit or .excalidraw file");
            assert_eq!(error, expected);
        }
        // A copy kept of a canvas file that isn't there says so.
        let missing = "/nowhere/canvas.sawit.bak";
        let error = refusal(&mut board, json!({"op": "replace", "path": missing}));
        assert!(
            error.starts_with(&format!("replace: couldn't read {missing}: ")),
            "{error}"
        );
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (1, 0));
    }

    /// MCP-6: the copies Sawit keeps of a canvas file (`canvas.sawit.bak` and the older
    /// `.bak.<n>`, which a clear or a load keeps of the board it takes off, `.del.bak` and the
    /// older `.del.bak.<n>`, which an agent's deletion keeps, and the `.lossy.bak` of a file
    /// opened with notes) are canvas files under another name, and load as the file they were:
    /// that is how an agent brings back a board it cleared. Refused by its name, a backup could
    /// only be loaded once someone copied it to a `.sawit` name; and passed on by its path,
    /// the engine, which tells a file's kind by its name, would place it as an image.
    #[test]
    fn replace_loads_a_copy_kept_of_a_canvas_file() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        for name in [
            "canvas.sawit.bak",
            "canvas.sawit.bak.2",
            "canvas.sawit.del.bak",
            "canvas.sawit.del.bak.3",
            "canvas.sawit.lossy.bak",
            "Board.SAWIT.BAK",
            // Those kept of a canvas file by the extension it had before.
            "canvas.figcraft.bak",
            "canvas.figcraft.del.bak.1",
        ] {
            let kept = dir.path().join(name);
            std::fs::copy(&other, &kept).unwrap();
            let mut board = board();
            add(
                &mut board,
                json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
            );
            let reply = ask(&mut board, json!({"op": "replace", "path": kept}));
            let now = elements(&mut board);
            assert_eq!(now.len(), 2, "{name}: {now:?}");
            assert!(
                now.iter().all(|e| e["text"] == json!("other")),
                "{name}: {now:?}"
            );
            assert_eq!(reply["elements"], json!(now), "{name}");
            assert_eq!(board.kind(), DocKind::Board);
        }
        // Read as a canvas file is checked: a named pipe at such a name is refused, unread.
        let pipe = dir.path().join("pipe.sawit.bak");
        let made = std::process::Command::new("mkfifo").arg(&pipe).status();
        assert!(made.unwrap().success(), "mkfifo {}", pipe.display());
        let error = refusal(&mut board(), json!({"op": "replace", "path": pipe}));
        assert!(error.contains("not a regular file"), "{error}");
    }

    /// Every id `commands` lists, with the whole reply checked.
    fn listed_commands(board: &mut BoardDoc) -> Vec<Value> {
        let reply = ask(board, json!({"op": "commands"}));
        reply["commands"].as_array().expect("commands").clone()
    }

    #[test]
    fn commands_hide_hidden_ids() {
        let mut board = board();
        let commands = listed_commands(&mut board);
        let ids: Vec<&str> = commands.iter().map(|c| c["id"].as_str().unwrap()).collect();

        // Nothing the canvas hides: no file.new, nor any other hidden id, nor a plugin's.
        assert!(!ids.contains(&"file.new"));
        assert!(!ids.contains(&"plugins.manage"));
        for id in &ids {
            assert!(!policy::is_hidden(id), "{id} is hidden");
        }
        // Saving is Sawit's: `command` refuses it, so it isn't offered (P7-C11).
        assert!(!ids.contains(&"file.save"));
        assert!(!ids.contains(&"file.saveAs"));

        // What an agent can do is there, as {id, label, params}.
        let sticky = commands
            .iter()
            .find(|c| c["id"] == json!("board.sticky"))
            .unwrap();
        assert_eq!(sticky["label"], json!("Sticky note"));
        assert!(
            sticky["params"].as_str().unwrap().contains("text?"),
            "{sticky}"
        );
        for command in &commands {
            let keys: Vec<&String> = command.as_object().unwrap().keys().collect();
            assert_eq!(keys, ["id", "label", "params"], "{command}");
            assert!(command.as_object().unwrap().values().all(Value::is_string));
        }
        assert!(ids.len() > 200, "{} commands", ids.len());
    }

    #[test]
    fn commands_lists_only_what_the_command_op_runs() {
        let mut board = board();
        let commands = listed_commands(&mut board);
        let ids: Vec<&str> = commands.iter().map(|c| c["id"].as_str().unwrap()).collect();

        // The one rule: nothing is offered that `command` refuses, up front or by the guard that
        // undoes a command that opens a document.
        for id in &ids {
            assert_eq!(
                refused(id, &Value::Null),
                None,
                "{id} is listed but refused"
            );
            assert!(!OPENS_DOCUMENTS.contains(id), "{id} opens a document");
        }
        for id in HANDLED_BY_SAWIT.iter().chain(OPENS_DOCUMENTS) {
            assert!(!ids.contains(id), "{id} is listed");
        }
        // Every other command of the engine is, except what the policy hides.
        let engine = figcraft_engine::cmd::all_commands();
        let unlisted: Vec<&str> = engine
            .iter()
            .map(|c| c.id)
            .filter(|id| !ids.contains(id))
            .collect();
        for id in &unlisted {
            assert!(
                policy::is_hidden(id)
                    || HANDLED_BY_SAWIT.contains(id)
                    || OPENS_DOCUMENTS.contains(id),
                "{id} is not listed, and nothing says why"
            );
        }
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "an id is listed twice");
    }

    #[test]
    fn commands_that_open_documents_are_real_engine_ids() {
        // A rename in the engine would leave a stale id here, and the command in the list.
        for id in OPENS_DOCUMENTS {
            assert!(figcraft_engine::cmd::find_command(id).is_some(), "{id}");
        }
    }

    #[test]
    fn without_a_document_ops_answer_with_an_error() {
        let mut board = BoardDoc {
            session: figcraft_engine::Session::empty(),
        };
        for op in [
            json!({"op": "get"}),
            json!({"op": "add", "elements": []}),
            json!({"op": "update", "elements": []}),
            json!({"op": "delete", "ids": []}),
            json!({"op": "clear"}),
            json!({"op": "replace", "path": "/tmp/canvas.sawit"}),
            json!({"op": "replace", "scene": {"elements": []}}),
            json!({"op": "export_svg"}),
            json!({"op": "export_png"}),
            json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}}),
            json!({"op": "pointer", "kind": "down", "x": 0, "y": 0}),
            json!({"op": "type", "text": "hi"}),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            assert!(reply["error"].as_str().is_some_and(|e| !e.is_empty()));
        }
    }

    #[test]
    fn unknown_op_is_reported() {
        let mut board = board();
        let reply = run(&mut board, &json!({"op": "teleport", "to": [0, 0]}));
        assert_eq!(
            reply,
            json!({"ok": false, "error": "teleport is not supported yet"})
        );
        // An op with no name, or that isn't an op at all.
        for op in [json!({}), json!("get"), json!({"op": 3}), json!(null)] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            assert!(reply["error"].as_str().is_some_and(|e| e.contains("op")));
        }
    }

    /// Sends pointer op `kind` at page point (`x`, `y`), checks that it answered, and returns the
    /// answer.
    fn pointer_at(board: &mut BoardDoc, kind: &str, x: f64, y: f64) -> Value {
        let reply = run(
            board,
            &json!({"op": "pointer", "kind": kind, "x": x, "y": y}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        reply
    }

    #[test]
    fn pointer_op_drives_tools() {
        let mut board = board();
        let reply = send(&mut board, "tool.sticky", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        pointer_at(&mut board, "down", 0.0, 0.0);
        let reply = pointer_at(&mut board, "up", 0.0, 0.0);
        assert_eq!(elements(&mut board).len(), 1);
        // The answer says which tool is on, back to Move once the sticky is placed, and what is
        // selected, by id.
        assert_eq!(reply["tool"], json!("move"));
        let selection = reply["selection"].as_array().expect("a selection");
        assert_eq!(selection.len(), 1, "{reply}");
        assert!(
            selection[0]
                .as_str()
                .is_some_and(|id| id.parse::<u64>().is_ok()),
            "{reply}"
        );
    }

    #[test]
    fn pointer_op_takes_page_points_whatever_the_view() {
        let mut board = design();
        // Zoomed in and panned: the op's points are page points all the same.
        board.session.ds_mut().unwrap().view = figcraft_engine::ViewState {
            cx: 37.0,
            cy: -12.0,
            zoom: 2.0,
            vw: 640.0,
            vh: 480.0,
            ..figcraft_engine::ViewState::default()
        };
        send(&mut board, "tool.rectangle", json!({}));
        for (kind, x, y) in [
            ("down", 10.0, 20.0),
            ("move", 60.0, 60.0),
            ("move", 110.0, 100.0),
            ("up", 110.0, 100.0),
        ] {
            pointer_at(&mut board, kind, x, y);
        }
        let elements = elements(&mut board);
        assert_eq!(elements.len(), 1, "{elements:?}");
        let drawn = &elements[0];
        let bounds = ["x", "y", "width", "height"].map(|key| drawn[key].as_f64().unwrap());
        assert_eq!(bounds, [10.0, 20.0, 100.0, 80.0], "{drawn}");
    }

    #[test]
    fn pointer_op_leaves_the_views_requests_for_it() {
        let mut board = design();
        let made = board
            .execute(
                "node.create",
                &json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100}),
            )
            .unwrap();
        let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
        board.execute("select.none", &json!({})).unwrap();
        // The view hands the engine its frames' names as it paints them: this one just above the
        // frame's top-left corner.
        let corner = figcraft_geom::Point::new;
        board.session.tool.labels = vec![(
            frame,
            [
                corner(0.0, -20.0),
                corner(60.0, -20.0),
                corner(60.0, -4.0),
                corner(0.0, -4.0),
            ],
        )];

        let reply = pointer_at(&mut board, "double", 10.0, -12.0);

        // Renaming the frame there is the view's to do: the request waits for it.
        let kinds: Vec<&str> = (board.session.ui_requests.iter())
            .map(|request| request.kind.as_str())
            .collect();
        assert_eq!(kinds, ["canvas.renameLabel"]);
        assert_eq!(reply["selection"], json!([frame.to_string()]));
    }

    #[test]
    fn malformed_pointer_ops_are_reported() {
        let mut board = board();
        send(&mut board, "tool.sticky", json!({}));
        for (op, says) in [
            (json!({"op": "pointer", "x": 0, "y": 0}), "kind"),
            (json!({"op": "pointer", "kind": 1, "x": 0, "y": 0}), "kind"),
            (
                json!({"op": "pointer", "kind": "tap", "x": 0, "y": 0}),
                "tap",
            ),
            (json!({"op": "pointer", "kind": "down"}), "x and y"),
            (
                json!({"op": "pointer", "kind": "down", "x": "0", "y": 0}),
                "x and y",
            ),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains(says), "{op}: {error}");
        }
        // None of them reached the tools: no sticky was placed, and the tool is still on.
        assert!(elements(&mut board).is_empty());
        assert_eq!(board.session.tool.tool.id(), "sticky");
    }

    /// Sends key op `key` with the modifier keys in `held` (`{"cmd": true}`), checks that it
    /// answered, and returns whether the canvas took the key.
    fn press(board: &mut BoardDoc, key: &str, held: Value) -> bool {
        let mut op = json!({"op": "key", "key": key});
        for (modifier, on) in held.as_object().expect("modifier keys") {
            op[modifier] = on.clone();
        }
        let reply = run(board, &op);
        assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
        reply["handled"].as_bool().expect("handled")
    }

    #[test]
    fn key_op_routes_like_the_keyboard() {
        let mut board = board();
        let reply = run(&mut board, &json!({"op": "key", "key": "r"}));
        assert_eq!(reply, json!({"ok": true, "handled": true}));
        assert_eq!(board.session.tool.tool.id(), "rectangle");
        // With the modifier keys held: ⌘Z takes back the sticky.
        send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert!(press(&mut board, "z", json!({"cmd": true})));
        assert!(elements(&mut board).is_empty());
        // A key that is no shortcut, and a hidden command's (⌘N is file.new), aren't the canvas's.
        assert!(!press(&mut board, "g", json!({})));
        assert!(!press(&mut board, "n", json!({"cmd": true})));
        assert_eq!(board.session.docs.len(), 1);
        // The shortcut of a command Sawit carries out itself is the canvas's, though the op runs
        // nothing for it (⌘S, the save).
        assert!(press(&mut board, "s", json!({"cmd": true})));
    }

    /// The op presses the canvas's default keys as Sawit binds them, not FigCraft's raw keymap:
    /// the keys Sawit keeps (⌘J toggles the terminal, ⌥⌘O adds a project) and the commands the
    /// canvas doesn't offer (`/`, a live session's cursor chat) aren't the canvas's, as on the
    /// keyboard, while its own keys still are.
    #[test]
    fn key_op_presses_the_canvas_default_keys() {
        let mut board = board();
        let made = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
        board.session.set_selection(vec![note]);
        let before = board.doc().unwrap();
        for (key, held) in [
            ("j", json!({"cmd": true})),
            ("o", json!({"alt": true, "cmd": true})),
            ("/", json!({})),
        ] {
            let mut op = json!({"op": "key", "key": key});
            for (modifier, on) in held.as_object().unwrap() {
                op[modifier] = on.clone();
            }
            let reply = run(&mut board, &op);
            assert_eq!(reply, json!({"ok": true, "handled": false}), "{op}");
            assert!(board.session.ui_requests.is_empty(), "{op}");
        }
        assert_eq!(board.doc().unwrap(), before);
        assert!(press(&mut board, "r", json!({})));
        assert_eq!(board.session.tool.tool.id(), "rectangle");
    }

    #[test]
    fn key_op_leaves_the_views_requests_for_it() {
        let mut board = board();
        // ⌘K asks the view for the Actions palette: the request waits for it.
        assert!(press(&mut board, "k", json!({"cmd": true})));
        let kinds: Vec<&str> = (board.session.ui_requests.iter())
            .map(|request| request.kind.as_str())
            .collect();
        assert_eq!(kinds, ["palette.actions"]);
        board.session.ui_requests.clear();

        // ⇧⌘↩ while typing in a sticky makes the next one, and typing goes on there, as from the
        // keyboard: the op does what the engine asked of the view that needs no view.
        let made = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let first = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
        assert!(figcraft_engine::board::edit_text(&mut board.session, first).unwrap());
        assert!(press(
            &mut board,
            "Enter",
            json!({"shift": true, "cmd": true})
        ));
        assert!(board.session.ui_requests.is_empty());
        let ds = board.session.ds().unwrap();
        let layers = &ds.doc.node(ds.page).unwrap().children;
        let [one, next] = layers[..] else {
            panic!("layers {layers:?}")
        };
        assert_eq!(one, first);
        let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
        assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
    }

    #[test]
    fn malformed_key_ops_and_refusals_are_reported() {
        let mut board = board();
        for op in [
            json!({"op": "key"}),
            json!({"op": "key", "key": 5}),
            json!({"op": "key", "key": ""}),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("a key op needs a key"), "{op}: {error}");
        }
        // A shortcut the engine refuses: ⌘G groups the selection, and nothing is selected.
        let reply = run(&mut board, &json!({"op": "key", "key": "g", "cmd": true}));
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("nothing is selected"), "{error}");
    }

    /// Puts a new, empty sticky on the board and starts typing in it, and returns the sticky. The
    /// edit is of the sticky's text, a layer of its own (ruling P2-C6).
    fn typing_in_a_new_sticky(board: &mut BoardDoc) -> NodeId {
        let made = send(board, "board.sticky", json!({"x": 0, "y": 0}));
        let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
        let text = figcraft_engine::board::text_of(&board.doc().unwrap(), note).unwrap();
        textedit::begin(&mut board.session, text, None, false).unwrap();
        note
    }

    /// What sticky `id` reads in the document.
    fn sticky_text(board: &BoardDoc, id: NodeId) -> String {
        let doc = board.doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, id).unwrap();
        let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
        text.characters.clone()
    }

    fn editing(board: &BoardDoc) -> bool {
        board.session.ds().unwrap().text_edit.is_some()
    }

    /// What an input method composes in the text being edited: shown there, not in the document.
    fn composing(board: &BoardDoc) -> Option<String> {
        let ds = board.session.ds().unwrap();
        let preedit = ds.text_edit.as_ref()?.preedit.as_ref()?;
        Some(preedit.text.clone())
    }

    #[test]
    fn typing_into_a_new_sticky() {
        let mut board = board();
        let note = typing_in_a_new_sticky(&mut board);
        textedit::insert(&mut board.session, "hi").unwrap();
        textedit::end(&mut board.session).unwrap();
        assert!(!editing(&board));
        assert_eq!(sticky_text(&board, note), "hi");
    }

    #[test]
    fn ime_commit_replaces_preedit() {
        let mut board = board();
        let note = typing_in_a_new_sticky(&mut board);
        textedit::preedit(&mut board.session, "ni", None).unwrap();
        // Composed, it shows; the document gets it only once it is committed.
        assert_eq!(composing(&board).as_deref(), Some("ni"));
        assert_eq!(sticky_text(&board, note), "");
        textedit::insert_ime_commit(&mut board.session, "你").unwrap();
        assert_eq!(sticky_text(&board, note), "你");
        assert_eq!(composing(&board), None);
        assert!(editing(&board));
    }

    /// Review Focus 3, at the engine: Escape while composing ends the edit, and the composition
    /// goes with it.
    #[test]
    fn cancelled_composition_leaves_no_text() {
        let mut board = board();
        let note = typing_in_a_new_sticky(&mut board);
        textedit::preedit(&mut board.session, "ni", None).unwrap();
        let escape = input::route_key(&mut board, "Escape", Mods::default());
        assert_eq!(escape, Ok(KeyOutcome::Handled));
        assert!(!editing(&board));
        assert_eq!(sticky_text(&board, note), "");
        // The sticky stays, emptied, as an emptied sticky does.
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// Ruling P2-C18.
    #[test]
    fn type_op_types_text() {
        let mut board = board();
        // With no text edited there is nothing to type into.
        let reply = run(&mut board, &json!({"op": "type", "text": "hi"}));
        assert_eq!(reply, json!({"ok": false, "error": "not editing text"}));
        assert!(elements(&mut board).is_empty());

        // While a new sticky is edited, the text goes into it, and the edit goes on.
        let note = typing_in_a_new_sticky(&mut board);
        let reply = run(&mut board, &json!({"op": "type", "text": "hi"}));
        assert_eq!(reply, json!({"ok": true}));
        assert_eq!(sticky_text(&board, note), "hi");
        assert!(editing(&board));
        // On at the caret, as the keyboard types.
        let reply = run(&mut board, &json!({"op": "type", "text": " there"}));
        assert_eq!(reply, json!({"ok": true}));
        assert_eq!(sticky_text(&board, note), "hi there");
    }

    #[test]
    fn type_op_follows_the_pointer_and_key_ops() {
        // An agent's sticky, as a mouse and a keyboard make one: the sticky tool's click starts
        // typing in the new sticky, the text goes in, and Escape ends the typing.
        let mut board = board();
        send(&mut board, "tool.sticky", json!({}));
        pointer_at(&mut board, "down", 0.0, 0.0);
        pointer_at(&mut board, "up", 0.0, 0.0);
        let reply = run(
            &mut board,
            &json!({"op": "type", "text": "Hello from Sawit"}),
        );
        assert_eq!(reply, json!({"ok": true}));
        assert!(press(&mut board, "Escape", json!({})));
        let elements = elements(&mut board);
        assert_eq!(elements.len(), 1, "{elements:?}");
        assert_eq!(elements[0]["text"], json!("Hello from Sawit"));
        let reply = run(&mut board, &json!({"op": "type", "text": "more"}));
        assert_eq!(reply, json!({"ok": false, "error": "not editing text"}));
    }

    #[test]
    fn malformed_type_ops_are_reported() {
        let mut board = board();
        let note = typing_in_a_new_sticky(&mut board);
        for op in [json!({"op": "type"}), json!({"op": "type", "text": 5})] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("a type op needs the text"), "{op}: {error}");
        }
        // Nothing was typed, and the edit goes on.
        assert_eq!(sticky_text(&board, note), "");
        assert!(editing(&board));
    }

    /// Whether the engine's gesture is open: typing holds one, "Edit text", until the edit ends.
    fn gesture_open(board: &BoardDoc) -> bool {
        board.session.ds().unwrap().interaction.is_some()
    }

    /// A new board with a sticky typed in, as a person types: "hi", the edit still open. The board
    /// and the sticky.
    fn typing_hi() -> (BoardDoc, NodeId) {
        let mut board = board();
        let note = typing_in_a_new_sticky(&mut board);
        textedit::insert(&mut board.session, "hi").unwrap();
        (board, note)
    }

    /// Plan 7 final review I-2 (ruling R-TEXTEDIT): an op that changes the document ends the text
    /// the user is typing in first, as a click elsewhere does. What was typed is an undo step of
    /// its own, and the op is one after it, which undo takes back alone. Inside the open edit an
    /// op joined the typing's step, so the user's ⌘Z took both; and an op that dropped the edit
    /// (Select none, deleting the text being typed, a replace) left its gesture open, so the
    /// user's next click on a layer cancelled it, and the typing and every op since were gone.
    #[test]
    fn an_op_that_changes_the_document_ends_the_typing_first() {
        let (mut typed, note) = typing_hi();
        let steps = typed.session.undo_depth().0;
        let rectangle = json!([{"type": "rectangle", "x": 400, "y": 0}]);
        ask(&mut typed, json!({"op": "add", "elements": rectangle}));
        assert!(!editing(&typed) && !gesture_open(&typed));
        assert_eq!(typed.session.undo_depth().0, steps + 2);
        send(&mut typed, "edit.undo", json!({}));
        assert_eq!(elements(&mut typed).len(), 1);
        assert_eq!(sticky_text(&typed, note), "hi");

        // Each of the others: then undo takes back the op, and what was typed is there.
        let ops: [fn(NodeId) -> Value; 6] = [
            |note| json!({"op": "update", "elements": [{"id": note, "x": 50}]}),
            |note| json!({"op": "delete", "ids": [note]}),
            |_| json!({"op": "clear"}),
            |_| json!({"op": "replace", "scene": scene()}),
            |_| json!({"op": "mermaid", "source": "graph TD; A-->B"}),
            |_| json!({"op": "command", "command": "page.add"}),
        ];
        for op in ops {
            let (mut typed, note) = typing_hi();
            let op = op(note);
            ask(&mut typed, op.clone());
            assert!(!editing(&typed) && !gesture_open(&typed), "{op}");
            send(&mut typed, "edit.undo", json!({}));
            assert_eq!(sticky_text(&typed, note), "hi", "{op}");
        }

        // A selection change too, which is no undo step.
        let (mut typed, note) = typing_hi();
        send(&mut typed, "select.none", json!({}));
        assert!(!editing(&typed) && !gesture_open(&typed));
        assert_eq!(sticky_text(&typed, note), "hi");
    }

    /// Ruling R-TEXTEDIT for the command op: a command that acts on the text being typed in (the
    /// Text menu's) or changes nothing (a copy) runs inside the edit, which goes on, and so does a
    /// `command.batch` of only such steps; a batch with any other step ends the edit first. A
    /// command refused up front runs nothing and ends nothing, and the reads and the input ops
    /// leave the edit as it is.
    #[test]
    fn a_command_ends_the_typing_unless_it_acts_on_the_text() {
        let (mut board, note) = typing_hi();
        let center = json!({"command": "text.alignCenter"});
        for op in [
            json!({"op": "command", "command": "text.alignLeft"}),
            json!({"op": "command", "command": "edit.copy"}),
            json!({"op": "command", "command": "command.batch", "params": {"commands": [center]}}),
            json!({"op": "command", "command": "file.save"}),
            json!({"op": "get"}),
            json!({"op": "commands"}),
            json!({"op": "type", "text": "!"}),
        ] {
            run(&mut board, &op);
            assert!(editing(&board) && gesture_open(&board), "{op}");
        }
        assert_eq!(sticky_text(&board, note), "hi!");

        let steps = [center, json!({"command": "select.none"})];
        let batch =
            json!({"op": "command", "command": "command.batch", "params": {"commands": steps}});
        ask(&mut board, batch);
        assert!(!editing(&board) && !gesture_open(&board));
        assert_eq!(sticky_text(&board, note), "hi!");
    }

    /// CANVAS-CORE-2 (ruling R-TEXTEDIT): the canvas's keys and menus send copy and paste to the
    /// characters of the text being typed in, but the `command` op hands them to the engine, whose
    /// cut and pastes act on layers. A cut, or a paste to replace, deleted the text being typed
    /// without ending its edit: the "Edit text" gesture stayed open, and the user's next click on
    /// a layer cancelled it, taking back the typing, the cut and every op since, though each had
    /// answered ok. A paste joined the typing's undo step. Each now ends the typing first: it is
    /// an undo step of its own, which the next click keeps, and the op is one after it.
    #[test]
    fn a_cut_or_a_paste_ends_the_typing_first() {
        let cut = json!({"command": "edit.cut"});
        let copy = json!({"command": "edit.copy"});
        for op in [
            cut.clone(),
            json!({"command": "edit.pasteReplace"}),
            json!({"command": "edit.paste"}),
            json!({"command": "edit.pasteOver"}),
            json!({"command": "command.batch", "params": {"commands": [copy, cut]}}),
        ] {
            let mut op = op;
            op["op"] = json!("command");
            // A rectangle on the page and on the clipboard, and a sticky being typed in.
            let mut board = board();
            let rectangle = json!([{"type": "rectangle", "x": 600, "y": 0, "width": 100,
                "height": 100}]);
            let added = ask(&mut board, json!({"op": "add", "elements": rectangle}));
            send(&mut board, "edit.copy", json!({"ids": added["ids"]}));
            let note = typing_in_a_new_sticky(&mut board);
            textedit::insert(&mut board.session, "hi").unwrap();

            ask(&mut board, op.clone());
            assert!(
                editing(&board) || !gesture_open(&board),
                "{op}: the typing's gesture is open with no text being typed in"
            );
            // The user's next click, on the rectangle, takes nothing back.
            let shown = board.doc().unwrap();
            pointer_at(&mut board, "down", 650.0, 50.0);
            pointer_at(&mut board, "up", 650.0, 50.0);
            assert!(
                *board.doc().unwrap() == *shown,
                "{op}: the click took edits back"
            );
            // Undo takes back the op alone: the sticky is there, with what was typed.
            send(&mut board, "edit.undo", json!({}));
            assert_eq!(sticky_text(&board, note), "hi", "{op}");
        }
    }

    /// The middle of the box `get` gives `element`, on the page.
    fn middle(element: &Value) -> (f64, f64) {
        let [x, y, width, height] =
            ["x", "y", "width", "height"].map(|key| element[key].as_f64().unwrap());
        (x + width / 2.0, y + height / 2.0)
    }

    /// Plan 8 final review I-1 (the controller's FINAL.P8 ruling): while the user holds a drag on
    /// the canvas, an op that changes the document is refused and changes nothing. Each move of
    /// the drag builds the document again from the one the drag began with (`Session::preview`),
    /// so an op run inside it was taken back by the next move, or by an Escape, though it had
    /// answered ok. Reads still answer during the drag; once the button is up the op goes
    /// through, and what it drew stays through the next drag.
    /// CANVAS-CORE-9: a panic while an op runs is the op's error, as a panic in an engine command
    /// is, and the canvas goes on: in the app it unwound into the window's callbacks, which
    /// aborted the app with every tab.
    #[test]
    fn a_panic_in_an_op_is_its_error() {
        let mut board = board();
        let op = json!({"op": "add", "elements": []});
        let reply = run_guarded(&mut board, &op, |_, _| panic!("a bug in the op"));
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().unwrap();
        assert!(
            error.starts_with("internal error in `add`: a bug in the op"),
            "{error}"
        );
        let sticky = json!([{"type": "sticky", "x": 0, "y": 0}]);
        ask(&mut board, json!({"op": "add", "elements": sticky}));
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// CANVAS-CORE-10: a Mermaid source with double quotes that fails is tried once more with
    /// single ones, but not after its render was stopped for running past the time limit: the
    /// second try froze the window as long again, 10 s in all.
    #[test]
    fn a_mermaid_render_stopped_for_time_is_not_tried_again() {
        let quoted = r#"graph TD; A["a"] --> B"#;
        assert_eq!(
            second_try(quoted, "Parse error on line 1").as_deref(),
            Some("graph TD; A['a'] --> B")
        );
        assert_eq!(
            second_try("graph TD; A --> B", "Parse error on line 1"),
            None
        );
        assert_eq!(
            second_try(quoted, "The diagram can't be drawn: it took too long."),
            None
        );
    }

    /// CANVAS-CORE-8: between the Pen's clicks no button is held, but the path being drawn is an
    /// open gesture, which only Enter (or Escape) ends. An agent's op ran inside it and joined
    /// its undo step: the ⌘Z after Enter took the path and the agent's layers away together. Such
    /// an op waits for the path, as one waits for a drag, and changes nothing; once the path is
    /// ended it goes through, an undo step of its own.
    #[test]
    fn an_op_that_changes_the_document_waits_for_the_pens_path() {
        let mut board = board();
        send(&mut board, "tool.pen", json!({}));
        pointer_at(&mut board, "down", 0.0, 0.0);
        pointer_at(&mut board, "up", 0.0, 0.0);
        pointer_at(&mut board, "down", 100.0, 0.0);
        pointer_at(&mut board, "up", 100.0, 0.0);
        assert!(board.session.tool.pen.is_some() && !board.session.tool.busy());
        let held = board.doc().unwrap();
        let rectangle = json!([{"type": "rectangle", "x": 400, "y": 0}]);
        for op in [
            json!({"op": "add", "elements": rectangle}),
            json!({"op": "command", "command": "page.add"}),
        ] {
            let name = op["op"].as_str().unwrap();
            let reply = run(&mut board, &op);
            let error = format!("{name}: finish the path the Pen draws on the canvas first");
            assert_eq!(reply, json!({"ok": false, "error": error}), "{op}");
            assert!(Arc::ptr_eq(&board.doc().unwrap(), &held), "{op}");
        }
        assert!(press(&mut board, "Enter", json!({})));
        assert!(board.session.tool.pen.is_none() && !gesture_open(&board));
        let path = board.doc().unwrap();
        ask(&mut board, json!({"op": "add", "elements": rectangle}));
        send(&mut board, "edit.undo", json!({}));
        assert!(*board.doc().unwrap() == *path, "the undo took the path too");
    }

    #[test]
    fn an_op_that_changes_the_document_waits_for_the_drag_to_end() {
        let mut board = board();
        let made = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
        let (x, y) = middle(&elements(&mut board)[0]);
        // The Move tool presses on the sticky and drags it past the slop, and the button stays
        // down.
        pointer_at(&mut board, "down", x, y);
        pointer_at(&mut board, "move", x + 40.0, y + 30.0);
        assert!(board.session.tool.busy());
        let held = board.doc().unwrap();
        let steps = board.session.undo_depth();

        let rectangle = json!([{"type": "rectangle", "x": 600, "y": 0}]);
        for op in [
            json!({"op": "add", "elements": rectangle}),
            json!({"op": "update", "elements": [{"id": note.to_string(), "x": 300}]}),
            json!({"op": "delete", "ids": [note.to_string()]}),
            json!({"op": "clear"}),
            json!({"op": "replace", "scene": scene()}),
            json!({"op": "mermaid", "source": "graph TD; A-->B"}),
            json!({"op": "command", "command": "board.sticky", "params": {"x": 900, "y": 0}}),
        ] {
            let name = op["op"].as_str().unwrap();
            let error = format!("{name}: finish the drag on the canvas first");
            assert_eq!(run(&mut board, &op), json!({"ok": false, "error": error}));
            // The page is as the drag shows it, and the drag goes on.
            assert!(Arc::ptr_eq(&board.doc().unwrap(), &held), "{op}");
            assert!(board.session.tool.busy() && gesture_open(&board), "{op}");
        }
        // The reads answer.
        assert_eq!(elements(&mut board).len(), 1);
        ask(&mut board, json!({"op": "commands"}));
        ask(&mut board, json!({"op": "export_svg"}));
        assert_eq!(board.session.undo_depth(), steps);

        // The drag ends as it would have: the sticky moved, in one undo step.
        pointer_at(&mut board, "move", x + 60.0, y + 50.0);
        pointer_at(&mut board, "up", x + 60.0, y + 50.0);
        assert!(!board.session.tool.busy());
        assert_eq!(board.session.undo_depth().0, steps.0 + 1);
        let moved = elements(&mut board);
        assert_eq!(moved.len(), 1, "{moved:?}");
        assert_eq!(
            (moved[0]["x"].clone(), moved[0]["y"].clone()),
            (json!(60.0), json!(50.0))
        );

        // Then the op goes through, and the next drag keeps what it drew.
        let added = ask(&mut board, json!({"op": "add", "elements": rectangle}));
        let drawn = added["ids"][0].clone();
        let (x, y) = middle(&moved[0]);
        pointer_at(&mut board, "down", x, y);
        pointer_at(&mut board, "move", x + 20.0, y + 20.0);
        pointer_at(&mut board, "up", x + 20.0, y + 20.0);
        let after = elements(&mut board);
        assert_eq!(after.len(), 2, "{after:?}");
        let kept = after.iter().find(|element| element["id"] == drawn);
        assert_eq!(kept.map(|element| element["x"].clone()), Some(json!(600.0)));
    }

    /// A 100 × 100 rectangle on the board with its top left corner at (`x`, 0).
    fn square_at(board: &mut BoardDoc, x: f64) -> NodeId {
        let params = json!({"shape": "rectangle", "x": x, "y": 0, "width": 100, "height": 100});
        let made = board.execute("board.shape", &params).unwrap();
        figcraft_engine::params::id_of(&made["id"]).unwrap()
    }

    /// The width an SVG file gives itself, in its `<svg ...>` tag.
    fn svg_width(svg: &str) -> f64 {
        let tag = &svg[..svg.find('>').expect("an <svg> tag")];
        let (_, after) = tag.split_once(" width=\"").expect("a width");
        let (width, _) = after.split_once('"').expect("a quoted width");
        width.parse().expect("a number")
    }

    /// Review Focus 3 and ruling P6-C13: the whole page is exported, whatever is selected.
    #[test]
    fn export_without_selection_exports_page() {
        let mut board = board();
        let left = square_at(&mut board, 0.0);
        square_at(&mut board, 1000.0);
        board.session.set_selection(vec![left]);
        let steps = board.session.undo_depth();
        let svg = String::from_utf8(export_page(&mut board, "svg", 1.0).unwrap()).unwrap();
        assert!(svg.starts_with("<svg"), "{svg}");
        // From the left square's left edge to the right one's right edge: both are in it.
        let width = svg_width(&svg);
        assert!(width >= 1100.0, "width {width}: {svg}");
        // The selected square alone is a tenth of that.
        let one = crate::design::export::export_bytes(
            &mut board,
            &[left],
            &ExportRow {
                format: "svg".into(),
                scale: 1.0,
            },
        );
        let one = svg_width(&String::from_utf8(one.unwrap()).unwrap());
        assert!(one < 200.0, "width {one}");
        // Exporting changes nothing: the selection stays, and there is no undo step.
        assert_eq!(board.session.selection(), [left]);
        assert_eq!(board.session.undo_depth(), steps);
        // At another scale and format: a PNG.
        let png = export_page(&mut board, "png", 0.5).unwrap();
        assert!(png.starts_with(b"\x89PNG"), "{:02x?}", &png[..8]);
    }

    #[test]
    fn an_empty_page_or_an_unknown_format_exports_nothing() {
        let mut board = board();
        let error = export_page(&mut board, "png", 1.0).unwrap_err();
        assert!(error.to_string().contains("nothing to export"), "{error}");
        square_at(&mut board, 0.0);
        let error = export_page(&mut board, "gif", 1.0).unwrap_err();
        assert!(
            error.to_string().contains("not an export format"),
            "{error}"
        );
    }

    /// The text of an SVG file after its optional `<?xml ...?>` prolog.
    fn after_prolog(svg: &str) -> &str {
        let svg = svg.trim_start();
        match svg.strip_prefix("<?xml") {
            Some(rest) => (rest.split_once("?>"))
                .map_or(rest, |(_, after)| after)
                .trim_start(),
            None => svg,
        }
    }

    #[test]
    fn export_png_is_base64_png() {
        let mut board = board();
        square_at(&mut board, 0.0);
        let reply = ask(&mut board, json!({"op": "export_png"}));
        let encoded = reply["png_base64"].as_str().expect("png_base64");
        let png = STANDARD.decode(encoded).unwrap();
        assert!(
            png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "{:02x?}",
            &png[..png.len().min(8)]
        );
        // At 1×, as the page is exported at that scale: not at another, which makes another size.
        let at = |board: &mut BoardDoc, scale| png_size(&export_page(board, "png", scale).unwrap());
        assert_eq!(png_size(&png), at(&mut board, 1.0));
        assert_ne!(png_size(&png), at(&mut board, 2.0));
    }

    #[test]
    fn export_svg_has_svg_root() {
        let mut board = board();
        square_at(&mut board, 0.0);
        let reply = ask(&mut board, json!({"op": "export_svg"}));
        let svg = reply["svg"].as_str().expect("svg");
        assert!(after_prolog(svg).starts_with("<svg"), "{svg}");
        assert!(svg.trim_end().ends_with("</svg>"), "{svg}");
    }

    /// Spec, "Agent wire contract": the ops answer the whole page, and exporting changes nothing
    /// in it.
    #[test]
    fn the_export_ops_export_the_whole_page_and_change_nothing() {
        let mut board = board();
        let left = square_at(&mut board, 0.0);
        square_at(&mut board, 1000.0);
        board.session.set_selection(vec![left]);
        let (doc, steps) = (board.doc().unwrap(), board.session.undo_depth());

        // Both squares are in the picture, though one is selected.
        let svg = ask(&mut board, json!({"op": "export_svg"}));
        let width = svg_width(svg["svg"].as_str().unwrap());
        assert!(width >= 1100.0, "width {width}");
        let png = ask(&mut board, json!({"op": "export_png"}));
        let png = STANDARD
            .decode(png["png_base64"].as_str().unwrap())
            .unwrap();
        assert!(png_size(&png).0 >= 1100, "{:?}", png_size(&png));

        assert_eq!(board.session.selection(), [left]);
        assert_eq!(board.session.undo_depth(), steps);
        assert!(Arc::ptr_eq(&doc, &board.doc().unwrap()));
    }

    #[test]
    fn the_export_ops_of_an_empty_page_are_errors_not_panics() {
        let mut board = board();
        for op in ["export_svg", "export_png"] {
            let error = refusal(&mut board, json!({"op": op}));
            assert!(error.contains("nothing to export"), "{op}: {error}");
        }
    }

    /// Review Focus 3: a page without frames says why it has no PDF.
    #[test]
    fn the_frames_of_the_page_make_a_pdf() {
        let mut board = board();
        let error = frames_pdf(&mut board).unwrap_err();
        assert!(
            error.to_string().contains("the page has no visible frames"),
            "{error}"
        );
        let mut design = design();
        for x in [0, 400] {
            let frame = json!({"type": "frame", "x": x, "y": 0, "width": 300, "height": 200});
            design.execute("node.create", &frame).unwrap();
        }
        let pdf = frames_pdf(&mut design).unwrap();
        assert!(pdf.starts_with(b"%PDF"), "{:02x?}", &pdf[..8]);
    }

    /// Review Focus 5 and ruling P6-C14: Copy as PNG makes no image longer than 4096 px.
    #[test]
    fn copy_png_is_capped() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        let wide = copy_png_scale((10000.0, 500.0), 2.0);
        assert!(close(wide, 4096.0 / 10000.0), "{wide}");
        let tall = copy_png_scale((500.0, 10000.0), 2.0);
        assert!(close(tall, 4096.0 / 10000.0), "{tall}");
        assert!(close(copy_png_scale((100.0, 50.0), 2.0), 2.0));
        // Exactly 4096 px long at the scale wanted, and nothing to measure: as wanted.
        assert!(close(copy_png_scale((2048.0, 10.0), 2.0), 2.0));
        assert!(close(copy_png_scale((0.0, 0.0), 2.0), 2.0));
    }

    /// Runs engine command `id` with `params`, and returns the layer it made.
    fn made(board: &mut BoardDoc, id: &str, params: Value) -> NodeId {
        let made = board.execute(id, &params).unwrap();
        id_of(&made["id"]).unwrap()
    }

    /// The width and height of the PNG file `png`.
    fn png_size(png: &[u8]) -> (u32, u32) {
        figcraft_render::images::dimensions(png).expect("a PNG")
    }

    /// Text, its own CSS or SVG, as Copy as gives it.
    fn copied_text(board: &mut BoardDoc, id: &str) -> String {
        match copy_as(board, id).unwrap() {
            Copied::Text(text) => text,
            Copied::Png(_) => panic!("{id} copied a PNG"),
        }
    }

    /// A PNG file, as Copy as PNG gives it.
    fn copied_png(board: &mut BoardDoc) -> Vec<u8> {
        match copy_as(board, "edit.copyAsPng").unwrap() {
            Copied::Png(png) => png,
            Copied::Text(text) => panic!("Copy as PNG copied text: {text}"),
        }
    }

    #[test]
    fn copy_as_text_copies_the_text_of_each_selected_layer() {
        let mut board = board();
        let note = made(
            &mut board,
            "board.sticky",
            json!({"x": 0, "y": 0, "text": "Ship it"}),
        );
        let shape = json!({"shape": "ellipse", "x": 300, "y": 0, "text": "Review"});
        let shape = made(&mut board, "board.shape", shape);
        let empty = made(&mut board, "board.sticky", json!({"x": 600, "y": 0}));
        let text = json!({"type": "text", "text": "Notes", "x": 0, "y": 400});
        let text = made(&mut board, "node.create", text);
        // A line each, in the order they were selected; the empty sticky has no line.
        board.session.set_selection(vec![shape, note, empty, text]);
        assert_eq!(
            copied_text(&mut board, "edit.copyAsText"),
            "Review\nShip it\nNotes"
        );
        // A frame's text is the text of the text layers in it, each copied once.
        let mut design = design();
        let frame = json!({"type": "frame", "width": 300, "height": 200});
        let frame = made(&mut design, "node.create", frame);
        let texts: Vec<NodeId> = ["Title", "Body"]
            .into_iter()
            .map(|text| {
                let text = json!({"type": "text", "text": text, "parent": frame});
                made(&mut design, "node.create", text)
            })
            .collect();
        design.session.set_selection(vec![frame, texts[0]]);
        assert_eq!(copied_text(&mut design, "edit.copyAsText"), "Title\nBody");
    }

    #[test]
    fn copy_as_text_and_css_need_something_selected_to_copy() {
        let mut board = board();
        for id in ["edit.copyAsText", "edit.copyAsCss"] {
            let error = copy_as(&mut board, id).unwrap_err();
            assert!(error.is_unavailable(), "{id}: {error:?}");
            assert_eq!(error.for_user(), "Nothing is selected", "{id}");
        }
        let square = square_at(&mut board, 0.0);
        board.session.set_selection(vec![square]);
        let error = copy_as(&mut board, "edit.copyAsText").unwrap_err();
        assert_eq!(error.to_string(), "the selection has no text");
        let error = copy_as(&mut board, "edit.copy").unwrap_err();
        assert_eq!(error.to_string(), "`edit.copy` is no Copy as command");
    }

    #[test]
    fn copy_as_css_is_the_first_selected_layers() {
        let mut design = design();
        let rectangle = json!({"type": "rectangle", "width": 120, "height": 80});
        let first = made(&mut design, "node.create", rectangle);
        let ellipse = json!({"type": "ellipse", "width": 30, "height": 30});
        let second = made(&mut design, "node.create", ellipse);
        design.session.set_selection(vec![first, second]);
        let css = copied_text(&mut design, "edit.copyAsCss");
        assert!(css.contains("width: 120px"), "{css}");
        assert!(!css.contains("width: 30px"), "{css}");
    }

    /// Copy as SVG and PNG copy the selection together, and with nothing selected, the page, as
    /// File ▸ Export does. Nothing in the document changes.
    #[test]
    fn copy_as_svg_and_png_copy_the_selection_or_the_page() {
        let mut board = board();
        let left = square_at(&mut board, 0.0);
        square_at(&mut board, 1000.0);
        board.session.set_selection(vec![left]);
        let steps = board.session.undo_depth();
        let svg = copied_text(&mut board, "edit.copyAsSvg");
        assert!(svg.starts_with("<svg"), "{svg}");
        assert!(svg_width(&svg) < 200.0, "{svg}");
        // At 2×, as FigCraft's own app copies.
        let doc = board.doc().unwrap();
        let area = export_area(&doc, &[left], &ExportSetting::default()).unwrap();
        let at_2x = |side: f64| (side * 2.0).ceil() as u32;
        assert_eq!(
            png_size(&copied_png(&mut board)),
            (at_2x(area.width()), at_2x(area.height()))
        );
        // Nothing selected: the page, both squares.
        board.session.set_selection(Vec::new());
        assert!(svg_width(&copied_text(&mut board, "edit.copyAsSvg")) >= 1100.0);
        assert!(png_size(&copied_png(&mut board)).0 >= 2200);
        assert_eq!(board.session.undo_depth(), steps);
        assert!(board.session.selection().is_empty());
    }

    /// Review Focus 5 and ruling P6-C14: the PNG's longest side is at most 4096 px, and an area
    /// too long for that at the engine's least scale is refused instead of copied longer.
    #[test]
    fn copy_as_png_caps_the_longest_side() {
        let mut design = design();
        let wide = json!({"type": "rectangle", "width": 10000, "height": 500});
        let wide = made(&mut design, "node.create", wide);
        design.session.set_selection(vec![wide]);
        let (width, height) = png_size(&copied_png(&mut design));
        assert_eq!(width, 4096);
        // 500 at 4096 / 10000, on whole pixels.
        assert_eq!(height, 205);
        let longer = json!({"ids": [wide], "props": {"width": 500_000}});
        design.execute("node.set", &longer).unwrap();
        let error = copy_as(&mut design, "edit.copyAsPng").unwrap_err();
        assert_eq!(
            error.to_string(),
            "The selection is too large to copy as PNG"
        );
        design.session.set_selection(Vec::new());
        let error = copy_as(&mut design, "edit.copyAsPng").unwrap_err();
        assert_eq!(error.to_string(), "The page is too large to copy as PNG");
    }

    /// One red rectangle, 100 × 80, as an SVG file holds it.
    const RECT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect width="100" height="80" fill="#f00"/></svg>"##;

    /// A board whose view shows page point (300.5, 200) in its middle.
    fn board_seen_about_its_point() -> BoardDoc {
        let mut board = board();
        let view = json!({"zoom": 1, "vw": 1000, "vh": 800, "cx": 300.5, "cy": 200});
        board.execute("view.set", &view).unwrap();
        board
    }

    /// Layer `id`'s top left corner and size on the page.
    fn placed(board: &BoardDoc, id: NodeId) -> (f64, f64, f64, f64) {
        let bounds = board.doc().unwrap().world_bounds(id).unwrap();
        (bounds.x0, bounds.y0, bounds.width(), bounds.height())
    }

    /// File ▸ Import and Place image: what they make is centred in the view, as a paste is, and
    /// selected, one undo step each.
    #[test]
    fn an_imported_file_is_centred_in_the_view() {
        let dir = tempfile::tempdir().unwrap();
        // A 160 × 90 PNG, as Export makes one.
        let mut design = design();
        let rectangle = json!({"type": "rectangle", "width": 160, "height": 90});
        let rectangle = made(&mut design, "node.create", rectangle);
        let png = export_bytes(&mut design, &[rectangle], &ExportRow::default()).unwrap();
        let photo = dir.path().join("photo.png");
        std::fs::write(&photo, png).unwrap();
        // An SVG file, whatever the case of its extension.
        let logo = dir.path().join("Logo.SVG");
        std::fs::write(&logo, RECT_SVG).unwrap();

        let mut board = board_seen_about_its_point();
        let [image] = import_file(&mut board, &photo, false).unwrap()[..] else {
            panic!("no image")
        };
        // Its middle on the view's, its corner on whole units.
        assert_eq!(placed(&board, image), (221.0, 155.0, 160.0, 90.0));
        assert_eq!(board.session.selection(), [image]);
        let name = |board: &BoardDoc, id| board.doc().unwrap().node(id).unwrap().name.clone();
        assert_eq!(name(&board, image), "photo");
        let [frame] = import_file(&mut board, &logo, false).unwrap()[..] else {
            panic!("no frame")
        };
        assert_eq!(placed(&board, frame), (251.0, 160.0, 100.0, 80.0));
        let [again] = import_file(&mut board, &photo, true).unwrap()[..] else {
            panic!("no placed image")
        };
        assert_eq!(placed(&board, again), (221.0, 155.0, 160.0, 90.0));
        assert_eq!(name(&board, again), "photo");
        assert_eq!(board.session.selection(), [again]);
        assert_eq!(board.session.undo_depth().0, 3);
    }

    #[test]
    fn a_file_that_cannot_be_imported_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut board = board();
        let missing = dir.path().join("missing.png");
        let error = import_file(&mut board, &missing, false).unwrap_err();
        assert!(matches!(error, BoardError::Io(_)), "{error:?}");
        // Place image takes images only.
        let notes = dir.path().join("notes.txt");
        std::fs::write(&notes, "hello").unwrap();
        assert!(import_file(&mut board, &notes, true).is_err());
        assert!(page_layers(&board).unwrap().is_empty());
        assert_eq!(board.session.undo_depth(), (0, 0));
    }
}
