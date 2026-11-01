//! When the canvas saves: after a flush, once it has been still, when its tab closes or the app
//! quits; and what it says when a save fails or would drop what the file holds.

use super::*;

#[gpui_kit::test]
fn edits_are_saved_after_flush(cx: &mut TestAppContext) {
    let tmp = Scratch::new("flush");
    let path = tmp.0.join(".nyawit/canvas.sawit");
    let view = open(cx, &path);
    add_sticky(&view, cx);
    view.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&path), 1);
}

#[gpui_kit::test]
fn edits_are_saved_once_the_canvas_has_been_still_for_the_delay(cx: &mut TestAppContext) {
    let tmp = Scratch::new("autosave");
    let path = tmp.0.join("canvas.sawit");
    let view = open(cx, &path);
    let step = Duration::from_millis(200);
    assert!(SAVE_DELAY > step && SAVE_DELAY < 2 * step);
    // Each frame after an edit (re)starts the wait: an edit within it starts it over.
    add_sticky(&view, cx);
    view.update(cx, |view, cx| view.autosave(cx));
    cx.executor().advance_clock(step);
    cx.run_until_parked();
    add_sticky(&view, cx);
    view.update(cx, |view, cx| view.autosave(cx));
    cx.executor().advance_clock(step);
    cx.run_until_parked();
    assert!(!path.exists(), "saved before the canvas was still");
    cx.executor().advance_clock(SAVE_DELAY - step);
    cx.run_until_parked();
    assert_eq!(saved_layers(&path), 2);
}

#[gpui_kit::test]
fn a_failed_save_shows_its_error_for_a_while(cx: &mut TestAppContext) {
    let tmp = Scratch::new("save-error");
    let blocker = tmp.0.join("not-a-folder");
    let path = blocker.join("canvas.sawit");
    let view = open(cx, &path);
    // The folder can't be made any more: a file has its name.
    std::fs::write(&blocker, b"x").unwrap();
    add_sticky(&view, cx);
    view.update(cx, |view, cx| view.autosave(cx));
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    let toast = view.read_with(cx, |view, _| view.toast.clone());
    assert!(
        toast.as_ref().is_some_and(|t| t.contains("canvas.sawit")),
        "{toast:?}"
    );
    assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
    // The document is kept, and a flush tries again.
    std::fs::remove_file(&blocker).unwrap();
    view.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&path), 1);
    cx.executor().advance_clock(TOAST_TIME);
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.toast.is_none()));
}

/// A 1 × 1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// A canvas file with a sticky, and an image whose bytes are damaged: the loader leaves the
/// image out, and says that a save drops it.
fn file_with_a_damaged_image() -> Vec<u8> {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "kept"}))
        .unwrap();
    let placed = board
        .execute("image.place", &json!({"data": PNG, "x": 0, "y": 300}))
        .unwrap();
    let hash = placed["image"].as_str().unwrap().to_string();
    let png = board.doc().unwrap().images[&hash].bytes.clone();
    let mut bytes = board.to_bytes().unwrap();
    let at = bytes
        .windows(png.len())
        .position(|w| w == png.as_slice())
        .unwrap();
    bytes[at + png.len() / 2] ^= 0xFF;
    bytes
}

#[gpui_kit::test]
fn a_lossy_open_says_what_saving_drops(cx: &mut TestAppContext) {
    let tmp = Scratch::new("lossy");
    let path = tmp.0.join("canvas.sawit");
    std::fs::write(&path, file_with_a_damaged_image()).unwrap();
    let view = open(cx, &path);
    let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
    assert!(toast.starts_with("canvas.sawit: "), "{toast}");
    assert!(toast.contains("saving this file again drops it"), "{toast}");
    cx.executor().advance_clock(TOAST_TIME);
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.toast.is_none()));
}

