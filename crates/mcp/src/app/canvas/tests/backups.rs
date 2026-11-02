//! The backups a clear, a load and a deletion keep with Sawit closed, apart, and never through a
//! link.

use rmcp::handler::server::wrapper::Parameters;
use sawit_board::store::Stamp;

use super::*;
use crate::app::canvas::stored::run_and_save;
use crate::app::canvas::{
    CanvasCommandArgs, CanvasDeleteArgs, CanvasElementsArgs, CanvasLoadArgs, CanvasTargetArgs,
};
use crate::app::tests::{answer, native, said};

/// Plan 7 final review T5-6: with Sawit closed every call opens the canvas afresh, so a load
/// can't be undone as it can in the app. The canvas it replaces is kept as `<file>.bak`, as a
/// clear keeps one, so one mistaken load doesn't lose the board. A canvas with nothing on it
/// has nothing to keep.
#[tokio::test]
async fn an_offline_load_keeps_the_canvas_it_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    let bak = file.with_extension("sawit.bak");
    write_board(&file, 2);
    let replaced = std::fs::read(&file).unwrap();
    let other = dir.path().join("other.sawit");
    write_board(&other, 1);
    let load = |path: &Path| CanvasLoadArgs {
        path: Some(path.display().to_string()),
        scene: None,
        file: None,
        project: project_arg(&project),
    };

    let loaded = server.canvas_load(Parameters(load(&other))).await;
    assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 1);
    assert_eq!(layers_in(&file).len(), 1);
    assert_eq!(std::fs::read(&bak).unwrap(), replaced);

    // Over a canvas with nothing on it.
    std::fs::remove_file(&bak).unwrap();
    write_board(&file, 0);
    let loaded = server.canvas_load(Parameters(load(&other))).await;
    assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 1);
    assert!(!bak.exists());
}

