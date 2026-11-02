//! The `replace` op: another canvas, an Excalidraw scene or a canvas file, loaded in place of the
//! canvas's pages.

use std::ffi::OsStr;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::elements::get;
use super::failure;
use super::files::{engine_name, is_sawit_file};
use crate::doc::{BoardDoc, BoardError};
use crate::excalidraw;

pub(super) fn replace(board: &mut BoardDoc, op: &Value) -> Value {
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
/// `.sawit` file, or a copy Sawit keeps of one, is read here and handed to the engine's
/// `file.import` by its bytes ([`kept_copy`]). Any other file is refused.
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
