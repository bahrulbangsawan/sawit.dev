//! Command ops that would write over a canvas file.

use std::sync::Arc;

use figcraft_engine::params::id_of;
use serde_json::json;

use super::*;
use crate::ops::refusal::{HANDLED_BY_SAWIT, WRITES_TO_PATH};

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
    ] {
        assert!(is_canvas_file(Path::new(name)), "{name}");
    }
    // An export to an image or a text path is not one, whatever its name holds.
    for name in [
        "page.png",
        "page.svg",
        "canvas.sawit.svg",
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
