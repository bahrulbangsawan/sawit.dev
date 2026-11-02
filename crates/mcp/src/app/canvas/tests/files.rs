//! A canvas file an agent names with `file`: a design file or an `.excalidraw` scene, with Sawit
//! closed and running, and refused when it is no canvas file or the app is too old for it.

use std::os::unix::net::UnixListener;
use std::sync::mpsc::Receiver;

use rmcp::handler::server::wrapper::Parameters;
use sawit_core::app_control::{Command, Request};
use sawit_core::instances::AppPaths;
use sawit_core::update::Version;

use super::*;
use crate::app::canvas::dispatch::{
    APP_WITH_CANVAS_FILES, Listener, app_reads_canvas_files, refuse_an_app_without_canvas_files,
    same_listener,
};
use crate::app::canvas::files::canvas_file;
use crate::app::canvas::{
    CanvasDeleteArgs, CanvasElementsArgs, CanvasExportArgs, CanvasTargetArgs,
};
use crate::app::tests::{answer, fake_app_answering, native, said};

/// A design file at `path`, with nothing on it yet: frames, text and vectors go on it, a
/// board's stickies and shapes don't.
fn write_design(path: &Path) {
    let bytes = figcraft_format::save_bytes(&Default::default(), None).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The canvas tools work on any canvas file an agent names with `file`, not only on the
/// project's board: a design file kept in the repository could be edited only by hand. With
/// Sawit closed, they draw on it, read it, export it and delete from it there, it stays a
/// design file, its backups are its own, and the project's canvas is left alone.
#[tokio::test]
async fn the_canvas_tools_work_on_a_design_file_named_by_file_with_sawit_closed() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let design = project.join("design").join("home.sawit");
    write_design(&design);
    let file = || Some(format!(" {} ", design.display()));

    let added = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![
                json!({"type": "frame", "x": 0, "y": 0, "width": 400, "height": 300}),
                json!({"type": "text", "id": "hero", "x": 20, "y": 20, "text": "Hero"}),
            ],
            file: file(),
            project: project_arg(&project),
            work: None,
        }))
        .await;
    assert_eq!(answer(&added)["added"], json!(2), "{}", said(&added));
    let got = server
        .canvas_get_tool(CanvasTargetArgs {
            file: file(),
            project: project_arg(&project),
        })
        .await;
    let got = answer(&got);
    let texts: Vec<&str> = (got["elements"].as_array().unwrap().iter())
        .filter_map(|layer| layer["text"].as_str())
        .collect();
    assert_eq!(texts, ["Hero"]);
    let saved = BoardDoc::from_bytes(&std::fs::read(&design).unwrap()).unwrap();
    assert_eq!(saved.kind(), sawit_board::DocKind::Design);
    assert!(
        !board_file(&project).exists(),
        "the project's canvas was made"
    );

    // An export with no path goes to the project's exports, named after the file.
    let exported = server
        .canvas_export(Parameters(CanvasExportArgs {
            format: None,
            output_path: None,
            file: file(),
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(exported.is_error, Some(true), "{}", said(&exported));
    let exports = names(&project.join(".nyawit").join("exports"));
    assert!(
        exports.len() == 1 && exports[0].starts_with("home-") && exports[0].ends_with(".svg"),
        "{exports:?}"
    );

    // A deletion keeps the design file's board beside it.
    let deleted = server
        .canvas_delete(Parameters(CanvasDeleteArgs {
            ids: vec!["hero".into()],
            file: file(),
            project: project_arg(&project),
            work: None,
        }))
        .await;
    assert_eq!(answer(&deleted)["deleted"], json!(1), "{}", said(&deleted));
    assert!(!texts_in(&design).contains("Hero"));
    assert!(texts_of(&deletion_backups_of(&design)[0]).contains("Hero"));
    assert_eq!(
        names(&design.with_file_name("")),
        ["home.sawit", "home.sawit.del.bak"]
    );
}

/// An `.excalidraw` file an agent names is imported once, into the `.sawit` file beside
/// it, as the app opens one (spec D14): reads import it and write nothing, the first edit
/// writes the `.sawit` file, and from then on that file is the canvas. The `.excalidraw`
/// file is never touched.
#[tokio::test]
async fn an_excalidraw_file_named_by_file_is_imported_once_into_the_sawit_file_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let sketches = project.join("sketches");
    let scene = sketches.join("flow.excalidraw");
    std::fs::create_dir_all(&sketches).unwrap();
    std::fs::write(&scene, OLD_BOARD).unwrap();
    let get = || {
        server.canvas_get_tool(CanvasTargetArgs {
            file: Some(scene.display().to_string()),
            project: project_arg(&project),
        })
    };

    let got = answer(&get().await);
    assert_eq!(got["elements"].as_array().unwrap().len(), 9);
    assert_eq!(names(&sketches), ["flow.excalidraw"], "a read wrote");

    let added = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![json!({"type": "sticky", "x": 900, "y": 0, "text": "added"})],
            file: Some(scene.display().to_string()),
            project: project_arg(&project),
            work: None,
        }))
        .await;
    assert_ne!(added.is_error, Some(true), "{}", said(&added));
    assert_eq!(names(&sketches), ["flow.excalidraw", "flow.sawit"]);
    assert_eq!(layers_in(&sketches.join("flow.sawit")).len(), 10);
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), OLD_BOARD);

    // Imported once: the scene changing on disk changes the canvas no more.
    std::fs::write(&scene, r#"{"type": "excalidraw", "elements": []}"#).unwrap();
    assert_eq!(
        answer(&get().await)["elements"].as_array().unwrap().len(),
        10
    );
    assert!(
        !board_file(&project).exists(),
        "the project's canvas was made"
    );
}

