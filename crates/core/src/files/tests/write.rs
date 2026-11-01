//! Atomic saves, their staging files, and links.

use std::fs;
use std::path::Path;

use crate::files::write::stage_and_rename;
use crate::files::{replace_file, staging_file, write_atomic, write_text_atomic};

#[test]
fn write_is_atomic_and_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("run.sh");
    fs::write(&f, "old").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    write_text_atomic(&f, "new").unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "new");
    assert_eq!(
        fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let leftovers: Vec<_> = fs::read_dir(d.path()).unwrap().collect();
    assert_eq!(leftovers.len(), 1);
}

#[test]
fn write_atomic_writes_bytes_through_a_staging_file() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("board.sawit");
    let pid = std::process::id();
    assert_eq!(
        staging_file(&f, 7),
        d.path().join(format!(".board.sawit.sawit-tmp-{pid}-7"))
    );
    // Bytes that aren't text, to a new file, which gets the usual permissions.
    write_atomic(&f, &[0, 0xFF, 0xFE]).unwrap();
    assert_eq!(fs::read(&f).unwrap(), [0, 0xFF, 0xFE]);
    assert_eq!(
        fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(names(d.path()), ["board.sawit"], "a temp file was left");
}

/// CANVAS-CORE-1: the temp file is made new, at a name nothing is at. A file, a folder or a
/// link at a name is passed over, untouched, for the next; with every name taken the write
/// fails, and the file is as it was.
#[test]
fn staging_passes_over_a_name_that_is_taken() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("board.sawit");
    fs::write(&f, "old").unwrap();
    let victim = d.path().join("victim");
    fs::write(&victim, "the user's").unwrap();
    let taken = ["link", "dangling", "folder"].map(|name| d.path().join(name));
    symlink(&victim, &taken[0]).unwrap();
    symlink(d.path().join("nothing"), &taken[1]).unwrap();
    fs::create_dir(&taken[2]).unwrap();
    let mode = fs::Permissions::from_mode(0o755);

    let free = d.path().join("free");
    let names = taken.iter().cloned().chain([free.clone()]);
    stage_and_rename(&f, b"new", mode.clone(), names).unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "new");
    assert_eq!(
        fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert!(!free.exists(), "the temp file was renamed");
    assert_eq!(fs::read_to_string(&victim).unwrap(), "the user's");
    let victim_mode = fs::metadata(&victim).unwrap().permissions().mode() & 0o777;
    assert_eq!(victim_mode, 0o644);
    assert!(!d.path().join("nothing").exists());

    let error = stage_and_rename(&f, b"newer", mode, taken.iter().cloned()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists, "{error}");
    assert_eq!(fs::read_to_string(&f).unwrap(), "new");
    assert_eq!(fs::read_to_string(&victim).unwrap(), "the user's");
}

/// CANVAS-CORE-1: a backup is put in place of a link at its name, which is never followed;
/// the canvas's own file, a link the user made, still is ([`write_atomic`]).
#[test]
fn replace_file_replaces_a_link_and_write_atomic_follows_one() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    let target = d.path().join("target");
    fs::write(&target, "target").unwrap();
    let link = d.path().join("link");
    symlink(&target, &link).unwrap();
    replace_file(&link, b"copy", fs::Permissions::from_mode(0o600)).unwrap();
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_file());
    assert_eq!(fs::read_to_string(&link).unwrap(), "copy");
    assert_eq!(
        fs::metadata(&link).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "target");

    let user_link = d.path().join("board");
    symlink(&target, &user_link).unwrap();
    write_atomic(&user_link, b"saved").unwrap();
    assert!(
        fs::symlink_metadata(&user_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "saved");
    assert_eq!(names(d.path()), ["board", "link", "target"]);
}

/// CANVAS-CORE-1: two threads saving one file, each through its own temp file, leave it
/// whole: one save or the other, never parts of both.
#[test]
fn two_writers_of_one_file_never_share_a_temp_file() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("canvas.sawit");
    let [a, b] = (*b"ab").map(|byte| vec![byte; 256 * 1024]);
    std::thread::scope(|scope| {
        for bytes in [&a, &b] {
            let f = &f;
            scope.spawn(move || {
                for _ in 0..20 {
                    write_atomic(f, bytes).unwrap();
                }
            });
        }
        for _ in 0..40 {
            let now = fs::read(&f).unwrap_or_else(|_| a.clone());
            assert!(now == a || now == b, "a mix of both saves");
        }
    });
    assert_eq!(names(d.path()), ["canvas.sawit"], "a temp file was left");
}

/// CANVAS-CORE-1: a cloned repository can hold a link where a save of one of its files was
/// staged (`.<name>.sawit-tmp`). A save never writes through it, nor makes what it names, nor
/// changes its permissions: the link to a hook, beside an executable README, made the hook
/// from the README's text, executable, and git ran it.
#[test]
fn a_save_never_writes_through_a_link_beside_the_file() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("README.md");
    fs::write(&f, "old").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    let staged = d.path().join(".README.md.sawit-tmp");
    // A file of the user's.
    let victim = d.path().join("victim");
    fs::write(&victim, "the user's").unwrap();
    symlink(&victim, &staged).unwrap();
    write_text_atomic(&f, "new").unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "the user's");
    let mode = fs::metadata(&victim).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "the link's target was made {mode:o}");
    let saved = fs::symlink_metadata(&f).unwrap();
    assert!(saved.file_type().is_file(), "the file became a link");
    assert_eq!(fs::read_to_string(&f).unwrap(), "new");
    assert_eq!(saved.permissions().mode() & 0o777, 0o755);
    // A hook that isn't there yet.
    let hooks = d.path().join("hooks");
    fs::create_dir(&hooks).unwrap();
    fs::remove_file(&staged).unwrap();
    symlink(hooks.join("pre-commit"), &staged).unwrap();
    write_text_atomic(&f, "newer").unwrap();
    assert!(names(&hooks).is_empty(), "made {:?}", names(&hooks));
    assert_eq!(fs::read_to_string(&f).unwrap(), "newer");
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn saving_through_a_symlink_keeps_the_link() {
    let d = tempfile::tempdir().unwrap();
    let target = d.path().join("real.txt");
    let link = d.path().join("link.txt");
    fs::write(&target, "old").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    write_text_atomic(&link, "new").unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
}
