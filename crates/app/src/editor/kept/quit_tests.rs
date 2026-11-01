//! Tests of unsaved changes kept by a quit that can't ask, and put back at the next launch.

use std::path::{Path, PathBuf};

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions,
    px, size,
};

use super::super::testing::{buffer, init, open_editor, scratch_dir, type_into, wait_until};
use super::super::{FileEditor, mtime};
use super::{Kept, KeptFolder, all, keep};

/// A folder for the test, with `notes.txt` ("on disk\n") in it, kept changes going to its
/// `kept/`.
fn setup(cx: &mut TestAppContext, name: &str) -> (PathBuf, PathBuf) {
    cx.update(init);
    let dir = scratch_dir(name);
    let folder = dir.join("kept");
    cx.update(|cx| cx.set_global(KeptFolder(folder.clone())));
    std::fs::write(dir.join("notes.txt"), "on disk\n").unwrap();
    (dir.join("notes.txt"), folder)
}

/// `path`'s unsaved `text`, made to `base`.
fn kept(path: &Path, text: &str, base: Option<&str>) -> Kept {
    Kept {
        path: path.to_path_buf(),
        text: text.into(),
        base: base.map(Into::into),
    }
}

/// The changes kept in `folder`, oldest first.
fn kept_in(folder: &Path) -> Vec<Kept> {
    all(folder).into_iter().map(|(_, kept)| kept).collect()
}

/// Keeps `change` in `folder`, as a quit does, then does what the next launch does with it:
/// opens its file in a window and hands the editor the change while the file loads. Waits
/// until it has loaded, or failed to; the window, the editor, and the file that kept the
/// change.
fn relaunch(
    cx: &mut TestAppContext,
    folder: &Path,
    change: Kept,
) -> (AnyWindowHandle, Entity<FileEditor>, PathBuf) {
    let from = keep(folder, &change).unwrap();
    cx.executor().allow_parking();
    let (window, editor) = cx.update(|cx| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        let from = from.clone();
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                let mut editor = FileEditor::open(change.path.clone(), None, window, cx);
                editor.restore(change, from, window, cx);
                editor
            })
        })
        .expect("the test window opens")
    });
    wait_until(cx, "the file has loaded", |cx| !editor.read(cx).loading);
    (window, editor, from)
}

/// `editor`'s note, and whether it is a warning.
fn note(cx: &mut TestAppContext, editor: &Entity<FileEditor>) -> (String, bool) {
    cx.update(|cx| {
        editor
            .read(cx)
            .notice
            .as_ref()
            .map_or((String::new(), false), |n| (n.text.clone(), n.error))
    })
}

/// SITE-1: the Dock's Quit, logging out or the install script's quit end the app with no
/// way to ask. Quitting then keeps each file's unsaved changes, and only those, with the
/// file's text they were made to, for the next launch, and writes nothing over the files
/// themselves. They were lost.
#[gpui_kit::test]
fn quitting_keeps_unsaved_changes_for_the_next_launch(cx: &mut TestAppContext) {
    let (path, folder) = setup(cx, "keep");
    let (w, editor) = open_editor(cx, &path);
    type_into(cx, w, &editor, "typed ");
    let clean = path.with_file_name("clean.txt");
    std::fs::write(&clean, "clean\n").unwrap();
    open_editor(cx, &clean);

    // What macOS's terminate: runs once nothing could stop it: every quit hook, then the end.
    cx.quit();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
    assert_eq!(
        kept_in(&folder),
        vec![kept(&path, "typed on disk\n", Some("on disk\n"))]
    );
}

/// Changes the user chose Don't Save for aren't kept; an edit made after that is.
#[gpui_kit::test]
fn changes_the_user_chose_not_to_save_are_not_kept(cx: &mut TestAppContext) {
    let (path, folder) = setup(cx, "discard");
    let other = path.with_file_name("todo.md");
    std::fs::write(&other, "todo\n").unwrap();
    let (w, discarded) = open_editor(cx, &path);
    type_into(cx, w, &discarded, "typed ");
    let (w2, edited) = open_editor(cx, &other);
    type_into(cx, w2, &edited, "a ");
    cx.update(|cx| {
        discarded.update(cx, |e, _| e.discard());
        edited.update(cx, |e, _| e.discard());
    });
    type_into(cx, w2, &edited, "b ");

    cx.quit();
    assert_eq!(
        kept_in(&folder),
        vec![kept(&other, "a b todo\n", Some("todo\n"))]
    );
}