/// With Sawit running, a canvas tool's `file` goes to the app with the op, as the file it is
/// on disk, for the app to run the op on that file's canvas tab, once the app's version says
/// it runs it there; without one the op is the project canvas's, as before. A file that is no
/// canvas file is refused here, and the app isn't asked.
#[tokio::test]
async fn a_running_app_is_sent_the_canvas_file_an_agent_names() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let design = project.join("home.sawit");
    write_design(&design);
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&project, &link).unwrap();
    let state = json!({"version": env!("CARGO_PKG_VERSION"), "windows": []});
    let asked = fake_app_answering(
        &server.paths,
        vec![
            state,
            json!({"ok": true, "elements": []}),
            json!({"ok": true}),
        ],
    );
    let sent = |asked: &Receiver<Request>| {
        let request = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
        match request.command {
            Command::Canvas { op, file } => (op, file),
            other => panic!("{other:?}"),
        }
    };

    let got = server
        .canvas_get_tool(CanvasTargetArgs {
            file: Some(link.join("home.sawit").display().to_string()),
            project: None,
        })
        .await;
    assert_ne!(got.is_error, Some(true), "{}", said(&got));
    let real = design.canonicalize().unwrap();
    let first = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
    assert_eq!(first.command, Command::State, "the app's version first");
    assert_eq!(sent(&asked), (json!({"op": "get"}), Some(real)));
    let cleared = server
        .canvas_clear(Parameters(CanvasTargetArgs {
            file: Some("  ".into()),
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    assert_eq!(sent(&asked), (json!({"op": "clear"}), None));

    let refused = server
        .canvas_clear(Parameters(CanvasTargetArgs {
            file: Some(project.join("notes.md").display().to_string()),
            project: project_arg(&project),
        }))
        .await;
    assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
    assert!(
        said(&refused).contains("isn't a canvas file"),
        "{}",
        said(&refused)
    );
    assert!(asked.recv_timeout(Duration::from_millis(300)).is_err());
}

/// An app older than this sawit-mcp ignores `file`, a field it doesn't know, and runs the op
/// on the project's canvas: a canvas_clear meant for a design file cleared the project's
/// board. Such an app is asked its version, and sent nothing else; an op on the project's
/// canvas, which names no file, goes to it as it always did.
#[tokio::test]
async fn an_app_too_old_to_know_a_canvas_file_is_never_sent_an_op_on_one() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let design = project.join("home.sawit");
    write_design(&design);
    let state = json!({"version": "0.0.6", "windows": []});
    let asked = fake_app_answering(&server.paths, vec![state, json!({"ok": true})]);
    let clear = |file: Option<String>| CanvasTargetArgs {
        file,
        project: project_arg(&project),
    };

    let refused = server
        .canvas_clear(Parameters(clear(Some(design.display().to_string()))))
        .await;
    assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
    let says = said(&refused);
    for said in ["Nothing was changed", "(0.0.6)", "home.sawit", "Quit Sawit"] {
        assert!(says.contains(said), "no {said:?} in {says}");
    }
    let request = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
    assert_eq!(request.command, Command::State);

    let cleared = server.canvas_clear(Parameters(clear(None))).await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    let request = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
    assert_eq!(
        request.command,
        Command::Canvas {
            op: json!({"op": "clear"}),
            file: None
        }
    );
}

/// The running app's version is asked once per app, not before every op on a canvas file: its
/// answer is the app's whole state, gathered from every window on its UI thread. An app opened
/// again listens on a socket of its own, and is asked again.
#[tokio::test]
async fn the_running_apps_version_is_asked_once_per_app() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let design = project.join("home.sawit");
    write_design(&design);
    let state = json!({"version": env!("CARGO_PKG_VERSION"), "windows": []});
    let got = json!({"ok": true, "elements": []});
    let get = || {
        server.canvas_get_tool(CanvasTargetArgs {
            file: Some(design.display().to_string()),
            project: None,
        })
    };
    let sent = |asked: &Receiver<Request>| -> Vec<Command> {
        asked.try_iter().map(|request| request.command).collect()
    };

    let asked = fake_app_answering(&server.paths, vec![state.clone(), got.clone(), got.clone()]);
    for _ in 0..2 {
        let answered = get().await;
        assert_ne!(answered.is_error, Some(true), "{}", said(&answered));
    }
    let commands = sent(&asked);
    assert!(
        matches!(
            commands.as_slice(),
            [
                Command::State,
                Command::Canvas { .. },
                Command::Canvas { .. }
            ]
        ),
        "{commands:?}"
    );

    // Sawit quit and opened again, on a socket of its own at that path.
    std::fs::remove_file(server.paths.cli_socket()).unwrap();
    let asked = fake_app_answering(&server.paths, vec![state, got]);
    let answered = get().await;
    assert_ne!(answered.is_error, Some(true), "{}", said(&answered));
    let commands = sent(&asked);
    assert!(
        matches!(
            commands.as_slice(),
            [Command::State, Command::Canvas { .. }]
        ),
        "{commands:?}"
    );
}