/// CANVAS-CORE-3: with Sawit closed every call opens the canvas afresh, so its backups are all
/// that is left of a board an agent clears or loads over. Starting over twice (a clear, a
/// drawing and a clear; or a load, then another) kept one `.bak`, of the step between, and
/// the user's board was in no file. It is still kept.
#[tokio::test]
async fn starting_over_twice_with_sawit_closed_keeps_the_users_board() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    let window = || CanvasTargetArgs {
        file: None,
        project: project_arg(&project),
    };
    write_board(&file, 3);
    let users = std::fs::read(&file).unwrap();

    // A clear, a drawing and a clear.
    let cleared = server.canvas_clear(Parameters(window())).await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    let drawn = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![json!({"type": "rectangle", "x": 0, "y": 0})],
            file: None,
            project: project_arg(&project),
            work: None,
        }))
        .await;
    assert_ne!(drawn.is_error, Some(true), "{}", said(&drawn));
    let cleared = server.canvas_clear(Parameters(window())).await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    assert!(layers_in(&file).is_empty());
    assert!(
        backups_of(&file).contains(&users),
        "the user's board is gone"
    );

    // A load, then another.
    write_board(&file, 3);
    let users = std::fs::read(&file).unwrap();
    let (one, two) = (dir.path().join("one.sawit"), dir.path().join("two.sawit"));
    write_board(&one, 1);
    write_board(&two, 2);
    for other in [&one, &two] {
        let loaded = server
            .canvas_load(Parameters(CanvasLoadArgs {
                path: Some(other.display().to_string()),
                scene: None,
                file: None,
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(loaded.is_error, Some(true), "{}", said(&loaded));
    }
    assert_eq!(layers_in(&file).len(), 2);
    assert!(
        backups_of(&file).contains(&users),
        "the user's board is gone"
    );
}

/// With Sawit closed there is no undo across calls, and an op that deleted layers kept a
/// backup only when it left the canvas empty: canvas_delete, canvas_clear of a page while
/// another page keeps its layers, and canvas_command's edit.delete or page.delete took them
/// for good. Each keeps the board it took them off now, the one before moving along: a
/// deletion as `canvas.sawit.del.bak`, a clear as `canvas.sawit.bak`. An op that only
/// adds or moves keeps none.
#[tokio::test]
async fn every_deletion_with_sawit_closed_keeps_the_board_it_took_layers_off() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 3);
    let id_of = |text: &str| {
        let layers = layers_in(&file);
        let layer = layers.iter().find(|layer| layer["text"] == json!(text));
        layer.unwrap()["id"].as_str().unwrap().to_string()
    };
    let command = |command: &str, params: Value| {
        let run = server.canvas_command_tool(CanvasCommandArgs {
            command: command.into(),
            params: Some(params),
            file: None,
            project: project_arg(&project),
            work: None,
        });
        async {
            let ran = run.await;
            answer(&ran)["result"].clone()
        }
    };
    let holds = |backup: &[u8], texts: &[&str]| {
        let held = texts_of(backup);
        texts.iter().all(|text| held.contains(*text))
    };
    let newest_deletion_holds = |texts: &[&str]| holds(&deletion_backups_of(&file)[0], texts);

    let deleted = server
        .canvas_delete(Parameters(CanvasDeleteArgs {
            ids: vec![id_of("note 0")],
            file: None,
            project: project_arg(&project),
            work: None,
        }))
        .await;
    assert_eq!(answer(&deleted)["deleted"], json!(1));
    assert!(newest_deletion_holds(&["note 0", "note 1", "note 2"]));
    let note = id_of("note 1").parse::<u64>().unwrap();
    command("edit.delete", json!({"ids": [note]})).await;
    assert!(newest_deletion_holds(&["note 1", "note 2"]));
    assert_eq!(deletion_backups_of(&file).len(), 2);
    assert!(
        backups_of(&file).is_empty(),
        "a deletion kept a clear's backup"
    );

    // Two more pages with a sticky each, the first page on show again: nothing is taken off.
    add_pages(&file, &[("Two", "on two"), ("Three", "on three")]);
    assert_eq!(deletion_backups_of(&file).len(), 2);
    assert!(backups_of(&file).is_empty());

    // A clear of the first page, while the others keep their stickies: a clear's backup.
    let cleared = server
        .canvas_clear(Parameters(CanvasTargetArgs {
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    assert!(layers_in(&file).is_empty());
    assert!(holds(
        &backups_of(&file)[0],
        &["note 2", "on two", "on three"]
    ));
    assert_eq!(backups_of(&file).len(), 1);
    assert_eq!(deletion_backups_of(&file).len(), 2);

    // A page deleted with its sticky, while the third keeps its own.
    let pages = command("page.list", json!({})).await;
    let two = pages
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Two");
    command("page.delete", json!({"id": two.unwrap()["id"]})).await;
    assert!(newest_deletion_holds(&["on two", "on three"]));
    assert_eq!(deletion_backups_of(&file).len(), 3);
    assert_eq!(backups_of(&file).len(), 1);
    assert!(texts_of(&std::fs::read(&file).unwrap()).contains("on three"));
}

/// CANVAS-CORE-3, with deletions: they shared the five `.bak` files with clears and loads,
/// so an agent that cleared the user's board, drew, then deleted five stickies one call at a
/// time pushed the cleared board out of every backup, and with Sawit closed nothing undoes a
/// call. Deletions keep their own five now (`.del.bak`): after a clear and six deletions,
/// canvas_load brings the cleared board back from the clear's `canvas.sawit.bak`.
#[tokio::test]
async fn deletions_after_a_clear_never_push_out_the_board_it_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 3);
    let users = std::fs::read(&file).unwrap();

    let cleared = server
        .canvas_clear(Parameters(CanvasTargetArgs {
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    let stickies = (0..7)
        .map(|n| json!({"type": "sticky", "x": n * 300, "y": 0, "text": format!("drawn {n}")}))
        .collect();
    let drawn = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: stickies,
            file: None,
            project: project_arg(&project),
            work: None,
        }))
        .await;
    assert_ne!(drawn.is_error, Some(true), "{}", said(&drawn));
    for n in 0..6 {
        let layers = layers_in(&file);
        let text = format!("drawn {n}");
        let layer = layers.iter().find(|layer| layer["text"] == json!(text));
        let id = layer.unwrap()["id"].as_str().unwrap().to_string();
        let deleted = server
            .canvas_delete(Parameters(CanvasDeleteArgs {
                ids: vec![id],
                file: None,
                project: project_arg(&project),
                work: None,
            }))
            .await;
        assert_eq!(answer(&deleted)["deleted"], json!(1), "{}", said(&deleted));
    }
    assert_eq!(layers_in(&file).len(), 1);
    assert_eq!(
        deletion_backups_of(&file).len(),
        5,
        "six deletions keep five"
    );
    assert_eq!(
        backups_of(&file),
        [users],
        "the cleared board was pushed out"
    );

    let loaded = server
        .canvas_load(Parameters(CanvasLoadArgs {
            path: Some(file.with_extension("sawit.bak").display().to_string()),
            scene: None,
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(loaded.is_error, Some(true), "{}", said(&loaded));
    let back = texts_in(&file);
    assert!(
        ["note 0", "note 1", "note 2"]
            .iter()
            .all(|note| back.contains(*note)),
        "{back:?}"
    );
    // A deletion's backup loads too: the board before the last deletion.
    let loaded = server
        .canvas_load(Parameters(CanvasLoadArgs {
            path: Some(file.with_extension("sawit.del.bak").display().to_string()),
            scene: None,
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(loaded.is_error, Some(true), "{}", said(&loaded));
    let back = texts_in(&file);
    assert!(
        back.contains("drawn 5") && back.contains("drawn 6"),
        "{back:?}"
    );
}

/// CANVAS-CORE-3: a load the changed-on-disk guard refuses changes nothing, the backups
/// included. Its `.bak` used to be made first, from the other process's file.
#[test]
fn a_refused_load_leaves_the_backups_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let file = board_file(&project_in(dir.path()));
    write_board(&file, 1);
    let bak = file.with_extension("sawit.bak");
    write_board(&bak, 4);
    let before = backups_of(&file);
    let read = Stamp::of(&file);
    let mut board = store::open(&file).unwrap();
    write_board(&file, 2);
    let theirs = std::fs::read(&file).unwrap();

    let other = dir.path().join("other.sawit");
    write_board(&other, 3);
    let load = json!({"op": "replace", "path": other});
    let refused = run_and_save(&mut board, &file, read, &load).unwrap_err();
    assert!(refused.contains("changed on disk"), "{refused}");
    assert_eq!(std::fs::read(&file).unwrap(), theirs);
    assert!(backups_of(&file) == before, "the backup was replaced");
}

/// CANVAS-CORE-1: a cloned project can hold a link at `canvas.sawit.bak` to any file of
/// the user's. A load with Sawit closed keeps the canvas it replaces in place of the link,
/// never through it.
#[tokio::test]
async fn an_offline_load_never_writes_through_a_link_at_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    let bak = file.with_extension("sawit.bak");
    write_board(&file, 2);
    let replaced = std::fs::read(&file).unwrap();
    let victim = dir.path().join("victim");
    std::fs::write(&victim, "the user's").unwrap();
    std::os::unix::fs::symlink(&victim, &bak).unwrap();
    let other = dir.path().join("other.sawit");
    write_board(&other, 1);

    let loaded = server
        .canvas_load(Parameters(CanvasLoadArgs {
            path: Some(other.display().to_string()),
            scene: None,
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(loaded.is_error, Some(true), "{}", said(&loaded));
    assert_eq!(layers_in(&file).len(), 1);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "the user's");
    assert!(
        std::fs::symlink_metadata(&bak)
            .unwrap()
            .file_type()
            .is_file()
    );
    assert_eq!(std::fs::read(&bak).unwrap(), replaced);
}