/// The next launch hands each file its kept changes, before or after the file has loaded:
/// they're back in the buffer, unsaved, with a note, the file on disk as it was, and the kept
/// copy goes.
#[gpui_kit::test]
fn kept_changes_come_back_unsaved(cx: &mut TestAppContext) {
    let (path, folder) = setup(cx, "restore");
    let change = kept(&path, "typed on disk\n", Some("on disk\n"));
    let (_, editor, from) = relaunch(cx, &folder, change);
    assert_eq!(buffer(cx, &editor), "typed on disk\n");
    assert!(
        cx.update(|cx| editor.read(cx).dirty),
        "kept changes came back saved"
    );
    let (text, warning) = note(cx, &editor);
    assert!(!warning && text.contains("are back"), "{text:?}");
    assert!(!from.exists(), "the kept copy stayed");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");

    // A file already open and loaded takes them at once.
    let other = path.with_file_name("todo.md");
    std::fs::write(&other, "todo\n").unwrap();
    let change = kept(&other, "more todo\n", Some("todo\n"));
    let from = keep(&folder, &change).unwrap();
    let (w, open) = open_editor(cx, &other);
    cx.update_window(w, |_, window, cx| {
        open.update(cx, |e, cx| e.restore(change, from.clone(), window, cx))
    })
    .expect("the test window is open");
    cx.run_until_parked();
    assert_eq!(buffer(cx, &open), "more todo\n");
    assert!(cx.update(|cx| open.read(cx).dirty));
    assert!(!note(cx, &open).1);
    assert!(!from.exists());

    // A new file, never saved, comes back the same way: nothing on disk is written over.
    let new = path.with_file_name("new.rs");
    let (_, editor, _) = relaunch(cx, &folder, kept(&new, "fn main() {}\n", Some("")));
    assert_eq!(buffer(cx, &editor), "fn main() {}\n");
    let (text, warning) = note(cx, &editor);
    assert!(!warning && text.contains("are back"), "{text:?}");
    assert!(!new.exists());
}

/// I1: the file changed on disk after its changes were kept (a `git pull`, another editor)
/// and before the next launch. The changes come back with a warning that saving writes over
/// the version on disk, as when that happens while Sawit runs, and a quit that can't ask
/// keeps them again as made to the older text, so the launch after warns too. The note said
/// "Save them", and ⌘S wrote over the change on disk without a word.
#[gpui_kit::test]
fn kept_changes_over_a_file_that_changed_since_come_back_with_a_warning(cx: &mut TestAppContext) {
    let (path, folder) = setup(cx, "changed");
    let change = kept(&path, "typed on disk\n", Some("on disk\n"));
    std::fs::write(&path, "pulled upstream\n").unwrap();
    let (w, editor, from) = relaunch(cx, &folder, change);
    assert_eq!(buffer(cx, &editor), "typed on disk\n");
    assert!(cx.update(|cx| editor.read(cx).dirty));
    let (text, warning) = note(cx, &editor);
    assert!(
        warning,
        "no warning that the file changed on disk: {text:?}"
    );
    assert!(text.contains("changed on disk"), "{text:?}");
    assert!(!from.exists(), "the kept copy stayed");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "pulled upstream\n");

    // The editor's watch looks at the file again and finds it as it loaded: that doesn't
    // make the warning out of date, as it would one about a change since the file loaded.
    let now = mtime(&path);
    cx.update_window(w, |_, window, cx| {
        editor.update(cx, |e, cx| e.on_disk_mtime(now, window, cx))
    })
    .expect("the test window is open");
    assert_eq!(note(cx, &editor), (text, true), "the warning went");

    cx.quit();
    assert_eq!(
        kept_in(&folder),
        vec![kept(&path, "typed on disk\n", Some("on disk\n"))]
    );
}

/// The file was deleted after its changes were kept: they come back with a warning that
/// saving makes it again, and nothing makes it yet. Changes kept with no text they were made
/// to warn as made to another version: Sawit can't tell.
#[gpui_kit::test]
fn kept_changes_for_a_deleted_file_or_an_unknown_version_come_back_with_a_warning(
    cx: &mut TestAppContext,
) {
    let (path, folder) = setup(cx, "deleted");
    std::fs::remove_file(&path).unwrap();
    let change = kept(&path, "typed on disk\n", Some("on disk\n"));
    let (_, editor, _) = relaunch(cx, &folder, change);
    assert_eq!(buffer(cx, &editor), "typed on disk\n");
    let (text, warning) = note(cx, &editor);
    assert!(warning && text.contains("deleted"), "{text:?}");
    assert!(!path.exists(), "the deleted file was made again");

    let other = path.with_file_name("todo.md");
    std::fs::write(&other, "todo\n").unwrap();
    let (_, editor, _) = relaunch(cx, &folder, kept(&other, "more todo\n", None));
    let (text, warning) = note(cx, &editor);
    assert!(warning && text.contains("changed on disk"), "{text:?}");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "todo\n");
}