/// A version is kept, and used, only under the socket file that was at the path both before the
/// connection was made and after it. When another app's socket took the path in between
/// (`cli::serve` unlinks the old file, then binds its own, so two apps starting at once can swap
/// it), the connection may have reached either app: the version is asked, kept under neither,
/// and asked again next time.
#[test]
fn a_version_is_kept_only_under_a_socket_that_stayed_put() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: dir.path().join("home"),
    };
    let state = json!({"version": env!("CARGO_PKG_VERSION"), "windows": []});
    let asked = fake_app_answering(&paths, vec![state.clone(), state.clone(), state]);
    // Another app's socket file, made as an app starting at once would make it.
    let other = dir.path().join("other.sock");
    let _other = UnixListener::bind(&other).unwrap();
    let ours = Listener::of(&paths.cli_socket()).unwrap();
    let theirs = Listener::of(&other).unwrap();
    assert_eq!(same_listener(Some(ours), Some(ours)), Some(ours));
    for (before, after) in [
        (Some(ours), Some(theirs)),
        (None, Some(ours)),
        (Some(ours), None),
    ] {
        assert_eq!(
            same_listener(before, after),
            None,
            "{before:?} then {after:?}"
        );
    }

    let request = Request {
        project: None,
        agent_folder: None,
        agent: None,
        command: Command::State,
    };
    let file = dir.path().join("home.sawit");
    let refuse =
        |listener| refuse_an_app_without_canvas_files(&paths, &mut None, listener, &request, &file);
    let asked_again = |asked: &Receiver<Request>| asked.recv_timeout(Duration::from_secs(5)).ok();
    // The socket changed under the connection: asked each time, and kept under neither.
    for _ in 0..2 {
        refuse(same_listener(Some(ours), Some(theirs))).unwrap();
        assert_eq!(asked_again(&asked).map(|r| r.command), Some(Command::State));
    }
    // It stayed: asked once, then known.
    refuse(Some(ours)).unwrap();
    assert_eq!(asked_again(&asked).map(|r| r.command), Some(Command::State));
    refuse(Some(ours)).unwrap();
    assert!(
        asked.recv_timeout(Duration::from_millis(300)).is_err(),
        "asked again"
    );
}

