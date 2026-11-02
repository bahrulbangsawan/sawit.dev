//! Opening a canvas file: a missing, corrupt or unreadable one, and one that opens with notes.

use std::path::Path;

use tempfile::tempdir;

use super::*;
use crate::doc::{
    BoardError, DocKind, file_with_a_damaged_image, file_with_an_image_past_the_limit,
};
use crate::store::backup::lossy_backup;

/// A `.sawit` file of a board with a sticky "kept", and a version history.
fn file_with_a_history() -> Vec<u8> {
    let doc = board_with(&["kept"]).doc().unwrap();
    let history = [figcraft_format::Version::new(
        "v1",
        "First",
        "",
        1,
        doc.clone(),
    )];
    figcraft_format::save_bytes_with_versions(&doc, None, &history).unwrap()
}

#[test]
fn board_file_paths() {
    let project = Path::new("/work/app");
    let root = Path::new("/support/sawit");
    assert_eq!(
        board_file(Some(project), root),
        project.join(".nyawit/canvas.sawit")
    );
    assert_eq!(board_file(None, root), root.join("canvas.sawit"));
}

/// Regression: a project that only holds a canvas file named with the canvas engine's own
/// extension has a blank canvas. That file isn't read, and nothing moves, renames or deletes
/// it: the first save writes `canvas.sawit` beside it, which stays as it was.
#[test]
fn a_file_by_the_engines_own_name_is_left_alone() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("shop");
    let nyawit = project.join(".nyawit");
    std::fs::create_dir_all(&nyawit).unwrap();
    let old = nyawit.join("canvas.figcraft");
    let held = bytes_with(&["kept"]);
    std::fs::write(&old, &held).unwrap();

    let file = board_file(Some(&project), dir.path());
    assert_eq!(file, nyawit.join("canvas.sawit"));
    let board = open(&file).unwrap();
    assert_eq!(top_level(&board), 0);
    assert_eq!(names(&nyawit), ["canvas.figcraft"]);

    let writer = Writer::spawn();
    writer.save(file.clone(), bytes_with(&["new"]), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert!(has_text(&open(&file).unwrap(), "new"));
    assert_eq!(std::fs::read(&old).unwrap(), held);
}

#[test]
fn open_missing_file_is_a_new_board_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".nyawit/canvas.sawit");
    let board = open(&path).unwrap();
    assert_eq!(board.kind(), DocKind::Board);
    assert!(!path.exists());
    assert!(names(dir.path()).is_empty(), "not even the folder is made");
    let (board, issues) = open_with_issues(&path).unwrap();
    assert_eq!(board.kind(), DocKind::Board);
    assert!(issues.is_empty());
}

#[test]
fn open_corrupt_file_errors_and_never_saves() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    std::fs::write(&path, b"not a canvas file").unwrap();
    assert!(matches!(open(&path), Err(BoardError::Format(_))));
    assert!(matches!(
        open_with_issues(&path),
        Err(BoardError::Format(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"not a canvas file");
    // No `.bak` or temporary file beside it either.
    assert_eq!(names(dir.path()), ["canvas.sawit"]);
}

#[test]
fn a_file_that_cannot_be_read_is_an_io_error() {
    let dir = tempdir().unwrap();
    // A folder where the file should be.
    let path = dir.path().join("canvas.sawit");
    std::fs::create_dir(&path).unwrap();
    let err = open(&path).err().unwrap();
    assert!(matches!(err, BoardError::Io(_)), "{err:?}");
    assert!(err.to_string().contains("canvas.sawit"), "{err}");
}

/// CANVAS-CORE-6: opening a canvas reads its file on the UI thread (and in sawit-mcp, holding
/// every other canvas call back). A file that is a named pipe blocked it for ever, and one a
/// cloned repository links to `/dev/zero` read zeros until the Mac ran out of memory. Both
/// are refused at once now, as an `.excalidraw` file opened from Files is.
#[test]
fn a_canvas_file_that_is_a_pipe_is_refused_at_once() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    pipe_at(&path);
    let opened = within_seconds(move || open_with_issues(&path).map(|_| ()));
    let error = opened.expect("opening a pipe never returned").unwrap_err();
    assert!(matches!(error, BoardError::Io(_)), "{error:?}");
    let error = error.to_string();
    assert!(
        error.contains("canvas.sawit") && error.contains("not a regular file"),
        "{error}"
    );
    let scene = dir.path().join("sketch.excalidraw");
    pipe_at(&scene);
    let imported = within_seconds(move || import_file(&scene).map(|_| ()));
    let error = imported
        .expect("importing a pipe never returned")
        .unwrap_err();
    let error = error.to_string();
    assert!(
        error.contains("sketch.excalidraw") && error.contains("not a regular file"),
        "{error}"
    );
}

/// CANVAS-CORE-6: a canvas file a cloned repository links to `/dev/zero` is refused at once.
/// Read, it never ended (the probe's process grew past 2.5 GB in a quarter of a second), so
/// this has no run against the old read.
#[test]
fn a_canvas_file_linked_to_a_device_is_refused_at_once() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    std::os::unix::fs::symlink("/dev/zero", &path).unwrap();
    let opened = within_seconds(move || open_with_issues(&path).map(|_| ()));
    let error = opened
        .expect("opening /dev/zero never returned")
        .unwrap_err();
    assert!(error.to_string().contains("not a regular file"), "{error}");
}

