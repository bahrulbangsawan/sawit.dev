//! Command ops that would read a file that never ends.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_engine::params::id_of;
use serde_json::json;

use super::*;
use crate::ops::refusal::{HANDLED_BY_SAWIT, READS_FROM_PATH, WRITES_TO_PATH};

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
