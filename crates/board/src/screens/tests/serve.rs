//! The scheme's answers, and what it refuses.

use std::os::unix::fs::symlink;

use crate::screens::serve::{CSP, may_navigate, serve};

/// A screen folder `login` with a page, a style sheet in a folder, and a secret beside it.
fn screen() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("screens/login");
    std::fs::create_dir_all(folder.join("css")).unwrap();
    std::fs::write(folder.join("index.html"), "<h1>Hi</h1>").unwrap();
    std::fs::write(folder.join("css/site.css"), "h1{}").unwrap();
    std::fs::write(dir.path().join("secret.txt"), "no").unwrap();
    (dir, folder)
}

#[test]
fn the_page_and_its_files_are_served() {
    let (_dir, folder) = screen();
    let get = |url: &str| serve(&folder, "login", "GET", url);
    let page = get("sawit-screen://login/index.html");
    assert_eq!((page.status, page.mime), (200, "text/html; charset=utf-8"));
    assert_eq!(page.body, b"<h1>Hi</h1>");
    assert_eq!(get("sawit-screen://login/").body, b"<h1>Hi</h1>");
    assert_eq!(get("sawit-screen://login").status, 200);
    assert_eq!(get("sawit-screen://login/css/site.css?v=1#x").body, b"h1{}");
    assert_eq!(get("sawit-screen://login/./css/../index.html").status, 200);
    assert_eq!(get("sawit-screen://login/nope.png").status, 404);
    assert_eq!(
        serve(&folder, "login", "HEAD", "sawit-screen://login/").body,
        b""
    );
    assert!(CSP.contains("default-src 'self'") && CSP.contains("form-action 'none'"));
}

/// Another screen's files, a path out of the folder (written plainly, escaped, or through a
/// link), another scheme and a write are all refused.
#[test]
fn what_is_outside_the_screen_is_refused() {
    let (dir, folder) = screen();
    symlink(dir.path().join("secret.txt"), folder.join("link.txt")).unwrap();
    let get = |url: &str| serve(&folder, "login", "GET", url).status;
    assert_eq!(get("sawit-screen://other/index.html"), 403);
    assert_eq!(get("sawit-screen://login/../../secret.txt"), 403);
    assert_eq!(get("sawit-screen://login/%2e%2e/%2e%2e/secret.txt"), 403);
    assert_eq!(
        get("sawit-screen://login/css/%2E%2E%2F..%2Fsecret.txt"),
        403
    );
    assert_eq!(get("sawit-screen://login/link.txt"), 403);
    assert_eq!(get("https://login/index.html"), 400);
    assert_eq!(get("file:///etc/passwd"), 400);
    assert_eq!(
        serve(&folder, "login", "POST", "sawit-screen://login/").status,
        405
    );
}

/// A screen's page goes only to pages of its own folder.
#[test]
fn navigation_stays_in_the_screen() {
    assert!(may_navigate("login", "sawit-screen://login/about.html"));
    assert!(may_navigate("login", "about:blank"));
    for out in [
        "sawit-screen://other/",
        "https://example.com/",
        "http://localhost:3000/",
        "file:///etc/passwd",
        "mailto:a@b.c",
        "javascript:alert(1)",
    ] {
        assert!(!may_navigate("login", out), "{out}");
    }
}