#[gpui_kit::test]
fn an_in_place_save_keeps_the_files_version_history(cx: &mut TestAppContext) {
    let tmp = Scratch::new("history");
    let path = tmp.0.join("canvas.sawit");
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "then"}))
        .unwrap();
    board
        .execute("file.saveVersion", &json!({"name": "Then"}))
        .unwrap();
    write_board(&path, &board);
    let view = open(cx, &path);
    // The canvas keeps it, so opening has nothing to say, and keeps no copy.
    assert!(view.read_with(cx, |view, _| view.toast.is_none()));
    let edit = json!({"op": "command", "command": "board.sticky", "params": {"x": 300, "y": 0}});
    let reply = run_op(&view, edit, cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    view.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&path), 2);
    let saved = store::open(&path).unwrap();
    let versions: Vec<String> = (saved.session.ds().unwrap().versions.iter())
        .map(|version| version.name.clone())
        .collect();
    assert_eq!(versions, ["Then"]);
    assert_eq!(names(&tmp.0), ["canvas.sawit"]);
}

#[gpui_kit::test]
fn a_version_shown_in_place_of_the_document_is_not_saved(cx: &mut TestAppContext) {
    let tmp = Scratch::new("preview");
    let path = tmp.0.join("canvas.sawit");
    // A canvas with a sticky, and a version from when it was blank.
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    board
        .execute("file.saveVersion", &json!({"name": "Blank"}))
        .unwrap();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "now"}))
        .unwrap();
    let bytes = write_board(&path, &board);
    let view = open(cx, &path);
    // An agent shows the blank version in its place: the canvas shows it, and that is no
    // edit. Nothing is saved, so the file keeps the document, and no backup of a canvas
    // being emptied is made either.
    let preview = json!({"op": "command", "command": "version.preview", "params": {"index": 0}});
    let reply = run_op(&view, preview, cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(layers(&view, cx).is_empty());
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    view.update(cx, |view, _| view.flush());
    assert!(std::fs::read(&path).unwrap() == bytes, "the file was saved");
    assert_eq!(names(&tmp.0), ["canvas.sawit"]);
}

