//! Opening a canvas file, or an Excalidraw file imported into the `.sawit` file beside it, and
//! what opening it says.

use super::*;

/// RELEASE-2: reading the system's fonts left AppKit's 160 MB symbol catalog resident for
/// the first half minute of every launch, with or without a canvas. They are read once a
/// canvas first opens now: starting the app reads none.
#[gpui_kit::test]
fn the_system_fonts_are_read_when_a_canvas_first_opens(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    cx.update(super::super::init);
    cx.run_until_parked();
    let reading = |cx: &mut TestAppContext| cx.update(|cx| cx.has_global::<SystemFonts>());
    assert!(!reading(cx), "starting the app read the system's fonts");
    let tmp = Scratch::new("system-fonts");
    let _view = cx.new(|cx| BoardView::open(tmp.0.join("canvas.sawit"), cx));
    assert!(reading(cx));
    cx.run_until_parked();
    assert!(figcraft_text::installed_fonts_known());
}

#[gpui_kit::test]
fn missing_canvas_opens_as_empty_board(cx: &mut TestAppContext) {
    let tmp = Scratch::new("missing");
    let path = tmp.0.join(".nyawit/canvas.sawit");
    let view = open(cx, &path);
    let kind = view.read_with(cx, |view, _| view.board().map(|board| board.kind()));
    assert_eq!(kind, Some(DocKind::Board));
    assert!(!path.exists());
    // Opening is no edit: nothing is written, not even the folder.
    view.update(cx, |view, _| view.flush());
    assert!(names(&tmp.0).is_empty(), "{:?}", names(&tmp.0));
}

#[gpui_kit::test]
fn corrupt_file_shows_error_and_leaves_file(cx: &mut TestAppContext) {
    let tmp = Scratch::new("corrupt");
    let path = tmp.0.join("canvas.sawit");
    std::fs::write(&path, b"not a canvas").unwrap();
    let view = open(cx, &path);
    assert!(view.read_with(cx, |view, _| view.board().is_none()));
    view.update(cx, |view, _| view.flush());
    assert_eq!(std::fs::read(&path).unwrap(), b"not a canvas");
    assert_eq!(names(&tmp.0), ["canvas.sawit"]);
}

fn path_of(view: &Entity<BoardView>, cx: &TestAppContext) -> PathBuf {
    view.read_with(cx, |view, _| view.path().to_path_buf())
}

#[gpui_kit::test]
fn excalidraw_file_opens_bound_to_sibling(cx: &mut TestAppContext) {
    let tmp = Scratch::new("import");
    let old = tmp.0.join("x.excalidraw");
    std::fs::write(&old, FIXTURE).unwrap();
    let view = import(cx, &old);
    let file = tmp.0.join("x.sawit");
    assert_eq!(path_of(&view, cx), file);
    // The scene is on the canvas. What the import left out is said of the file it is in.
    assert_eq!(layers(&view, cx).len(), 9);
    let toast = view.read_with(cx, |view, _| view.toast.clone());
    assert_eq!(toast.as_deref(), Some("x.excalidraw: skipped 1 embed"));
    // The import isn't on disk yet, so a flush writes it, with no edit, beside the scene.
    view.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&file), 9);
    assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
    assert_eq!(names(&tmp.0), ["x.excalidraw", "x.sawit"]);
}

#[gpui_kit::test]
fn an_excalidraw_file_with_its_sawit_file_beside_it_opens_that(cx: &mut TestAppContext) {
    let tmp = Scratch::new("import-again");
    let old = tmp.0.join("x.excalidraw");
    let file = tmp.0.join("x.sawit");
    std::fs::write(&old, FIXTURE).unwrap();
    // Saved there before, and edited since: a sticky, not the scene's nine layers.
    let mut board = BoardDoc::new_board("x").unwrap();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "edited"}))
        .unwrap();
    let bytes = write_board(&file, &board);
    let view = import(cx, &old);
    assert_eq!(path_of(&view, cx), file);
    assert_eq!(layers(&view, cx).len(), 1);
    assert!(view.read_with(cx, |view, _| view.toast.is_none()));
    // Opening it is no edit: neither file is written.
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    view.update(cx, |view, _| view.flush());
    assert!(std::fs::read(&file).unwrap() == bytes, "the file was saved");
    assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
}

