//! The stamps that keep the writer's saves from replacing what something else wrote to a file.

use tempfile::tempdir;

use super::*;
use crate::store::backup::sibling;

#[test]
fn a_stamp_tells_a_file_written_since() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    assert_eq!(Stamp::of(&path), None, "no file");
    let folder = dir.path().join("folder.sawit");
    std::fs::create_dir(&folder).unwrap();
    assert_eq!(Stamp::of(&folder), None, "a folder");
    std::fs::write(&path, b"one").unwrap();
    let first = Stamp::of(&path).unwrap();
    assert_eq!(Stamp::of(&path), Some(first), "unchanged");
    std::fs::write(&path, b"three").unwrap();
    assert_ne!(Stamp::of(&path), Some(first), "written since");
    // Through a symlink, it is its target's.
    let link = dir.path().join("link.sawit");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert_eq!(Stamp::of(&link), Stamp::of(&path));
}

#[test]
fn a_guarded_file_changed_on_disk_is_not_saved_over() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("design.sawit");
    std::fs::write(&path, bytes_with(&["read"])).unwrap();
    let writer = Writer::spawn();
    // Stamped before the canvas reads it, as a canvas tab opens its file.
    writer.guard(path.clone(), Stamp::of(&path));
    // Another app (FigCraft, figcraft-cli, git) or another window writes the file.
    let theirs = bytes_with(&["theirs", "and more"]);
    std::fs::write(&path, &theirs).unwrap();

    writer.save(path.clone(), bytes_with(&["mine"]), false);
    writer.flush();
    assert_eq!(
        writer.last_error().as_deref(),
        Some(
            "design wasn't saved: the file changed on disk since it was opened or saved here \
                 (another app or window wrote to it), and saving would replace those changes. \
                 Save it under another name to keep this version, or close its tab and open the \
                 file again to load those changes."
        )
    );
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    // The canvas's next save is refused too: it still hasn't read what is there.
    writer.save(path.clone(), bytes_with(&["mine", "again"]), false);
    writer.flush();
    assert!(writer.last_error().is_some());
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert_eq!(names(dir.path()), ["design.sawit"]);
}

#[test]
fn a_writers_own_saves_of_a_guarded_file_all_go_through() {
    let dir = tempdir().unwrap();
    // A file that was there when the canvas read it, and one that wasn't, which its first
    // save makes.
    let read = dir.path().join("read.sawit");
    std::fs::write(&read, bytes_with(&["read"])).unwrap();
    let new = dir.path().join("new.sawit");
    let writer = Writer::spawn();
    for path in [&read, &new] {
        writer.guard(path.clone(), Stamp::of(path));
    }
    for n in 0..3 {
        let text = format!("save {n}");
        for path in [&read, &new] {
            writer.save(path.clone(), bytes_with(&[&text]), false);
        }
        writer.flush();
        assert_eq!(writer.last_error(), None, "{text}");
        for path in [&read, &new] {
            assert!(has_text(&open(path).unwrap(), &text), "{text}");
        }
    }
    // Saves queued faster than the thread writes go through too, and so does emptying.
    for n in 0..10 {
        writer.save(read.clone(), bytes_with(&[&format!("burst {n}")]), false);
    }
    writer.flush();
    writer.save(read.clone(), blank(), true);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(is_empty(&open(&read).unwrap().doc().unwrap()));
    assert!(has_text(&open(&sibling(&read, ".bak")).unwrap(), "burst 9"));
}

/// CANVAS-CORE-5: a canvas that found no file at its path (a project's canvas not saved yet,
/// an imported `.excalidraw` file's `.sawit`) refuses to save over a file made there since:
/// another window's canvas of the same file, `sawit -n`, a `git pull` of a committed
/// `.nyawit/`, a headless sawit-mcp. Its first save used to replace that file without a word.
/// The writer's own first save is still written, and guards the file from then on.
#[test]
fn a_file_made_since_the_canvas_found_none_is_not_saved_over() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let writer = Writer::spawn();
    writer.guard(path.clone(), Stamp::of(&path));
    let theirs = bytes_with(&["theirs"]);
    std::fs::write(&path, &theirs).unwrap();

    writer.save(path.clone(), bytes_with(&["mine"]), false);
    writer.flush();
    let error = writer.last_error().expect("the save went through");
    assert!(error.starts_with("canvas wasn't saved: "), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    // Nor is an emptying save, whose backup isn't made either.
    writer.save(path.clone(), blank(), true);
    writer.flush();
    assert!(writer.last_error().is_some());
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert_eq!(names(dir.path()), ["canvas.sawit"]);
}

