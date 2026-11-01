//! Quitting, restarting and closing a window with files whose changes are unsaved: the question
//! asks to save them, and Save, Don't Save and Cancel do what they say.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext, WeakEntity,
};

use super::super::{Ending, ask_before_closing, end_all};
use super::{start_sleep, try_quit};
use crate::editor::FileEditor;
use crate::editor::testing::{open_editor, scratch_dir, type_into, wait_until};
use crate::terminal::testing::{init, open_panel};

/// A file with unsaved changes ("typed " before its "on disk\n"), `name` in a folder of its
/// own, open in a window of its own.
fn unsaved_file(
    cx: &mut TestAppContext,
    folder: &str,
    name: &str,
) -> (AnyWindowHandle, Entity<FileEditor>, PathBuf) {
    let path = scratch_dir(folder).join(name);
    std::fs::write(&path, "on disk\n").unwrap();
    let (w, editor) = open_editor(cx, &path);
    type_into(cx, w, &editor, "typed ");
    (w, editor, path)
}

fn on_disk(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// Calls `end_all` for `ending` with no terminal docks, as ⌘Q or Restart to Update do in a
/// window whose terminals run nothing, and counts in `ends` how often it went on.
fn try_ending(cx: &mut TestAppContext, ending: Ending, ends: &Rc<Cell<u32>>) {
    let ends = ends.clone();
    cx.update(|cx| end_all(Vec::new(), ending, cx, move |_| ends.set(ends.get() + 1)));
    cx.run_until_parked();
}

/// SITE-1: quitting with a file's changes unsaved asks to save them first, as closing its tab
/// does. Cancel keeps the app; Save writes the file, then quits; with nothing unsaved left,
/// quitting goes ahead without asking. It used to quit at once, and the changes were lost.
#[gpui_kit::test]
fn quitting_asks_to_save_a_file_with_unsaved_changes(cx: &mut TestAppContext) {
    cx.update(crate::editor::testing::init);
    let (_, editor, path) = unsaved_file(cx, "quit-save", "notes.txt");
    let quits = Rc::new(Cell::new(0));

    try_ending(cx, Ending::Quit, &quits);
    assert_eq!(quits.get(), 0, "quitting didn't ask about the unsaved file");
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Save changes to notes.txt before quitting?".to_string(),
            "Your changes will be lost if you don't save them.".to_string()
        ))
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(quits.get(), 0, "Cancel quit");
    assert_eq!(on_disk(&path), "on disk\n");

    try_ending(cx, Ending::Quit, &quits);
    cx.simulate_prompt_answer("Save");
    wait_until(cx, "it saves, then quits", |_| quits.get() == 1);
    assert_eq!(on_disk(&path), "typed on disk\n");
    assert!(cx.update(|cx| !editor.read(cx).dirty));

    try_ending(cx, Ending::Quit, &quits);
    assert_eq!(quits.get(), 2, "a saved file held up quitting");
    assert!(!cx.has_pending_prompt());
}

/// With several unsaved files the question counts and names them and offers Save All, which
/// goes on only once every one is saved: a file that can't be saved (here its folder was
/// deleted) keeps the app, its notice saying why. Don't Save restarts and writes nothing.
#[gpui_kit::test]
fn ending_with_several_unsaved_files_saves_them_all_or_stays(cx: &mut TestAppContext) {
    cx.update(crate::editor::testing::init);
    let (_, notes, notes_path) = unsaved_file(cx, "many-a", "notes.txt");
    let (_, todo, todo_path) = unsaved_file(cx, "many-b", "todo.md");
    let restarts = Rc::new(Cell::new(0));

    try_ending(cx, Ending::Restart, &restarts);
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Save changes to 2 files before restarting?".to_string(),
            "notes.txt, todo.md. Your changes will be lost if you don't save them.".to_string()
        ))
    );
    std::fs::remove_dir_all(todo_path.parent().unwrap()).unwrap();
    cx.simulate_prompt_answer("Save All");
    wait_until(cx, "notes.txt is saved", |cx| !notes.read(cx).dirty);
    wait_until(cx, "todo.md's save failed", |cx| {
        todo.read(cx).note().is_some()
    });
    cx.run_until_parked();
    assert_eq!(
        restarts.get(),
        0,
        "restarted with a file that couldn't be saved"
    );
    assert_eq!(on_disk(&notes_path), "typed on disk\n");
    let note = cx.update(|cx| {
        todo.read(cx)
            .note()
            .map(|(text, error)| (text.to_owned(), error))
    });
    let (text, error) = note.expect("no notice says why");
    assert!(error && text.starts_with("Unable to save."), "{text:?}");

    try_ending(cx, Ending::Restart, &restarts);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "Save changes to todo.md before restarting?");
    cx.simulate_prompt_answer("Don't Save");
    cx.run_until_parked();
    assert_eq!(restarts.get(), 1, "Don't Save didn't restart");
    assert!(!todo_path.exists(), "Don't Save wrote the file");
}

