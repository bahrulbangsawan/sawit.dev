//! Canvas files by their names: the ones a `command` op must not write over, the ones Sawit
//! reads as canvas files and hands to the engine by their bytes, and File ▸ Import.

use std::ffi::OsStr;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_doc::NodeId;
use figcraft_engine::params::id_of;
use serde_json::{Value, json};

use super::BATCH;
use crate::clipboard;
use crate::doc::{BoardDoc, BoardError};

/// The ends of the names of the files a canvas is kept in: the one it is saved to (`.sawit`), and
/// the old one that it was migrated from.
const CANVAS_EXTENSIONS: [&str; 2] = [".sawit", ".excalidraw"];

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

/// Whether `path` names a canvas file: a `.sawit` or `.excalidraw` file, or one of the copies and
/// staging files that Sawit keeps beside one.
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

/// Whether `path` names a Sawit canvas file: a `.sawit` file, or a copy Sawit keeps of one under
/// another name, the backups of the store (`<name>.sawit.bak` and `<name>.sawit.del.bak`, the older
/// `.bak.<n>` and `.del.bak.<n>`, and the `.lossy.bak` of a file opened with notes:
/// [`sawit_core::nyawit::is_backup_suffix`]). Only the name counts, in any case.
pub fn is_sawit_file(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy().to_lowercase();
    let extension = format!(".{}", sawit_core::files::CANVAS_EXTENSION);
    name.rfind(&extension).is_some_and(|at| {
        let kept = &name[at + extension.len()..];
        kept.is_empty() || sawit_core::nyawit::is_backup_suffix(kept)
    })
}

/// Whether `path` is named with the canvas engine's own extension ([`figcraft_format::EXTENSION`]),
/// in any case. Sawit's canvas files are `.sawit`, and reach the engine under that name only as
/// bytes in memory ([`engine_name`]); a file a user or an agent names that way is refused, not
/// handed to the engine, which would read it as a canvas file.
fn has_engine_extension(path: &Path) -> bool {
    (path.extension()).is_some_and(|ext| ext.eq_ignore_ascii_case(figcraft_format::EXTENSION))
}

/// Why `file.import`, run by an agent, won't bring in a file named as [`has_engine_extension`]
/// has it, after the file's path.
const NOT_A_SAWIT_FILE: &str = "isn't a canvas file Sawit reads: name a .sawit file";

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
    if !is_sawit_file(file) {
        return name.to_string();
    }
    format!("{name}.{}", figcraft_format::EXTENSION)
}

/// `params` of command `id`, with the Sawit canvas file that a `file.import` names by its `path`
/// (a `.sawit` file, or a copy Sawit keeps of a canvas file: [`is_sawit_file`]) handed to the
/// engine by its bytes instead, named as the engine knows a canvas file ([`engine_name`]): it
/// tells a file's kind by its extension, and would place one as an image. The steps of a
/// `command.batch` are read the same way, a nested one's too; a path that another step's result
/// gives is the engine's. A file that can't be read is the error, and nothing runs.
pub(super) fn canvas_files_by_bytes(id: &str, mut params: Value) -> Result<Value, String> {
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
    if let Some(path) =
        path.filter(|path| id == "file.import" && has_engine_extension(Path::new(path)))
    {
        return Err(format!("file.import: {path} {NOT_A_SAWIT_FILE}"));
    }
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

/// Imports the file at `path` into the page on show, as File ▸ Import from file… does
/// (`file.import`), or with `place` as Place image… does (`file.placeImage`), which takes only
/// images. The layer it made, which is then selected.
///
/// The import lands in the middle of the view, as a paste does ([`clipboard::paste`]): an image or
/// an SVG file is centred there. A design file (`.sawit`, `.fig`, `.sketch`, `.penpot`) adds its
/// pages after the canvas's own instead, and makes no layer on the page on show. One undo step. A
/// file that can't be read, or that the engine can't import, is the error, which doesn't name the
/// file, and nothing changes; so is one named with the engine's own extension
/// ([`has_engine_extension`]), unread.
pub fn import_file(
    board: &mut BoardDoc,
    path: &Path,
    place: bool,
) -> Result<Vec<NodeId>, BoardError> {
    if has_engine_extension(path) {
        return Err(BoardError::Format(
            "not a file Sawit imports; choose a .sawit file".into(),
        ));
    }
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
