//! The writer thread: its saves, folders, errors, temporary files, links and permissions.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use tempfile::tempdir;

use super::*;
use crate::doc::file_with_a_damaged_image;
use crate::store::backup::{lossy_backup, sibling};
use crate::store::writer::{Batch, Msg, Save};

#[test]
fn writer_creates_nyawit_with_gitignore() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".nyawit/canvas.sawit");
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["hi"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&path).unwrap(), "hi"));
    let ignore = dir.path().join(".nyawit/.gitignore");
    assert!(std::fs::read_to_string(ignore).unwrap().ends_with("*\n"));
}

#[test]
fn a_gitignore_the_user_deleted_stays_deleted() {
    // "Delete this file to commit them": the next autosave must not put it back.
    let dir = tempdir().unwrap();
    let path = dir.path().join(".nyawit/canvas.sawit");
    let ignore = dir.path().join(".nyawit/.gitignore");
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["one"]), false);
    writer.flush();
    assert!(ignore.exists());
    std::fs::remove_file(&ignore).unwrap();
    writer.save(path.clone(), bytes_with(&["two"]), false);
    writer.flush();
    assert!(has_text(&open(&path).unwrap(), "two"));
    assert!(!ignore.exists());
}

#[test]
fn other_folders_are_made_without_a_gitignore() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a/b/canvas.sawit");
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["hi"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&path).unwrap(), "hi"));
    assert_eq!(names(&dir.path().join("a/b")), ["canvas.sawit"]);
    assert_eq!(names(&dir.path().join("a")), ["b"]);
}

#[test]
fn newest_save_wins() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["first"]), false);
    writer.save(path.clone(), bytes_with(&["second"]), false);
    writer.flush();
    let board = open(&path).unwrap();
    assert!(has_text(&board, "second"));
    assert!(!has_text(&board, "first"));
}

#[test]
fn a_burst_of_saves_leaves_the_newest_of_each_path() {
    let dir = tempdir().unwrap();
    let (one, two) = (dir.path().join("one.sawit"), dir.path().join("two.sawit"));
    let saves: Vec<(Vec<u8>, Vec<u8>)> = (0..40)
        .map(|n| {
            (
                bytes_with(&[&format!("one {n}")]),
                bytes_with(&[&format!("two {n}")]),
            )
        })
        .collect();
    let writer = Writer::spawn();
    // Queued faster than the thread writes, so most are replaced before they reach the disk.
    for (a, b) in saves {
        writer.save(one.clone(), a, false);
        writer.save(two.clone(), b, false);
    }
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&one).unwrap(), "one 39"));
    assert!(has_text(&open(&two).unwrap(), "two 39"));
}

#[test]
fn a_batch_keeps_the_newest_save_of_each_path() {
    let save = |path: &str, byte: u8| {
        Msg::Save(Save {
            path: path.into(),
            bytes: vec![byte],
            backup: Backup::None,
        })
    };
    let mut batch = Batch::default();
    for msg in [save("a", 1), save("b", 2), save("a", 3)] {
        batch.add(msg);
    }
    let (ack, _) = std::sync::mpsc::channel();
    batch.add(Msg::Flush(ack));
    let mut kept: Vec<(PathBuf, Vec<u8>)> = batch
        .saves
        .iter()
        .map(|s| (s.path.clone(), s.bytes.clone()))
        .collect();
    kept.sort();
    assert_eq!(kept, [("a".into(), vec![3]), ("b".into(), vec![2])]);
    assert_eq!(batch.flushes.len(), 1);
    assert!(!batch.stop);
}

#[test]
fn writer_reports_errors() {
    let dir = tempdir().unwrap();
    // A regular file where the folder should be.
    let blocker = dir.path().join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let path = blocker.join("canvas.sawit");
    let writer = Writer::spawn();
    assert_eq!(writer.last_error(), None);
    writer.save(path, bytes_with(&["hi"]), false);
    writer.flush();
    let error = writer.last_error().unwrap();
    assert!(error.contains("canvas.sawit"), "{error}");
    assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
}

#[test]
fn a_later_good_save_clears_the_error() {
    let dir = tempdir().unwrap();
    let blocker = dir.path().join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let good = dir.path().join("canvas.sawit");
    let writer = Writer::spawn();
    writer.save(blocker.join("canvas.sawit"), bytes_with(&["a"]), false);
    writer.flush();
    assert!(writer.last_error().is_some());
    // A flush with nothing queued doesn't count as a save that went through.
    writer.flush();
    assert!(writer.last_error().is_some());
    writer.save(good.clone(), bytes_with(&["b"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&good).unwrap(), "b"));
}

#[test]
fn saves_leave_no_temporary_file() {
    let dir = tempdir().unwrap();
    let good = dir.path().join("good.sawit");
    // A folder where the file should be: the rename fails, after the temporary file is made.
    let bad = dir.path().join("bad.sawit");
    std::fs::create_dir(&bad).unwrap();
    let writer = Writer::spawn();
    writer.save(good, bytes_with(&["a"]), false);
    writer.save(bad.clone(), bytes_with(&["b"]), false);
    writer.flush();
    assert!(writer.last_error().is_some());
    assert!(bad.is_dir());
    assert_eq!(names(dir.path()), ["bad.sawit", "good.sawit"]);
}

