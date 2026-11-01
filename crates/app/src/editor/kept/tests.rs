//! Tests of keeping changes in a folder of their own and reading them back.

use std::path::PathBuf;
use std::time::Duration;

use super::{Kept, all, keep};

fn kept(path: &str, text: &str, base: Option<&str>) -> Kept {
    Kept {
        path: PathBuf::from(path),
        text: text.into(),
        base: base.map(Into::into),
    }
}

/// Each file's changes are kept in a file of their own, never over another's (two files of
/// one name), with the text they were made to, and read back oldest first; what isn't a kept
/// change is left alone. Changes kept before Sawit kept that text read back without it.
#[test]
fn kept_changes_are_read_back_and_never_written_over() {
    let folder = std::env::temp_dir().join(format!("sawit-kept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let first = keep(&folder, &kept("/p/notes.txt", "one", Some("on disk"))).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let second = keep(&folder, &kept("/q/notes.txt", "two", None)).unwrap();
    assert_ne!(first, second);
    std::thread::sleep(Duration::from_millis(20));
    let legacy = folder.join("legacy.txt.json");
    std::fs::write(&legacy, r#"{"path":"/r/legacy.txt","text":"three"}"#).unwrap();
    std::fs::write(folder.join("stray.json"), "not a kept change").unwrap();
    std::fs::write(folder.join("readme.txt"), "{}").unwrap();

    assert_eq!(
        all(&folder),
        vec![
            (first, kept("/p/notes.txt", "one", Some("on disk"))),
            (second, kept("/q/notes.txt", "two", None)),
            (legacy, kept("/r/legacy.txt", "three", None)),
        ]
    );
    assert!(folder.join("stray.json").exists() && folder.join("readme.txt").exists());
    assert!(all(&folder.join("none")).is_empty());
    let _ = std::fs::remove_dir_all(&folder);
}
