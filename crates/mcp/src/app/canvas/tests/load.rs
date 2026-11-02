//! `canvas_load`: a canvas file loaded where it is, a scene read as the agent means its path, and
//! a pipe or a device refused at once.

use rmcp::handler::server::wrapper::Parameters;

use super::*;
use crate::app::canvas::files::{absolute_path, is_sawit_file};
use crate::app::canvas::{CanvasLoadArgs, CanvasTargetArgs};
use crate::app::tests::{answer, native, said};

#[tokio::test]
async fn canvas_load_takes_a_sawit_file_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    let file = board_file(&project);
    write_board(&file, 1);
    let other = dir.path().join("elsewhere").join("board.SAWIT");
    write_board(&other, 3);
    let other_bytes = std::fs::read(&other).unwrap();
    let load = |path: &Path| CanvasLoadArgs {
        path: Some(path.display().to_string()),
        scene: None,
        file: None,
        project: project_arg(&project),
    };

    let loaded = server.canvas_load(Parameters(load(&other))).await;
    assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 3);
    assert_eq!(layers_in(&file).len(), 3);
    assert_eq!(std::fs::read(&other).unwrap(), other_bytes);

    // An Excalidraw file is still read here and sent as a scene.
    let old = dir.path().join("old.excalidraw");
    std::fs::write(&old, OLD_BOARD).unwrap();
    let loaded = server.canvas_load(Parameters(load(&old))).await;
    assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 9);
    assert_eq!(layers_in(&file).len(), 9);

    // A .sawit file that isn't there is an error that changes nothing.
    let before = std::fs::read(&file).unwrap();
    let missing = server
        .canvas_load(Parameters(load(&dir.path().join("nope.sawit"))))
        .await;
    assert_eq!(missing.is_error, Some(true), "{}", said(&missing));
    assert_eq!(std::fs::read(&file).unwrap(), before);

    // Neither is no load at all.
    let nothing = server
        .canvas_load(Parameters(CanvasLoadArgs {
            path: None,
            scene: None,
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_eq!(said(&nothing), "Give a path or a scene.");
}

/// CANVAS-CORE-6, for an agent's load: `canvas_load` of a file that is a named pipe, or a link
/// a cloned repository holds to `/dev/zero`, never answered. The canvas's engine read a
/// `.sawit` file (with the app closed, in the blocking task that holds every other canvas
/// call back; with it open, on its UI thread), and the tool itself read any other, on the
/// server's async thread. Each is refused at once now, and the canvas is as it was.
#[test]
fn canvas_load_of_a_pipe_or_a_device_is_refused_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 1);
    let before = std::fs::read(&file).unwrap();
    let refused_at_once = |paths: &[PathBuf]| {
        let failed: Vec<String> = (paths.iter())
            .filter_map(|path| {
                let (server, project) = (server.clone(), project_arg(&project));
                let path = path.display().to_string();
                let load = CanvasLoadArgs {
                    path: Some(path.clone()),
                    scene: None,
                    file: None,
                    project,
                };
                let call = async move { server.canvas_load(Parameters(load)).await };
                let Some(result) = within_seconds(call) else {
                    return Some(format!("{path}: no answer within 5 s"));
                };
                let error = said(&result);
                let refused = result.is_error == Some(true)
                    && error.contains(&path)
                    && error.contains("not a regular file");
                (!refused).then(|| format!("{path}: {error}"))
            })
            .collect();
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    };
    let pipes = ["board.sawit", "scene.excalidraw", "scene.json"].map(|n| dir.path().join(n));
    for pipe in &pipes {
        let made = std::process::Command::new("mkfifo").arg(pipe).status();
        assert!(made.unwrap().success(), "mkfifo {}", pipe.display());
    }
    // The pipes first: a device that never ends is read only once they are refused.
    refused_at_once(&pipes);
    let zeros = ["zero.sawit", "zero.excalidraw"].map(|n| dir.path().join(n));
    for zero in &zeros {
        std::os::unix::fs::symlink("/dev/zero", zero).unwrap();
    }
    refused_at_once(&zeros);
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

/// MCP-6: canvas_clear keeps the board it cleared as `canvas.sawit.bak`, and canvas_load
/// brings it back from there. It read the backup as Excalidraw JSON ("stream did not contain
/// valid UTF-8"), so no tool could restore a cleared board.
#[tokio::test]
async fn canvas_load_brings_back_the_board_a_clear_kept() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 3);

    let cleared = server
        .canvas_clear(Parameters(CanvasTargetArgs {
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    assert!(layers_in(&file).is_empty());

    let backup = file.with_extension("sawit.bak");
    let loaded = server
        .canvas_load(Parameters(CanvasLoadArgs {
            path: Some(backup.display().to_string()),
            scene: None,
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 3);
    let texts = texts_in(&file);
    assert!(
        (0..3).all(|n| texts.contains(&format!("note {n}"))),
        "{texts:?}"
    );
}

/// MCP-7: canvas_load reads an Excalidraw file's path as it reads a `.sawit` file's:
/// trimmed, `~` for the home folder, and relative to the agent's folder. It read the path as
/// written: a space at its end, or `~/`, was "No such file or directory".
#[tokio::test]
async fn canvas_load_reads_an_import_path_as_it_reads_a_sawit_path() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let old = dir.path().join("old.excalidraw");
    std::fs::write(&old, OLD_BOARD).unwrap();
    let load = |path: String| CanvasLoadArgs {
        path: Some(path),
        scene: None,
        file: None,
        project: project_arg(&project),
    };

    let loaded = server
        .canvas_load(Parameters(load(format!(" {} ", old.display()))))
        .await;
    assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 9);

    if let Some(home) = std::env::var_os("HOME") {
        let name = "sawit-test-no-such-board.excalidraw";
        let missing = server
            .canvas_load(Parameters(load(format!("~/{name}"))))
            .await;
        assert_eq!(missing.is_error, Some(true));
        let expanded = Path::new(&home).join(name);
        assert!(
            said(&missing).contains(&expanded.display().to_string()),
            "{}",
            said(&missing)
        );
    }
}

#[test]
fn sawit_files_are_told_by_their_extension() {
    assert!(is_sawit_file("/a/b/board.sawit"));
    assert!(is_sawit_file("board.Sawit"));
    assert!(is_sawit_file("  ~/board.sawit "));
    assert!(!is_sawit_file("/a/b/board.excalidraw"));
    assert!(!is_sawit_file("sawit"));
    assert!(!is_sawit_file("/a/sawit/board"));
    // The backups Sawit keeps of a canvas file are canvas files too; an export of one isn't.
    assert!(is_sawit_file("/p/.nyawit/canvas.sawit.bak"));
    assert!(is_sawit_file("/p/.nyawit/canvas.sawit.bak.3"));
    assert!(is_sawit_file("/p/.nyawit/canvas.sawit.del.bak"));
    assert!(is_sawit_file("/p/design/home.sawit.del.bak.4"));
    assert!(is_sawit_file("/p/.nyawit/canvas.sawit.lossy.bak"));
    assert!(!is_sawit_file("/p/canvas.sawit.svg"));
    assert!(!is_sawit_file("/p/canvas.sawit.bak.x"));
    assert!(!is_sawit_file("/p/canvas.sawit.del.bak.x"));
    assert!(!is_sawit_file("/p/canvas.sawit.del"));
}

#[test]
fn a_path_for_the_app_is_absolute() {
    assert_eq!(absolute_path("/a/b.sawit"), "/a/b.sawit");
    let relative = absolute_path("b.sawit");
    assert!(Path::new(&relative).is_absolute(), "{relative}");
    assert!(relative.ends_with("/b.sawit"), "{relative}");
    if let Some(home) = std::env::var_os("HOME") {
        let home = Path::new(&home).join("b.sawit");
        assert_eq!(absolute_path("~/b.sawit"), home.to_string_lossy());
    }
}
