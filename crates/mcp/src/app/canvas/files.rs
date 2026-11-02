//! The files the canvas tools name: the canvas file of `file`, a scene to load, a file or folder an
//! engine command's parameters name, and where an export goes.

use std::path::{Path, PathBuf};

use sawit_board::{ops, store};
use sawit_core::files::FileKind;
use serde_json::Value;

/// The text of the scene file at `path`, for `canvas_load`: only a regular file of at most
/// [`sawit_board::excalidraw::MAX_SCENE`] bytes is read, as the canvas reads an `.excalidraw`
/// file ([`sawit_core::files::read_regular`]). A named pipe blocked the read for ever, and a link
/// a cloned repository holds to `/dev/zero` read until the Mac ran out of memory (CANVAS-CORE-6);
/// both are refused at once. It blocks, so it runs off the server's async threads.
pub(super) fn read_scene(path: &Path) -> std::io::Result<String> {
    let bytes = sawit_core::files::read_regular(path, sawit_board::excalidraw::MAX_SCENE)?;
    String::from_utf8(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })
}

/// Whether `path` names a Sawit canvas file: a `.sawit` file, or a backup Sawit keeps of one,
/// such as the `canvas.sawit.bak` a clear keeps or the `canvas.sawit.del.bak` a deletion keeps
/// ([`ops::is_sawit_file`]), which loads as the board it was (MCP-6).
pub(super) fn is_sawit_file(path: &str) -> bool {
    ops::is_sawit_file(Path::new(path.trim()))
}

/// The canvas file an agent names with `file`, if it names one: trimmed (a blank one names none),
/// `~` for the home folder, absolute, and through the links on its way, as the app names the files
/// it opens ([`sawit_core::files::resolve`]). It is a `.sawit` file that is there, or one to be
/// made in a folder that is there (a new board, written by the first edit), or an `.excalidraw`
/// file that is there or was imported into the `.sawit` file beside it ([`store::bound_file`]).
/// Anything else is refused ("name a .sawit or .excalidraw file"). It is checked before the app or
/// a file is touched, so a mistaken name opens no tab and makes no file.
pub(super) fn canvas_file(file: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(named) = file.map(str::trim).filter(|file| !file.is_empty()) else {
        return Ok(None);
    };
    let path = crate::expand_home(named);
    if !path.is_absolute() {
        return Err(format!(
            "file “{named}” must be an absolute path (or start with ~/): nothing was changed."
        ));
    }
    let path = sawit_core::files::resolve(&path);
    // A link left at the name is no file to make, nor one to write through.
    let there = std::fs::symlink_metadata(&path).is_ok();
    let in_a_folder = path.parent().is_some_and(Path::is_dir);
    match sawit_core::files::classify(&path) {
        FileKind::Canvas if path.is_file() || (!there && in_a_folder) => Ok(Some(path)),
        FileKind::Canvas if there => Err(format!("{} isn't a canvas file.", path.display())),
        FileKind::Canvas => Err(format!(
            "{} isn't there, nor is the folder a new canvas file would go in.",
            path.display()
        )),
        FileKind::Excalidraw if path.is_file() || store::bound_file(&path).is_file() => {
            Ok(Some(path))
        }
        FileKind::Excalidraw => Err(format!(
            "{} isn't there: name an .excalidraw file to import.",
            path.display()
        )),
        _ => Err(format!(
            "{} isn't a canvas file: name a .sawit or .excalidraw file.",
            path.display()
        )),
    }
}

/// `path` as the app, whose working folder is not the agent's, can open it: `~` expanded, and a
/// relative path taken from the folder the agent runs in. It is text for a JSON op, so a folder
/// that isn't valid UTF-8 comes out with replacement characters, and the canvas says it can't be read.
pub(super) fn absolute_path(path: &str) -> String {
    crate::agent_path(path).to_string_lossy().into_owned()
}

/// The engine's commands that write or read a file or a folder named in one of their parameters,
/// with that parameter. Every other `path` an engine command takes is no file: SVG path data
/// (`vector.create`, `board.ink`, `node.create`), a layer (`text.onPath`), a place in a prototype's
/// actions (`proto.action.*`).
pub(super) const FILE_PARAMS: &[(&str, &str)] = &[
    ("file.export", "path"),
    ("file.exportSvg", "path"),
    ("file.exportFramesPdf", "path"),
    ("file.exportLayers", "dir"),
    ("data.fill", "dir"),
    ("variable.export", "path"),
    ("component.playground", "path"),
    ("codeConnect.export", "path"),
    ("codeConnect.import", "path"),
    ("file.import", "path"),
    ("file.placeImage", "path"),
    ("image.place", "path"),
    ("library.add", "path"),
    ("text.loadFont", "path"),
    ("collab.recoverFile", "path"),
];

/// `params` of engine command `command`, with the file or folder it names ([`FILE_PARAMS`]) made
/// absolute as the agent means it ([`absolute_path`]), as canvas_load makes its own: the app,
/// whose folder is not the agent's, runs the command, and the engine reads no `~` (MCP-8). The
/// steps of a `command.batch` are read the same way. A share link (`library.add` takes one) and
/// every other parameter go as they are.
pub(super) fn with_files_resolved(command: &str, mut params: Value) -> Value {
    if command == "command.batch" {
        let steps = params.get_mut("commands").and_then(Value::as_array_mut);
        for step in steps.into_iter().flatten() {
            let id = step
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let (Some(id), Some(params)) = (id, step.get_mut("params")) {
                *params = with_files_resolved(&id, params.take());
            }
        }
        return params;
    }
    let names_a_file = FILE_PARAMS.iter().find(|(id, _)| *id == command);
    if let Some((_, key)) = names_a_file
        && let Some(Value::String(path)) = params.get_mut(*key)
        && !path.trim().is_empty()
        && !path.contains("://")
    {
        *path = absolute_path(path);
    }
    params
}

/// Where an export goes when no path is given: the project's `.nyawit/exports/`, else
/// `<cache>/canvas`, named after the canvas file (`stem`) and the time.
pub(super) fn export_path(project: Option<&Path>, cache: &Path, stem: &str, ext: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let name = format!("{stem}-{stamp}.{ext}");
    match project {
        Some(project) => {
            let dir = sawit_core::nyawit::exports_dir(project);
            let _ = sawit_core::nyawit::ensure(project, &dir);
            dir.join(name)
        }
        None => cache.join("canvas").join(name),
    }
}
