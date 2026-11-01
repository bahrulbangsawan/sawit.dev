//! Ignore-file lines.

use std::fs;
use std::path::Path;

use crate::files::{add_ignore_line, ignore_pattern};

#[test]
fn ignore_lines_are_anchored_and_added_once() {
    assert_eq!(ignore_pattern(Path::new("a/b.log"), false), "/a/b.log");
    assert_eq!(ignore_pattern(Path::new("dist"), true), "/dist/");
    assert_eq!(ignore_pattern(Path::new("#x*.md"), false), "/\\#x\\*.md");
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join(".gitignore");
    fs::write(&f, "target").unwrap();
    assert!(add_ignore_line(&f, "/dist/").unwrap());
    assert!(!add_ignore_line(&f, "/dist/").unwrap());
    assert_eq!(fs::read_to_string(&f).unwrap(), "target\n/dist/\n");
    let ex = d.path().join(".git/info/exclude");
    assert!(add_ignore_line(&ex, "/a").unwrap());
    assert_eq!(fs::read_to_string(&ex).unwrap(), "/a\n");
}
