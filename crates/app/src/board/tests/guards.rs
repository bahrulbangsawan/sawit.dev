//! Saves refused over what another app or window wrote to the file, and the edits a canvas that
//! couldn't save them keeps in a copy (CANVAS-CORE-4, CANVAS-CORE-5).

use super::*;

/// What a save over a file that another app or window wrote to since says, after the file's
/// name: FigCraft's sentence, with Sawit's way out.
const CHANGED_ON_DISK: &str = "wasn't saved: the file changed on disk since it was opened or \
        saved here (another app or window wrote to it), and saving would replace those changes. \
        Save it under another name to keep this version, or close its tab and open the file again \
        to load those changes.";

#[gpui_kit::test]
fn a_file_changed_on_disk_since_it_opened_is_not_saved_over(cx: &mut TestAppContext) {
    let tmp = Scratch::new("changed-on-disk");
    let path = tmp.0.join("design.sawit");
    write_board(&path, &board_with(&["mine"]));
    let view = open(cx, &path);
    // Another app (FigCraft, figcraft-cli, git) writes the file while its canvas is open.
    let theirs = write_board(&path, &board_with(&["theirs", "and more"]));
    let kept_theirs = || std::fs::read(&path).unwrap() == theirs;
    // An edit isn't saved over what it wrote, by a flush or the autosave, which says why.
    add_sticky(&view, cx);
    view.update(cx, |view, _| view.flush());
    assert!(kept_theirs(), "the file was saved over");
    let reply = run_op(&view, sticky_op(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
    assert_eq!(toast.to_string(), format!("design {CHANGED_ON_DISK}"));
    assert!(kept_theirs(), "the file was saved over");
    // Closing the tab doesn't save over it either. It keeps the canvas's edits in a copy beside
    // it, where they used to be in no file (CANVAS-CORE-4).
    drop(view);
    cx.update(|_| {});
    assert!(kept_theirs(), "the file was saved over");
    assert_eq!(unsaved_copies(&tmp.0, "design"), [3]);
}

/// The copies of canvas `stem`'s unsaved edits in `dir` ([`BoardView::keep_unsaved`]): how many
/// layers each holds.
fn unsaved_copies(dir: &Path, stem: &str) -> Vec<usize> {
    let copies = names(dir)
        .into_iter()
        .filter(|name| name.starts_with(&format!("{stem} (unsaved ")) && name.ends_with(".sawit"));
    copies.map(|name| saved_layers(&dir.join(name))).collect()
}

/// CANVAS-CORE-4: a canvas whose folder became read-only can't save its edits, nor make a
/// copy of them beside its file: closing it keeps them in `unsaved/` in Sawit's own folder.
#[gpui_kit::test]
fn closing_a_canvas_whose_folder_is_read_only_keeps_its_edits_in_sawits_folder(
    cx: &mut TestAppContext,
) {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = Scratch::new("read-only-unsaved");
    let folder = tmp.0.join("designs");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("locked.sawit");
    write_board(&path, &board_with(&["mine"]));
    let view = open(cx, &path);
    let mode = |mode| std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(&folder, mode(0o555)).unwrap();
    add_sticky(&view, cx);
    let kept = view.update(cx, |view, _| {
        assert!(view.save_now().is_err(), "the save went through");
        view.keep_unsaved(&tmp.0.join("app"))
    });
    std::fs::set_permissions(&folder, mode(0o755)).unwrap();
    let kept = kept.unwrap().expect("no copy");
    assert_eq!(kept.parent().unwrap(), tmp.0.join("app").join("unsaved"));
    assert_eq!(saved_layers(&kept), 2);
    assert_eq!(names(&folder), ["locked.sawit"]);
    assert_eq!(saved_layers(&path), 1);
    // Kept once: the same edits again make no second copy.
    let again = view.update(cx, |view, _| view.keep_unsaved(&tmp.0.join("app")));
    assert_eq!(again, Ok(None));
}

/// CANVAS-CORE-4: quitting with edits the canvas couldn't save (another app wrote the file)
/// keeps them in a copy beside the file; it used to drop them with a line in the log.
#[gpui_kit::test]
fn quitting_keeps_edits_it_couldnt_save_in_a_copy(cx: &mut TestAppContext) {
    let tmp = Scratch::new("quit-unsaved");
    let path = tmp.0.join("plan.sawit");
    write_board(&path, &board_with(&["mine"]));
    let view = open(cx, &path);
    let theirs = write_board(&path, &board_with(&["theirs"]));
    add_sticky(&view, cx);
    cx.quit();
    assert!(
        std::fs::read(&path).unwrap() == theirs,
        "the file was saved over"
    );
    assert_eq!(unsaved_copies(&tmp.0, "plan"), [2]);
    // The view, dropped after, keeps no second copy of the same edits.
    drop(view);
    cx.update(|_| {});
    assert_eq!(unsaved_copies(&tmp.0, "plan"), [2]);
}

#[gpui_kit::test]
fn of_two_canvases_of_one_file_the_second_to_save_is_refused(cx: &mut TestAppContext) {
    // Two windows have the file open, and each canvas would save over the other's edits.
    let tmp = Scratch::new("two-canvases");
    let path = tmp.0.join("canvas.sawit");
    write_board(&path, &board_with(&["shared"]));
    let first = open(cx, &path);
    let second = cx.new(|cx| BoardView::open(path.clone(), cx));
    add_sticky(&first, cx);
    first.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&path), 2);
    let saved = std::fs::read(&path).unwrap();
    // The second's edit would replace the first's: it is refused, and says so.
    let reply = run_op(&second, sticky_op(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    let toast = second.read_with(cx, |view, _| view.toast.clone()).unwrap();
    assert_eq!(toast.to_string(), format!("canvas {CHANGED_ON_DISK}"));
    second.update(cx, |view, _| view.flush());
    assert!(
        std::fs::read(&path).unwrap() == saved,
        "the first's edit was replaced"
    );
    // The first goes on saving its own edits.
    add_sticky(&first, cx);
    first.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&path), 3);
    assert!(first.read_with(cx, |view, _| view.toast.is_none()));
}