#[test]
fn a_save_goes_through_a_temporary_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempdir().unwrap();
    let folder = dir.path().join("boards");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("canvas.sawit");
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["old"]), false);
    writer.flush();
    // A folder nothing can be made in, around a canvas file that can be written: the
    // temporary file can't be made, so the save fails, and the canvas is untouched. Written
    // straight to the file, the save would have gone through.
    let mode = |mode| std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(&folder, mode(0o555)).unwrap();
    writer.save(path.clone(), bytes_with(&["new"]), false);
    writer.flush();
    std::fs::set_permissions(&folder, mode(0o755)).unwrap();
    assert!(writer.last_error().is_some());
    let board = open(&path).unwrap();
    assert!(has_text(&board, "old"));
    assert!(!has_text(&board, "new"));
    assert_eq!(names(&folder), ["canvas.sawit"]);
}

#[test]
fn a_canvas_file_that_is_a_symlink_stays_one_and_its_target_gets_the_save() {
    // A link the user made to a board kept elsewhere: the save goes to that board, and the
    // link still points at it.
    let dir = tempdir().unwrap();
    let elsewhere = dir.path().join("boards");
    std::fs::create_dir(&elsewhere).unwrap();
    let target = elsewhere.join("plan.sawit");
    std::fs::write(&target, bytes_with(&["old"])).unwrap();
    let link = dir.path().join("plan.sawit");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let writer = Writer::spawn();
    writer.save(link.clone(), bytes_with(&["new"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    let link_kind = std::fs::symlink_metadata(&link).unwrap().file_type();
    assert!(link_kind.is_symlink(), "the link was replaced by a copy");
    assert_eq!(std::fs::read_link(&link).unwrap(), target);
    let saved = open(&target).unwrap();
    assert!(has_text(&saved, "new"));
    assert!(!has_text(&saved, "old"));
    // Nothing is left beside either of them.
    assert_eq!(names(dir.path()), ["boards", "plan.sawit"]);
    assert_eq!(names(&elsewhere), ["plan.sawit"]);
}

/// CANVAS-CORE-1: a cloned repository can hold links at the names the canvas writes beside its
/// file: its backups (`.lossy.bak`, `.bak`) and where a save was staged
/// (`.canvas.sawit.sawit-tmp`), to any file of the user's (`~/.zshrc`). Opening the file
/// with notes, emptying it and saving it never write through them: each link is replaced by
/// the file, or left alone, and what it names stays as it was.
#[test]
fn the_canvas_never_writes_through_a_link_beside_its_file() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let folder = dir.path().join(".nyawit");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("canvas.sawit");
    let original = file_with_a_damaged_image();
    std::fs::write(&path, &original).unwrap();
    let victim = |name: &str| {
        let victim = dir.path().join(name);
        std::fs::write(&victim, format!("{name} of the user's")).unwrap();
        victim
    };
    let (lossy, bak) = (victim("lossy"), victim("bak"));
    symlink(&lossy, lossy_backup(&path)).unwrap();
    symlink(&bak, sibling(&path, ".bak")).unwrap();
    // A link to a file that isn't there yet, by a relative path, as git checks one out.
    symlink("../hook", folder.join(".canvas.sawit.sawit-tmp")).unwrap();

    // Opened with notes, the original is kept in place of the link.
    let (_, issues) = open_with_issues(&path).unwrap();
    assert!(!issues.is_empty());
    assert_eq!(std::fs::read(lossy_backup(&path)).unwrap(), original);
    let kept = std::fs::symlink_metadata(lossy_backup(&path)).unwrap();
    assert!(kept.file_type().is_file(), "the backup is still a link");
    // Saved, then emptied.
    let writer = Writer::spawn();
    let edited = bytes_with(&["edited"]);
    writer.save(path.clone(), edited.clone(), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    writer.save(path.clone(), blank(), true);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert_eq!(std::fs::read(sibling(&path, ".bak")).unwrap(), edited);
    let kept = std::fs::symlink_metadata(sibling(&path, ".bak")).unwrap();
    assert!(kept.file_type().is_file(), "the backup is still a link");
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_file()
    );
    assert!(is_empty(&open(&path).unwrap().doc().unwrap()));
    // What the links named is as it was, and nothing was made where one pointed.
    assert_eq!(
        std::fs::read_to_string(&lossy).unwrap(),
        "lossy of the user's"
    );
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "bak of the user's");
    assert!(!dir.path().join("hook").exists());
}

#[test]
fn a_saved_file_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt as _;
    // A file only its owner may read stays so: a save doesn't make it readable by everyone.
    let dir = tempdir().unwrap();
    let path = dir.path().join("private.sawit");
    std::fs::write(&path, bytes_with(&["old"])).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["new"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&path).unwrap(), "new"));
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "saved as {mode:o}");
}

#[test]
fn a_writer_whose_thread_is_gone_reports_saves_and_does_not_hang_on_flush() {
    let (tx, rx) = mpsc::channel();
    drop(rx);
    let writer = Writer {
        tx,
        error: Arc::default(),
        notice: Arc::default(),
        stamps: Arc::default(),
        thread: None,
    };
    writer.save("/nowhere/canvas.sawit".into(), blank(), false);
    let error = writer.last_error().unwrap();
    assert!(error.contains("/nowhere/canvas.sawit"), "{error}");
    writer.flush();
}

#[test]
fn dropping_the_writer_finishes_what_is_queued() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    {
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["hi"]), false);
    }
    assert!(has_text(&open(&path).unwrap(), "hi"));
}
