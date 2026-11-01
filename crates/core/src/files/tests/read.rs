//! Reading text and regular files, and refusing the rest.

use std::fs;
use std::path::Path;

use crate::files::{MAX_TEXT_BYTES, check_regular, read_regular, read_text};

#[test]
fn reads_utf8_text() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("q.sql");
    fs::write(&f, "select 1;\n").unwrap();
    let t = read_text(&f).unwrap();
    assert_eq!(t.text, "select 1;\n");
    assert_eq!(t.read_only_reason, None);
}

#[test]
fn binary_file_is_not_opened_as_text() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("img.png");
    fs::write(&f, [0x89, b'P', b'N', b'G', 0, 0, 0xff, 0xfe]).unwrap();
    let t = read_text(&f).unwrap();
    assert!(t.text.is_empty());
    assert!(t.read_only_reason.unwrap().contains("Binary"));
    // A zip archive with an extension the editor doesn't know: read-only, nothing shown.
    let zip = d.path().join("archive.unknown");
    fs::write(&zip, b"PK\x03\x04\x14\x00\x00\x00\x08\x00\xff\xfe").unwrap();
    let t = read_text(&zip).unwrap();
    assert!(t.text.is_empty());
    assert!(t.read_only_reason.unwrap().contains("Binary"));
}

#[test]
fn huge_file_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("big.log");
    fs::write(&f, vec![b'a'; MAX_TEXT_BYTES as usize + 1]).unwrap();
    let t = read_text(&f).unwrap();
    assert!(t.text.is_empty());
    assert!(t.read_only_reason.unwrap().contains("MB"));
}

/// CANVAS-CORE-6: only a regular file is read, and only up to `max`; anything else is
/// refused before it is read, and at once.
#[test]
fn read_regular_reads_files_and_refuses_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("a.bin");
    fs::write(&f, [1, 2, 3]).unwrap();
    assert_eq!(read_regular(&f, 3).unwrap(), [1, 2, 3]);
    let link = d.path().join("link");
    std::os::unix::fs::symlink(&f, &link).unwrap();
    assert_eq!(read_regular(&link, 3).unwrap(), [1, 2, 3]);
    let kind = |path: &Path, max| read_regular(path, max).unwrap_err().kind();
    assert_eq!(kind(&f, 2), std::io::ErrorKind::FileTooLarge);
    assert_eq!(
        kind(&d.path().join("none"), 9),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(kind(d.path(), 9), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        kind(Path::new("/dev/zero"), 9),
        std::io::ErrorKind::InvalidInput
    );
    let pipe = d.path().join("pipe");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.unwrap().success());
    assert_eq!(kind(&pipe, 9), std::io::ErrorKind::InvalidInput);
}

/// `check_regular` passes what `read_regular` reads, and refuses the rest as it does, at once
/// and reading nothing: for a file another reader opens next.
#[test]
fn check_regular_passes_what_read_regular_reads() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("a.bin");
    fs::write(&f, [1, 2, 3]).unwrap();
    let link = d.path().join("link");
    std::os::unix::fs::symlink(&f, &link).unwrap();
    assert!(check_regular(&f, 3).is_ok());
    assert!(check_regular(&link, 3).is_ok());
    let pipe = d.path().join("pipe");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.unwrap().success());
    let zero = d.path().join("zero");
    std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
    for (path, max) in [
        (f.as_path(), 2),
        (&d.path().join("none"), 9),
        (d.path(), 9),
        (&zero, 9),
        (&pipe, 9),
    ] {
        let checked = check_regular(path, max).unwrap_err();
        let read = read_regular(path, max).unwrap_err();
        assert_eq!(checked.kind(), read.kind(), "{}", path.display());
        assert_eq!(checked.to_string(), read.to_string(), "{}", path.display());
    }
}

#[test]
fn special_files_are_not_read() {
    let d = tempfile::tempdir().unwrap();
    let fifo = d.path().join("pipe");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    // Opening a FIFO for reading blocks until a writer appears; this must not.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(read_text(&fifo)).unwrap());
    let file = rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("read_text blocked on a FIFO")
        .unwrap();
    assert!(file.read_only_reason.is_some());
}