/// Site M3: the file can't be edited at the next launch (it is binary now, or can't be read),
/// so its kept changes can't be put back. They stay where they were kept, for a launch that
/// can, and the tab's note says where, as a warning, also when the changes reach a tab whose
/// file had already failed to load. They stayed there unseen, the tab opening again at every
/// launch.
#[gpui_kit::test]
fn kept_changes_for_a_file_that_cant_be_edited_say_where_they_are(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let (path, folder) = setup(cx, "uneditable");
    std::fs::write(&path, b"binary\0now").unwrap();
    let change = kept(&path, "typed on disk\n", Some("on disk\n"));
    let (_, editor, from) = relaunch(cx, &folder, change);
    let (text, warning) = note(cx, &editor);
    assert!(warning, "not a warning: {text:?}");
    assert!(text.starts_with("Binary or non-UTF-8 file."), "{text:?}");
    assert!(text.contains(&from.display().to_string()), "{text:?}");
    assert!(from.exists(), "the kept changes went");
    assert_eq!(std::fs::read(&path).unwrap(), b"binary\0now");

    // A file that can't be read at all.
    let locked = path.with_file_name("locked.txt");
    std::fs::write(&locked, "secret\n").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let change = kept(&locked, "typed secret\n", Some("secret\n"));
    let (_, editor, from) = relaunch(cx, &folder, change);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (text, warning) = note(cx, &editor);
    assert!(warning, "not a warning: {text:?}");
    assert!(text.starts_with("Unable to open this file."), "{text:?}");
    assert!(text.contains(&from.display().to_string()), "{text:?}");
    assert!(from.exists(), "the kept changes went");

    // The file's tab was open already, and its file had failed to load, when the changes came.
    let other = path.with_file_name("other.bin");
    std::fs::write(&other, b"also\0binary").unwrap();
    let change = kept(&other, "typed\n", Some("text\n"));
    let from = keep(&folder, &change).unwrap();
    let (w, open) = cx.update(|cx| {
        let other = other.clone();
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| FileEditor::open(other, None, window, cx))
        })
        .expect("the test window opens")
    });
    wait_until(cx, "the file has loaded", |cx| !open.read(cx).loading);
    cx.update_window(w, |_, window, cx| {
        open.update(cx, |e, cx| e.restore(change, from.clone(), window, cx))
    })
    .expect("the test window is open");
    let (text, warning) = note(cx, &open);
    assert!(
        warning && text.contains(&from.display().to_string()),
        "{text:?}"
    );
    assert!(from.exists(), "the kept changes went");
}

/// Changes put back over a changed file and then saved, as the warning offers, or edited back
/// to the file as it loaded, are done with: later changes are made to the file as it is, and
/// a quit keeps them as made to that, with nothing for the next launch to warn about.
#[gpui_kit::test]
fn changes_after_a_save_over_the_warning_are_kept_as_made_to_the_saved_text(
    cx: &mut TestAppContext,
) {
    let (path, folder) = setup(cx, "saved");
    std::fs::write(&path, "pulled upstream\n").unwrap();
    let change = kept(&path, "typed on disk\n", Some("on disk\n"));
    let (w, editor, _) = relaunch(cx, &folder, change);
    assert!(note(cx, &editor).1, "no warning");
    let _saving = cx.update(|cx| editor.update(cx, |e, cx| e.save_task(cx)));
    wait_until(cx, "the save lands", |cx| !editor.read(cx).dirty);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "typed on disk\n");
    type_into(cx, w, &editor, "more ");

    let other = path.with_file_name("todo.md");
    std::fs::write(&other, "pulled todo\n").unwrap();
    let change = kept(&other, "my todo\n", Some("todo\n"));
    let (w2, back, _) = relaunch(cx, &folder, change);
    assert!(note(cx, &back).1, "no warning");
    // Edited back to the file as it loaded, then changed again.
    cx.update_window(w2, |_, window, cx| {
        let state = back.read(cx).state.clone().expect("the file has loaded");
        state.update(cx, |s, cx| {
            s.select_all(window, cx);
            s.replace("pulled todo\n", window, cx);
        })
    })
    .expect("the test window is open");
    cx.run_until_parked();
    assert!(
        !cx.update(|cx| back.read(cx).dirty),
        "not back as it loaded"
    );
    type_into(cx, w2, &back, "and ");

    cx.quit();
    let mut bases: Vec<(PathBuf, Option<String>)> = kept_in(&folder)
        .into_iter()
        .map(|kept| (kept.path, kept.base))
        .collect();
    bases.sort();
    assert_eq!(
        bases,
        vec![
            (path, Some("typed on disk\n".into())),
            (other, Some("pulled todo\n".into())),
        ]
    );
}

/// The file changed on disk under unsaved changes while Sawit ran, which said so; then a quit
/// that can't ask came. The changes are kept as made to the text before that change, so the
/// next launch warns again.
#[gpui_kit::test]
fn changes_kept_after_the_file_changed_under_them_keep_the_older_text(cx: &mut TestAppContext) {
    let (path, folder) = setup(cx, "running");
    let (w, editor) = open_editor(cx, &path);
    type_into(cx, w, &editor, "typed ");
    std::fs::write(&path, "pulled upstream\n").unwrap();
    // What the editor's watch does when it sees the file's new modification time.
    let now = mtime(&path);
    cx.update_window(w, |_, window, cx| {
        editor.update(cx, |e, cx| e.on_disk_mtime(now, window, cx))
    })
    .expect("the test window is open");
    let (text, warning) = note(cx, &editor);
    assert!(warning && text.contains("changed on disk"), "{text:?}");

    cx.quit();
    assert_eq!(
        kept_in(&folder),
        vec![kept(&path, "typed on disk\n", Some("on disk\n"))]
    );
}
