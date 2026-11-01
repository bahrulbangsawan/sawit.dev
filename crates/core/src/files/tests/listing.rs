//! Listing a folder, and resolving the links on a path.

use std::fs;
use std::path::{Path, PathBuf};

use crate::files::{list_dir, resolve};

#[test]
fn resolve_gives_one_path_for_two_names_of_a_place_there_or_not() {
    let d = tempfile::tempdir().unwrap();
    let real = d.path().join("project");
    fs::create_dir(&real).unwrap();
    let link = d.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let canonical = real.canonicalize().unwrap();
    // A file that is there, one that isn't yet, and one in a folder that isn't yet.
    fs::write(real.join("a.sawit"), "").unwrap();
    for name in ["a.sawit", "new.sawit", ".nyawit/canvas.sawit"] {
        assert_eq!(resolve(&link.join(name)), canonical.join(name), "{name}");
        assert_eq!(resolve(&real.join(name)), canonical.join(name), "{name}");
    }
    // A path with nothing of it there stays as it is.
    assert_eq!(
        resolve(Path::new("relative/x")),
        PathBuf::from("relative/x")
    );
}

#[test]
fn list_dir_sorts_dirs_first_and_hides_noise() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    for dir in ["src", ".git", "node_modules", "target", "Assets"] {
        fs::create_dir(p.join(dir)).unwrap();
    }
    for file in ["b.ts", "a.sql", ".env", ".DS_Store"] {
        fs::write(p.join(file), "").unwrap();
    }
    let names: Vec<_> = list_dir(p)
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.is_dir))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Assets".into(), true),
            ("src".into(), true),
            (".env".into(), false),
            ("a.sql".into(), false),
            ("b.ts".into(), false),
        ]
    );
}
