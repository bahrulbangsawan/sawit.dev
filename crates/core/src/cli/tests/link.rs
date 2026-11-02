//! The `sawit` link in `PATH`, and the quoting of the script that installs it as admin.

use super::*;

#[test]
fn link_install_and_state() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("sawit-app");
    std::fs::write(&exe, "").unwrap();
    let link = dir.path().join("bin/sawit");
    assert_eq!(link_state(&link, &exe), LinkState::Missing);
    install_link(&link, &exe).unwrap();
    assert_eq!(link_state(&link, &exe), LinkState::Installed);
    // Re-pointing an older symlink works.
    let other = dir.path().join("old");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&other, &link).unwrap();
    assert_eq!(link_state(&link, &exe), LinkState::Other(other));
    install_link(&link, &exe).unwrap();
    assert_eq!(link_state(&link, &exe), LinkState::Installed);
    // A real file is left alone.
    std::fs::remove_file(&link).unwrap();
    std::fs::write(&link, "mine").unwrap();
    assert_eq!(link_state(&link, &exe), LinkState::Blocked);
    assert!(install_link(&link, &exe).is_err());
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "mine");
}

#[test]
fn admin_script_quoting() {
    assert_eq!(shell_quote(OsStr::new("/A B/it's")), r"'/A B/it'\''s'");
    assert_eq!(applescript_escape(r#"a "b" \c"#), r#"a \"b\" \\c"#);
}