#[gpui_kit::test]
fn a_file_with_images_left_in_it_is_not_saved_over(cx: &mut TestAppContext) {
    let tmp = Scratch::new("images-left");
    let path = tmp.0.join("canvas.sawit");
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "kept"}))
        .unwrap();
    let bytes = write_board(&path, &board);
    let view = open(cx, &path);
    // As FigCraft's loader records a file whose images it didn't all read, past what it reads
    // from one file (sawit-board's tests open such a file, of 20 001 images): three of them.
    view.update(cx, |view, _| {
        let session = &mut view.board_mut().unwrap().session;
        let ds = session.docs.remove(0);
        let ds = ds.with_images_left_in_file(&path.to_string_lossy(), 3);
        session.docs.insert(0, ds);
    });
    // An edit isn't saved over the file, which would delete them, and the canvas says so in
    // FigCraft's words. Closing the tab doesn't save it either.
    let reply = run_op(&view, sticky_op(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
    assert!(
        toast.starts_with("canvas wasn't saved: 3 of its images weren't loaded"),
        "{toast}"
    );
    view.update(cx, |view, _| view.flush());
    assert!(std::fs::read(&path).unwrap() == bytes, "the file was saved");
    assert_eq!(names(&tmp.0), ["canvas.sawit"]);
}

/// Seams m7: a canvas test leaves no folder behind. Its view outlives the test's last line:
/// `#[gpui_kit::test]` drops it after, and the closing canvas saves what it hadn't saved yet.
/// That save made the folder again once the test's scratch had removed it, and nothing removed
/// it then: a run of the app's tests left 97 canvas folders in the temp dir. Here a test runs
/// as `#[gpui_kit::test]` runs one (its body, then the harness's ending, as the macro writes
/// them), on a thread of its own, as libtest runs each test.
#[test]
fn a_canvas_test_leaves_no_folder_behind() {
    let dir = std::thread::spawn(|| {
        let made = std::sync::Mutex::new(None);
        gpui_kit::run_test(
            1,
            &[],
            0,
            &mut |dispatcher, _| {
                let mut cx = TestAppContext::build(dispatcher.clone(), None);
                {
                    // The test's body: it ends with an edit not saved yet.
                    let tmp = Scratch::new("leaves-nothing");
                    let path = tmp.0.join("canvas.sawit");
                    write_board(&path, &board_with(&["mine"]));
                    let view = open(&mut cx, &path);
                    add_sticky(&view, &mut cx);
                    *made.lock().unwrap() = Some(tmp.0.clone());
                }
                // The harness's ending: the view goes, and saves the sticky as it closes.
                cx.run_until_parked();
                cx.update(|cx| {
                    cx.background_executor().forbid_parking();
                    cx.quit();
                });
                cx.run_until_parked();
                drop(cx);
                dispatcher.drain_tasks();
            },
            None,
        );
        made.into_inner().unwrap().expect("the test ran")
    })
    .join()
    .expect("the test's thread panicked");
    assert!(!dir.exists(), "{} was left behind", dir.display());
}

#[gpui_kit::test]
fn an_op_is_saved_without_a_paint(cx: &mut TestAppContext) {
    // No window: the canvas is never painted, as when an agent edits a covered window, whose
    // display link has stopped. The edit must reach the file all the same.
    let tmp = Scratch::new("op-saves");
    let path = tmp.0.join("canvas.sawit");
    let view = open(cx, &path);
    let step = SAVE_DELAY / 2;

    // Ops that change nothing, or fail, leave nothing to save.
    run_op(&view, json!({"op": "get"}), cx);
    run_op(&view, json!({"op": "command", "command": "no.such"}), cx);
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    assert!(!path.exists(), "saved with nothing to save");

    let reply = run_op(&view, sticky_op(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.executor().advance_clock(step);
    cx.run_until_parked();
    assert!(!path.exists(), "saved before the canvas was still");
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    assert_eq!(saved_layers(&path), 1);

    // Each op after that is saved the same way.
    run_op(&view, sticky_op(), cx);
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    assert_eq!(saved_layers(&path), 2);
}

#[gpui_kit::test]
fn an_edit_in_the_window_is_saved_once_the_painted_canvas_has_been_still(cx: &mut TestAppContext) {
    // An edit made in the window (a drag, a key, typing) arrives with no op: the canvas only
    // redraws, and its paint is what starts the save.
    let tmp = Scratch::new("painted-save");
    let path = tmp.0.join("canvas.sawit");
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (_, view) = open_in_window(cx, &path);
    next_frame(cx, &view, None);
    view.update(cx, |view, cx| {
        let sticky = json!({"x": 0, "y": 0, "text": "drawn"});
        view.board_mut()
            .unwrap()
            .execute("board.sticky", &sticky)
            .unwrap();
        cx.notify();
    });
    cx.run_until_parked();
    cx.executor().advance_clock(SAVE_DELAY / 2);
    cx.run_until_parked();
    assert!(!path.exists(), "saved before the canvas was still");
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    assert_eq!(saved_layers(&path), 1);
}

#[gpui_kit::test]
fn closing_the_tab_saves_what_is_not_on_disk(cx: &mut TestAppContext) {
    let tmp = Scratch::new("drop-saves");
    let path = tmp.0.join("canvas.sawit");
    let view = open(cx, &path);
    // An edit that no paint, op or wait has saved.
    add_sticky(&view, cx);
    assert!(!path.exists());
    // The tab closes: the workspace lets go of the view, and the app drops it.
    drop(view);
    cx.update(|_| {});
    assert_eq!(saved_layers(&path), 1);
}

#[gpui_kit::test]
fn quitting_saves_what_is_not_on_disk(cx: &mut TestAppContext) {
    let tmp = Scratch::new("quit-saves");
    let path = tmp.0.join("canvas.sawit");
    let view = open(cx, &path);
    add_sticky(&view, cx);
    assert!(!path.exists());
    // ⌘Q: the app quits without dropping its views, and asks each to finish first.
    cx.quit();
    assert_eq!(saved_layers(&path), 1);
    assert!(view.read_with(cx, |view, _| view.board().is_some()));
}