/// CANVAS-CORE-6: a canvas file past [`MAX_FILE`] is refused before it is read. The file is
/// sparse: it takes no room on the disk.
#[test]
fn a_canvas_file_past_the_limit_is_refused_unread() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_FILE + 1).unwrap();
    let opened = within_seconds(move || open_with_issues(&path).map(|_| ()));
    let error = opened.expect("opening never returned").unwrap_err();
    assert!(matches!(error, BoardError::Io(_)), "{error:?}");
    assert!(error.to_string().contains("more than 4096 MB"), "{error}");
}

#[test]
fn open_with_issues_backs_up_the_original() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let bak = lossy_backup(&path);
    assert_eq!(bak, dir.path().join("canvas.sawit.lossy.bak"));
    let original = file_with_a_damaged_image();
    std::fs::write(&path, &original).unwrap();

    let (board, issues) = open_with_issues(&path).unwrap();
    assert!(has_text(&board, "kept"));
    assert!(!issues.is_empty());
    assert_eq!(std::fs::read(&bak).unwrap(), original);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "opening saves nothing"
    );

    // `open` does the same, and only drops the notes.
    std::fs::remove_file(&bak).unwrap();
    assert!(open(&path).is_ok());
    assert_eq!(std::fs::read(&bak).unwrap(), original);
}

#[test]
fn open_with_issues_leaves_a_healthy_file_alone() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let original = bytes_with(&["hi"]);
    std::fs::write(&path, &original).unwrap();

    let (board, issues) = open_with_issues(&path).unwrap();
    assert!(has_text(&board, "hi"));
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(names(dir.path()), ["canvas.sawit"], "no backup");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn open_with_issues_refuses_a_file_it_cannot_back_up() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let original = file_with_a_damaged_image();
    std::fs::write(&path, &original).unwrap();
    // A folder where the backup should go.
    std::fs::create_dir(lossy_backup(&path)).unwrap();

    let err = open_with_issues(&path).err().unwrap();
    assert!(matches!(err, BoardError::Io(_)), "{err:?}");
    assert!(err.to_string().contains("canvas.sawit.lossy.bak"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn a_file_with_a_history_opens_without_notes_or_a_backup() {
    // A save writes the history back, so there is nothing to warn about or to keep a copy of.
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    std::fs::write(&path, file_with_a_history()).unwrap();

    let (board, issues) = open_with_issues(&path).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert!(has_text(&board, "kept"));
    assert_eq!(board.session.ds().unwrap().versions.len(), 1);
    assert_eq!(names(dir.path()), ["canvas.sawit"], "no backup");
}

#[test]
fn images_left_in_a_file_make_saving_over_it_refused() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("design.sawit");
    let original = file_with_an_image_past_the_limit();
    std::fs::write(&path, &original).unwrap();

    let (board, issues) = open_with_issues(&path).unwrap();
    assert!(!issues.is_empty());
    let refused = board.refuse_dropping_images(&path).unwrap_err().to_string();
    assert!(
        refused.starts_with("design wasn't saved: 1 of its images weren't loaded"),
        "{refused}"
    );
    // A copy anywhere else loses nothing of this file.
    let copy = dir.path().join("copy.sawit");
    assert!(board.refuse_dropping_images(&copy).is_ok());
    // `open` records them too, and the original is kept beside the file, as for any open
    // with notes.
    assert!(open(&path).unwrap().refuse_dropping_images(&path).is_err());
    assert_eq!(std::fs::read(lossy_backup(&path)).unwrap(), original);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