/// The apps that run an op on the canvas file it names: 0.0.8 and later, and any of this
/// sawit-mcp's own version or later, a build from before the release that brings `file`. One
/// older than both, or that names no version, doesn't.
#[test]
fn an_app_reads_canvas_files_from_the_version_that_brings_them() {
    for version in [
        "0.0.8",
        "v0.0.9",
        "0.1.0",
        "1.0.0",
        env!("CARGO_PKG_VERSION"),
    ] {
        assert!(app_reads_canvas_files(version), "{version}");
    }
    for version in ["0.0.6", "0.0", "an unknown version", ""] {
        assert!(!app_reads_canvas_files(version), "{version}");
    }
    if Version::current() >= APP_WITH_CANVAS_FILES {
        assert!(!app_reads_canvas_files("0.0.7"));
    }
}

/// `file` names a canvas file by its absolute path (or `~/…`): a `.sawit` file that is
/// there, or a new one in a folder that is (a new board, made by the first edit), or an
/// `.excalidraw` file that is there or was imported. Anything else is refused before a tab
/// opens or a file is made, and says why.
#[test]
fn a_canvas_file_is_an_absolute_sawit_or_excalidraw_file() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().canonicalize().unwrap();
    let design = real.join("home.sawit");
    write_design(&design);
    let file = |path: &Path| canvas_file(Some(&path.display().to_string()));
    assert_eq!(canvas_file(None), Ok(None));
    assert_eq!(canvas_file(Some("  ")), Ok(None));
    assert_eq!(file(&design), Ok(Some(design.clone())));
    let new = real.join("new.sawit");
    assert_eq!(file(&new), Ok(Some(new.clone())));
    assert!(!new.exists());
    // An `.excalidraw` file whose scene went, once imported, is the file it was imported into.
    write_board(&real.join("gone.sawit"), 1);
    let gone = real.join("gone.excalidraw");
    assert_eq!(file(&gone), Ok(Some(gone.clone())));
    if let Some(home) = std::env::var_os("HOME") {
        let home = Path::new(&home).canonicalize().unwrap();
        let named = canvas_file(Some("~/sawit-test-no-such-folder/x.sawit"));
        let error = named.unwrap_err();
        assert!(error.contains(&home.display().to_string()), "{error}");
    }

    for (named, says) in [
        ("home.sawit".to_string(), "must be an absolute path"),
        (
            real.join("notes.md").display().to_string(),
            "isn't a canvas file",
        ),
        (
            real.join("canvas.sawit.bak").display().to_string(),
            "isn't a canvas file",
        ),
        (
            real.join("no/such/folder/x.sawit").display().to_string(),
            "nor is the folder a new canvas file would go in",
        ),
        (
            real.join("missing.excalidraw").display().to_string(),
            "isn't there: name an .excalidraw file to import",
        ),
    ] {
        let refused = canvas_file(Some(&named)).unwrap_err();
        assert!(refused.contains(says), "{named}: {refused}");
    }
    // A folder by that name is no canvas file, and neither is a link left at the name.
    std::fs::create_dir(real.join("folder.sawit")).unwrap();
    std::os::unix::fs::symlink(real.join("nowhere"), real.join("dangling.sawit")).unwrap();
    for name in ["folder.sawit", "dangling.sawit"] {
        let refused = file(&real.join(name)).unwrap_err();
        assert!(refused.contains("isn't a canvas file"), "{name}: {refused}");
    }
}
