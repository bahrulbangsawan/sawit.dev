//! The engine commands the `command` op refuses before anything runs: the ones Sawit carries out
//! itself, and the ones that would write over a canvas file or read a file that never ends.

use std::path::Path;

use serde_json::Value;
use thiserror::Error;

use super::BATCH;
use super::files::is_canvas_file;

/// The commands the `command` op refuses: they switch, open, close or write documents behind the
/// canvas's store. Sawit does these itself.
///
/// This is the cheap first check, and the ids can't be a complete list: [`Documents`] catches what
/// else a command does to the session's documents.
pub(super) const HANDLED_BY_SAWIT: &[&str] = &[
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

/// The command in [`HANDLED_BY_SAWIT`] that `id` is, if it is one.
pub(super) fn handled(id: &str) -> Option<&'static str> {
    HANDLED_BY_SAWIT
        .iter()
        .find(|handled| **handled == id)
        .copied()
}

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
pub(super) const WRITES_TO_PATH: &[&str] = &[
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
pub(super) const READS_FROM_PATH: &[(&str, &[&str])] = &[
    ("file.import", &["path", "variables"]),
    ("file.recover", &["path"]),
    ("image.place", &["path"]),
    ("variable.import", &["variables"]),
    ("collab.recoverFile", &["path"]),
];

/// Why the `command` op won't run `id` with `params`, if it won't. `id` itself, or a step of a
/// `command.batch`, however deeply batches are nested, is one that Sawit handles
/// ([`HANDLED_BY_SAWIT`]), or that would write over a canvas file ([`WRITES_TO_PATH`]), or read a
/// file that never ends ([`READS_FROM_PATH`]), or a step that writes where its params say and
/// takes any of them from another step's result ([`refers_to_a_step`]), or reads where they say
/// and takes that from one: what that names isn't known until the batch runs (CANVAS-CORE-7).
pub(super) fn refused(id: &str, params: &Value) -> Option<Refusal> {
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
pub(super) enum Refusal {
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