#[test]
fn an_unguarded_file_is_saved_whatever_wrote_to_it_since() {
    // As sawit-mcp saves a canvas file that no canvas tab has open: every save writes.
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    std::fs::write(&path, bytes_with(&["theirs"])).unwrap();
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["one"]), false);
    writer.flush();
    std::fs::write(&path, bytes_with(&["theirs", "again"])).unwrap();
    writer.save(path.clone(), bytes_with(&["two"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&path).unwrap(), "two"));

    // A guarded file that is gone has nothing to replace: the save makes it again.
    let gone = dir.path().join("gone.sawit");
    std::fs::write(&gone, bytes_with(&["read"])).unwrap();
    writer.guard(gone.clone(), Stamp::of(&gone));
    std::fs::remove_file(&gone).unwrap();
    writer.save(gone.clone(), bytes_with(&["saved"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&gone).unwrap(), "saved"));
}

/// CANVAS-UI-3: a guarded file that went from where the canvas read or wrote it (moved,
/// renamed, put in the Trash or deleted elsewhere) is written there again, as FigCraft's rule
/// has it, but not silently: the writer leaves a note for the canvas to show, once.
#[test]
fn a_guarded_file_written_again_where_it_had_gone_leaves_a_note() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    std::fs::write(&path, bytes_with(&["read"])).unwrap();
    let writer = Writer::spawn();
    writer.guard(path.clone(), Stamp::of(&path));
    writer.save(path.clone(), bytes_with(&["one"]), false);
    writer.flush();
    assert_eq!(writer.take_notice(), None);

    std::fs::remove_file(&path).unwrap();
    writer.save(path.clone(), bytes_with(&["two"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&path).unwrap(), "two"));
    let note = writer.take_notice().expect("no note");
    assert!(
        note.starts_with("canvas.sawit was no longer in ") && note.contains("saved it there again"),
        "{note}"
    );
    // Closing the tab alone leaves the file it wrote again: the note says to delete it too.
    assert!(
        note.ends_with("If it was meant to go, close its tab, then delete the file again."),
        "{note}"
    );
    assert_eq!(writer.take_notice(), None);
    // The next save of the file it wrote is a plain one.
    writer.save(path.clone(), bytes_with(&["three"]), false);
    writer.flush();
    assert_eq!(writer.take_notice(), None);

    // A file that wasn't there when the canvas opened, which its first save makes, is no
    // file that went: no note.
    let new = dir.path().join("new.sawit");
    writer.guard(new.clone(), Stamp::of(&new));
    writer.save(new.clone(), bytes_with(&["new"]), false);
    writer.flush();
    assert_eq!(writer.take_notice(), None);
}

/// CANVAS-UI-3: the guard of a renamed or moved file follows it, with the stamp the canvas
/// held: another app's write to it, at its new place, is still not saved over.
#[test]
fn a_guard_follows_its_file_to_its_new_place() {
    let dir = tempdir().unwrap();
    let (from, to) = (dir.path().join("a.sawit"), dir.path().join("b.sawit"));
    std::fs::write(&from, bytes_with(&["read"])).unwrap();
    let writer = Writer::spawn();
    writer.guard(from.clone(), Stamp::of(&from));
    std::fs::rename(&from, &to).unwrap();
    writer.follow(&from, to.clone());
    writer.save(to.clone(), bytes_with(&["mine"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&to).unwrap(), "mine"));
    let theirs = bytes_with(&["theirs", "and more"]);
    std::fs::write(&to, &theirs).unwrap();
    writer.save(to.clone(), bytes_with(&["mine", "again"]), false);
    writer.flush();
    assert!(writer.last_error().is_some());
    assert_eq!(std::fs::read(&to).unwrap(), theirs);
}
