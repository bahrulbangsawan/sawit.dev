//! A canvas following its file when it is renamed or moved, and saving it again where it went
//! (CANVAS-UI-3).

use super::*;

/// The path of the canvas's file.
fn bound_to(view: &Entity<BoardView>, cx: &TestAppContext) -> PathBuf {
    view.read_with(cx, |view, _| view.path().to_path_buf())
}

/// CANVAS-UI-3: the Files panel renames a canvas's file, and the workspace has the canvas
/// follow it ([`BoardView::rebind`]). Its edits are saved under the new name, and the old one
/// doesn't come back. Before, the canvas went on saving at the old path: the board forked
/// into two files, and the renamed one lacked every edit made since.
#[gpui_kit::test]
fn a_canvas_follows_its_file_when_it_is_renamed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("renamed-under-canvas");
    let (a, b) = (tmp.0.join("a.sawit"), tmp.0.join("b.sawit"));
    write_board(&a, &board_with(&["mine"]));
    let view = open(cx, &a);
    std::fs::rename(&a, &b).unwrap();
    view.update(cx, |view, cx| view.rebind(b.clone(), cx));
    assert_eq!(bound_to(&view, cx), b);
    add_sticky(&view, cx);
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    view.update(cx, |view, _| view.flush());
    assert_eq!(names(&tmp.0), ["b.sawit"]);
    assert_eq!(saved_layers(&b), 2);
    assert_eq!(view.read_with(cx, |view, _| view.toast.clone()), None);
    // Closing the tab writes nothing at the old name either.
    add_sticky(&view, cx);
    drop(view);
    cx.update(|_| {});
    assert_eq!(names(&tmp.0), ["b.sawit"]);
    assert_eq!(saved_layers(&b), 3);
}

/// CANVAS-UI-3 for a folder: the folder that holds the canvas's file is moved, and the canvas
/// follows the file into it. The guard against another app's changes goes with the file: a
/// write made to it after the move is still not saved over.
#[gpui_kit::test]
fn a_canvas_follows_its_file_when_its_folder_moves(cx: &mut TestAppContext) {
    let tmp = Scratch::new("folder-moved-under-canvas");
    let (sub, moved) = (tmp.0.join("sub"), tmp.0.join("moved"));
    std::fs::create_dir_all(&sub).unwrap();
    write_board(&sub.join("a.sawit"), &board_with(&["mine"]));
    let view = open(cx, &sub.join("a.sawit"));
    std::fs::rename(&sub, &moved).unwrap();
    let file = moved.join("a.sawit");
    view.update(cx, |view, cx| view.rebind(file.clone(), cx));
    add_sticky(&view, cx);
    view.update(cx, |view, _| view.flush());
    assert_eq!(names(&tmp.0), ["moved"]);
    assert_eq!(saved_layers(&file), 2);

    // Another app writes the file where it is now: the canvas's next save is refused.
    let theirs = write_board(&file, &board_with(&["theirs", "and more", "still more"]));
    add_sticky(&view, cx);
    view.update(cx, |view, _| view.flush());
    assert_eq!(
        std::fs::read(&file).unwrap(),
        theirs,
        "the file was saved over"
    );
}

/// CANVAS-UI-3, a file that went without the canvas being told: moved to the Trash, deleted,
/// or renamed or moved by another app. The next save writes it there again, as FigCraft's own
/// save does, rather than lose the edits, and says so in a toast: before, the Trash was
/// silently undone. The same for a project's `.nyawit/` deleted under its canvas.
#[gpui_kit::test]
fn a_canvas_whose_file_went_saves_it_again_and_says_so(cx: &mut TestAppContext) {
    let tmp = Scratch::new("file-went");
    let a = tmp.0.join("a.sawit");
    write_board(&a, &board_with(&["mine"]));
    let view = open(cx, &a);
    std::fs::remove_file(&a).unwrap();
    // Nothing changed: nothing is written.
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    assert!(!a.exists());
    let reply = run_op(&view, sticky_op(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    assert_eq!(saved_layers(&a), 2);
    let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
    assert!(
        toast.starts_with("a.sawit was no longer in ") && toast.contains("saved it there again"),
        "{toast}"
    );

    // A project's canvas whose `.nyawit/` was deleted.
    let nyawit = tmp.0.join(".nyawit");
    let canvas = nyawit.join("canvas.sawit");
    std::fs::create_dir_all(&nyawit).unwrap();
    write_board(&canvas, &board_with(&["mine"]));
    let view = open(cx, &canvas);
    std::fs::remove_dir_all(&nyawit).unwrap();
    run_op(&view, sticky_op(), cx);
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    assert_eq!(saved_layers(&canvas), 2);
    let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
    assert!(
        toast.starts_with("canvas.sawit was no longer in "),
        "{toast}"
    );
}

/// CANVAS-UI-3, a save under way when the file was renamed: written after the rename, it
/// made the old file again, with the newest document, and the renamed file has an older one.
/// The canvas, bound to the new name, saves the newest document there too.
#[gpui_kit::test]
fn a_save_that_lands_after_the_rename_reaches_the_new_name_too(cx: &mut TestAppContext) {
    let tmp = Scratch::new("rename-mid-save");
    let (a, b) = (tmp.0.join("a.sawit"), tmp.0.join("b.sawit"));
    let older = write_board(&a, &board_with(&["mine"]));
    let view = open(cx, &a);
    add_sticky(&view, cx);
    view.update(cx, |view, _| view.flush());
    // As if the rename came first: `b` is the file as it was, `a` the save written after.
    std::fs::write(&b, &older).unwrap();
    view.update(cx, |view, cx| view.rebind(b.clone(), cx));
    view.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&b), 2);
}
