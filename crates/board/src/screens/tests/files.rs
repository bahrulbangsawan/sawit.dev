//! Writes to a screen's files.

use serde_json::json;

use crate::screens::files::{Write, apply, check_path, list, make_folder, page, replace_once};

fn write(op: serde_json::Value) -> Result<Write, String> {
    Write::from_op(&op)
}

/// The page, its edits and other files are written; an edit whose text isn't there once
/// changes nothing at all.
#[test]
fn writes_edits_and_files() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("login");
    let first = write(json!({"html": "<h1>Hi</h1><p>One</p>", "files": [
        {"path": "css/site.css", "content": "h1{color:red}"},
        {"path": "img/dot.png", "base64": "iVBORw0KGgo="},
    ]}))
    .unwrap();
    let written = apply(&folder, &first).unwrap();
    assert_eq!(written, ["index.html", "css/site.css", "img/dot.png"]);
    assert_eq!(page(&folder).unwrap(), "<h1>Hi</h1><p>One</p>");
    assert_eq!(
        std::fs::read(folder.join("img/dot.png")).unwrap()[..4],
        [0x89, b'P', b'N', b'G']
    );
    let edit = write(json!({"edits": [
        {"find": "One", "replace": "Two"},
        {"path": "css/site.css", "find": "red", "replace": "blue"},
    ]}))
    .unwrap();
    apply(&folder, &edit).unwrap();
    assert_eq!(page(&folder).unwrap(), "<h1>Hi</h1><p>Two</p>");
    let bad = write(json!({"edits": [
        {"find": "Hi", "replace": "Hello"},
        {"find": "missing", "replace": "x"},
    ]}))
    .unwrap();
    assert!(apply(&folder, &bad).unwrap_err().contains("isn't in it"));
    assert_eq!(
        page(&folder).unwrap(),
        "<h1>Hi</h1><p>Two</p>",
        "nothing changed"
    );
    assert_eq!(list(&folder), ["css/site.css", "img/dot.png", "index.html"]);
}

#[test]
fn a_write_must_say_what_it_writes() {
    assert!(write(json!({})).is_err());
    assert!(write(json!({"html": "x", "edits": [{"find": "a", "replace": "b"}]})).is_err());
    assert!(write(json!({"edits": [{"find": "", "replace": "b"}]})).is_err());
    assert!(write(json!({"files": [{"path": "a.css"}]})).is_err());
    assert!(write(json!({"files": [{"path": "a.png", "base64": "!!"}]})).is_err());
    assert!(
        replace_once("a a", "a", "b")
            .unwrap_err()
            .contains("more than once")
    );
    assert_eq!(replace_once("abc", "b", "x").unwrap(), "axc");
}

/// Paths stay in the folder: no `..`, no absolute path, no hidden or empty part.
#[test]
fn paths_stay_in_the_folder() {
    for bad in [
        "",
        "/etc/passwd",
        "../x.css",
        "a/../../x",
        "a//b",
        ".git/config",
        "a\\b",
        "./a",
    ] {
        assert!(check_path(bad).is_err(), "{bad}");
        assert!(
            write(json!({"files": [{"path": bad, "content": "x"}]})).is_err(),
            "{bad}"
        );
    }
    assert_eq!(check_path("css/site.css").unwrap(), "css/site.css");
}

/// A link in the screen's folder is replaced, never written through, and a linked folder is
/// refused.
#[test]
fn links_are_never_written_through() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("login");
    std::fs::create_dir_all(&folder).unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    symlink(&outside, folder.join("index.html")).unwrap();
    apply(&folder, &write(json!({"html": "<p>new</p>"})).unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
    assert_eq!(page(&folder).unwrap(), "<p>new</p>");
    symlink(dir.path(), folder.join("css")).unwrap();
    let linked = write(json!({"files": [{"path": "css/a.css", "content": "x"}]})).unwrap();
    assert!(apply(&folder, &linked).unwrap_err().contains("is a link"));
}

/// A project's screens folder is made in `.nyawit`, with its `.gitignore`.
#[test]
fn a_projects_screen_folder_is_made_in_nyawit() {
    let dir = tempfile::tempdir().unwrap();
    let screens = dir.path().join(".nyawit/screens");
    let made = make_folder(&screens, "login").unwrap();
    assert!(made.is_dir());
    assert!(dir.path().join(".nyawit/.gitignore").is_file());
}