#[gpui_kit::test]
fn an_excalidraw_file_that_cannot_be_imported_shows_error_and_makes_no_sawit_file(
    cx: &mut TestAppContext,
) {
    let tmp = Scratch::new("import-corrupt");
    let old = tmp.0.join("x.excalidraw");
    std::fs::write(&old, b"{ not json").unwrap();
    let view = import(cx, &old);
    assert!(view.read_with(cx, |view, _| view.board().is_none()));
    assert_eq!(path_of(&view, cx), tmp.0.join("x.sawit"));
    // Ops answer why, naming the file that couldn't be imported.
    let reply = run_op(&view, json!({"op": "get"}), cx);
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().expect("an error string");
    assert!(
        error.contains("x.excalidraw") && error.contains("isn't valid JSON"),
        "{error}"
    );
    // Nothing is written, and the file is as it was.
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    view.update(cx, |view, _| view.flush());
    assert_eq!(std::fs::read(&old).unwrap(), b"{ not json");
    assert_eq!(names(&tmp.0), ["x.excalidraw"]);
}

#[gpui_kit::test]
fn an_excalidraw_file_with_nothing_in_it_opens_blank(cx: &mut TestAppContext) {
    let tmp = Scratch::new("import-blank");
    let old = tmp.0.join("x.excalidraw");
    std::fs::write(&old, b" \n").unwrap();
    let view = import(cx, &old);
    let kind = view.read_with(cx, |view, _| view.board().map(|board| board.kind()));
    assert_eq!(kind, Some(DocKind::Board));
    assert!(layers(&view, cx).is_empty());
    assert!(view.read_with(cx, |view, _| view.toast.is_none()));
    // A blank board is no edit, as a new canvas isn't: it is written once there is one.
    view.update(cx, |view, _| view.flush());
    assert_eq!(names(&tmp.0), ["x.excalidraw"]);
    add_sticky(&view, cx);
    view.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&tmp.0.join("x.sawit")), 1);
    assert_eq!(std::fs::read(&old).unwrap(), b" \n");
}

#[gpui_kit::test]
fn a_migrated_canvas_is_written_once_and_then_opens_from_its_file(cx: &mut TestAppContext) {
    // A project with only its old canvas: the canvas opens it imported, and saves the import
    // with no edit, so that it is imported once (spec D5).
    let project = |test: &str| {
        let tmp = Scratch::new(test);
        let nyawit = tmp.0.join(".nyawit");
        std::fs::create_dir(&nyawit).unwrap();
        std::fs::write(nyawit.join("canvas.excalidraw"), FIXTURE).unwrap();
        (tmp, nyawit)
    };
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);

    // Painted: its first autosave writes canvas.sawit.
    let (_painted, nyawit) = project("migrate-painted");
    let path = nyawit.join("canvas.sawit");
    let (_, view) = open_in_window(cx, &path);
    next_frame(cx, &view, None);
    assert!(!path.exists(), "saved before the canvas was still");
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    // (A canvas file that isn't there would open as the old canvas again: the folder says
    // whether it was written.)
    assert_eq!(names(&nyawit), ["canvas.excalidraw", "canvas.sawit"]);
    assert_eq!(saved_layers(&path), 9);
    let old = std::fs::read(nyawit.join("canvas.excalidraw")).unwrap();
    assert_eq!(old, FIXTURE.as_bytes());
    // Opened again, it is read from that file, not imported again: the same layers, and the
    // import's note isn't said again.
    let again = cx.new(|cx| BoardView::open(path.clone(), cx));
    assert_eq!(layers(&again, cx).len(), 9);
    assert!(again.read_with(cx, |view, _| view.toast.is_none()));

    // Never painted (its tab closed, or the app quit, before a frame): a flush writes it.
    let (_unpainted, nyawit) = project("migrate-flush");
    let path = nyawit.join("canvas.sawit");
    let view = cx.new(|cx| BoardView::open(path.clone(), cx));
    view.update(cx, |view, _| view.flush());
    assert_eq!(names(&nyawit), ["canvas.excalidraw", "canvas.sawit"]);
    assert_eq!(saved_layers(&path), 9);
    let old = std::fs::read(nyawit.join("canvas.excalidraw")).unwrap();
    assert_eq!(old, FIXTURE.as_bytes());
}

#[test]
fn open_issues_read_as_one_line() {
    let path = Path::new("/p/.nyawit/canvas.sawit");
    assert_eq!(issues_line(path, &[]), None);
    assert_eq!(
        issues_line(path, &["an image was left out".into()]).as_deref(),
        Some("canvas.sawit: an image was left out")
    );
    let two = ["a layer was repaired".into(), "a font is missing".into()];
    assert_eq!(
        issues_line(path, &two).as_deref(),
        Some(
            "canvas.sawit: 2 problems were repaired or left out while opening it, \
                 e.g. a layer was repaired"
        )
    );
}