/// CANVAS-CORE-5: two windows with no project share Sawit's own `canvas.sawit`, and
/// `sawit -n` puts one project in two; each window has its own canvas of the file. With no
/// file there when they opened, the second to save replaced the first's board without a word,
/// and the first's next save was refused: its work was in no file. The second's save is
/// refused now, and the first goes on saving.
#[gpui_kit::test]
fn of_two_canvases_of_a_file_not_made_yet_the_second_to_save_is_refused(cx: &mut TestAppContext) {
    let tmp = Scratch::new("two-canvases-new");
    let path = tmp.0.join("canvas.sawit");
    let first = open(cx, &path);
    let second = cx.new(|cx| BoardView::open(path.clone(), cx));
    add_sticky(&first, cx);
    first.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&path), 1);
    let saved = std::fs::read(&path).unwrap();
    // The second's edit would replace the first's: it is refused, and says so.
    let reply = run_op(&second, sticky_op(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    let toast = second.read_with(cx, |view, _| view.toast.clone());
    let toast = toast.expect("no toast").to_string();
    assert!(toast.starts_with("canvas wasn't saved: "), "{toast}");
    second.update(cx, |view, _| view.flush());
    assert!(
        std::fs::read(&path).unwrap() == saved,
        "the first's board was replaced"
    );
    // The first goes on saving its own edits.
    add_sticky(&first, cx);
    first.update(cx, |view, _| view.flush());
    assert_eq!(saved_layers(&path), 2);
    assert!(first.read_with(cx, |view, _| view.toast.is_none()));
}

#[gpui_kit::test]
fn a_board_with_no_file_at_open_is_written_by_its_first_flush_and_guarded_after(
    cx: &mut TestAppContext,
) {
    // An `.excalidraw` file imported, and a project's old canvas migrated: neither `.sawit`
    // file is there yet, and nothing else makes one meanwhile (when something does, the save
    // is refused: CANVAS-CORE-5).
    let imported = Scratch::new("guard-import");
    let old = imported.0.join("x.excalidraw");
    std::fs::write(&old, FIXTURE).unwrap();
    let migrated = Scratch::new("guard-migrate");
    let nyawit = migrated.0.join(".nyawit");
    std::fs::create_dir(&nyawit).unwrap();
    std::fs::write(nyawit.join("canvas.excalidraw"), FIXTURE).unwrap();
    let canvas = nyawit.join("canvas.sawit");
    let views = [
        (import(cx, &old), imported.0.join("x.sawit")),
        (cx.new(|cx| BoardView::open(canvas.clone(), cx)), canvas),
    ];
    for (view, file) in &views {
        assert!(!file.exists());
        view.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(file), 9);
        // From then on its own saves go through, and one over another app's change doesn't.
        add_sticky(view, cx);
        view.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(file), 10);
        let theirs = write_board(file, &board_with(&["theirs"]));
        add_sticky(view, cx);
        view.update(cx, |view, _| view.flush());
        assert!(
            std::fs::read(file).unwrap() == theirs,
            "{file:?} was saved over"
        );
    }
}