/// Site M1: a tab made read-only after it was typed into still has unsaved changes, and the
/// question names it. ⌘S keeps to the tab's mark and writes nothing, but Save in the question
/// is the user answering for that file: it writes it, then quits. It used to save nothing and
/// say nothing, and the app stayed, so quitting looked stuck.
#[gpui_kit::test]
fn save_in_the_question_saves_a_read_only_tab_then_quits(cx: &mut TestAppContext) {
    cx.update(crate::editor::testing::init);
    let (w, editor, path) = unsaved_file(cx, "quit-read-only", "notes.txt");
    cx.update(|cx| editor.update(cx, |e, cx| e.set_read_only(true, cx)));
    cx.simulate_keystrokes(w, "cmd-s");
    // A save lands from the runtime's threads: long enough for one to land, nothing does.
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < until {
        cx.run_until_parked();
        assert_eq!(on_disk(&path), "on disk\n", "⌘S wrote a read-only tab");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(cx.update(|cx| editor.read(cx).dirty));

    let quits = Rc::new(Cell::new(0));
    try_ending(cx, Ending::Quit, &quits);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "Save changes to notes.txt before quitting?");
    cx.simulate_prompt_answer("Save");
    wait_until(cx, "it saves, then quits", |_| quits.get() == 1);
    assert_eq!(on_disk(&path), "typed on disk\n");
    assert!(cx.update(|cx| !editor.read(cx).dirty));
    assert!(
        cx.update(|cx| editor.read(cx).read_only),
        "the tab is no longer read-only"
    );
}

/// One question covers both: a terminal that runs something and a file with unsaved changes.
#[gpui_kit::test]
fn quitting_asks_once_about_a_running_terminal_and_an_unsaved_file(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::editor::testing::init(cx);
        init(cx);
    });
    let (w, panel) = open_panel(cx);
    start_sleep(cx, &panel);
    unsaved_file(cx, "quit-both", "notes.txt");
    let quits = Rc::new(Cell::new(0));
    try_quit(cx, w, &panel, &quits);
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Save changes to notes.txt before quitting?".to_string(),
            "Your changes will be lost if you don't save them. A terminal is still running: \
                 sleep. Quitting stops it."
                .to_string()
        ))
    );
    cx.simulate_prompt_answer("Don't Save");
    cx.run_until_parked();
    assert_eq!(quits.get(), 1);
    assert!(!cx.has_pending_prompt(), "a second question followed");
}

/// SITE-1: closing a window asks to save its own unsaved files, not another window's. Cancel
/// keeps it open; Save writes the file, then closes the window.
#[gpui_kit::test]
fn closing_a_window_asks_to_save_its_unsaved_files(cx: &mut TestAppContext) {
    cx.update(crate::editor::testing::init);
    let clean = scratch_dir("close-clean").join("clean.txt");
    std::fs::write(&clean, "clean\n").unwrap();
    let (clean_window, _clean) = open_editor(cx, &clean);
    let (w, _editor, path) = unsaved_file(cx, "close-unsaved", "notes.txt");
    for window in [clean_window, w] {
        cx.update_window(window, |_, window, cx| {
            ask_before_closing(window, WeakEntity::new_invalid(), cx)
        })
        .expect("the test window is open");
    }

    let mut vcx = VisualTestContext::from_window(clean_window, cx);
    assert!(
        vcx.simulate_close(),
        "another window's unsaved file held this one"
    );
    assert!(!cx.has_pending_prompt());

    let mut vcx = VisualTestContext::from_window(w, cx);
    assert!(!vcx.simulate_close(), "the window closed without asking");
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Save changes to notes.txt before closing this window?".to_string(),
            "Your changes will be lost if you don't save them.".to_string()
        ))
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(cx.windows().contains(&w), "Cancel closed the window");

    let mut vcx = VisualTestContext::from_window(w, cx);
    assert!(!vcx.simulate_close());
    cx.simulate_prompt_answer("Save");
    wait_until(cx, "the window closes", |cx| !cx.windows().contains(&w));
    assert_eq!(on_disk(&path), "typed on disk\n");
}
