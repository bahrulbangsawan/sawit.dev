//! Annotation numbers per page, and the names and folders of screenshots and sources.

use super::*;

#[test]
fn numbers_count_per_url() {
    let at = |url: &str, n| Annotation {
        n,
        url: url.into(),
        ..annotation()
    };
    let existing = vec![
        at("http://a/x", 1),
        at("http://a/x#top", 2),
        at("http://a/y", 1),
    ];
    assert_eq!(next_number(&existing, "http://a/x"), 3);
    assert_eq!(next_number(&existing, "http://a/x#other"), 3);
    assert_eq!(next_number(&existing, "http://a/z"), 1);
}

#[test]
fn numbers_follow_the_highest_after_a_delete() {
    let at = |n| Annotation {
        n,
        url: "http://a/x".into(),
        ..annotation()
    };
    // 2 was deleted: the next is 4, so 3 keeps its number.
    assert_eq!(next_number(&[at(1), at(3)], "http://a/x"), 4);
}

#[test]
fn screenshot_names_never_collide() {
    let dir = tempfile::tempdir().unwrap();
    let name = screenshot_name("http://localhost:5173/", 1);
    assert_eq!(name, "localhost-5173-1.png");
    let first = crate::nyawit::unique(dir.path(), &name);
    std::fs::write(&first, b"x").unwrap();
    let second = crate::nyawit::unique(dir.path(), &name);
    assert_eq!(first.file_name().unwrap(), "localhost-5173-1.png");
    assert_eq!(second.file_name().unwrap(), "localhost-5173-1-2.png");
}

#[test]
fn screenshot_names() {
    assert_eq!(
        screenshot_name("http://localhost:5173/cart?x=1", 1),
        "localhost-5173-1.png"
    );
    assert_eq!(
        screenshot_name("https://www.apple.com/iphone/", 2),
        "www.apple.com-2.png"
    );
}

#[test]
fn screenshot_dirs() {
    assert_eq!(
        screenshot_dir(Some(Path::new("/p")), Path::new("/c"), "2026-10-29"),
        Path::new("/p/.nyawit/annotations/2026-10-29")
    );
    assert_eq!(
        screenshot_dir(None, Path::new("/c"), "2026-10-29"),
        Path::new("/c/annotations/2026-10-29")
    );
}

#[test]
fn relative_sources() {
    assert_eq!(
        relative_source("webpack:///./src/App.tsx", None),
        "src/App.tsx"
    );
    let app = Some(Path::new("/Users/me/app"));
    assert_eq!(
        relative_source("/Users/me/app/src/App.tsx", app),
        "src/App.tsx"
    );
    assert_eq!(
        relative_source("/@fs/Users/me/app/src/x.vue", app),
        "src/x.vue"
    );
    assert_eq!(relative_source("file:///opt/x.js", None), "/opt/x.js");
}
