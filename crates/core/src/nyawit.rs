//! `.nyawit/` inside a project folder: what Sawit makes for that project
//! (the canvas board, canvas and diagram exports, database exports) is kept
//! there, next to the code, instead of in Sawit's own data folder.

use std::path::{Path, PathBuf};

pub const DIR: &str = ".nyawit";

/// Written once into a new `.nyawit/`: exports can hold database rows, so
/// nothing in it is committed unless the user deletes this file.
const GITIGNORE: &str = "# Sawit's files for this project: the canvas and exports.\n\
                         # Delete this file to commit them.\n*\n";

pub fn dir(project: &Path) -> PathBuf {
    project.join(DIR)
}

pub fn canvas_file(project: &Path) -> PathBuf {
    dir(project).join("canvas.excalidraw")
}

pub fn exports_dir(project: &Path) -> PathBuf {
    dir(project).join("exports")
}

pub fn diagrams_dir(project: &Path) -> PathBuf {
    dir(project).join("diagrams")
}

/// Creates `sub` (a folder under `.nyawit/`, e.g. [`exports_dir`]) and, the
/// first time, `.nyawit/.gitignore`.
pub fn ensure(project: &Path, sub: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(sub)?;
    let ignore = dir(project).join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, GITIGNORE)?;
    }
    Ok(())
}

/// `dir/name`, or `dir/stem-2.ext`, `-3`, … when that is taken.
pub fn unique(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}{ext}")))
        .find(|p| !p.exists())
        .expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_makes_the_folder_and_ignores_it() {
        let project = tempfile::tempdir().unwrap();
        let exports = exports_dir(project.path());
        ensure(project.path(), &exports).unwrap();
        assert!(exports.is_dir());
        let ignore = project.path().join(".nyawit/.gitignore");
        assert!(std::fs::read_to_string(&ignore).unwrap().ends_with("*\n"));
        // A .gitignore the user edited is left alone.
        std::fs::write(&ignore, "canvas.excalidraw\n").unwrap();
        ensure(project.path(), &diagrams_dir(project.path())).unwrap();
        assert_eq!(
            std::fs::read_to_string(&ignore).unwrap(),
            "canvas.excalidraw\n"
        );
    }

    #[test]
    fn unique_names_count_up() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unique(dir.path(), "a.png"), dir.path().join("a.png"));
        std::fs::write(dir.path().join("a.png"), "").unwrap();
        assert_eq!(unique(dir.path(), "a.png"), dir.path().join("a-2.png"));
        std::fs::write(dir.path().join("a-2.png"), "").unwrap();
        assert_eq!(unique(dir.path(), "a.png"), dir.path().join("a-3.png"));
        std::fs::write(dir.path().join("Makefile"), "").unwrap();
        assert_eq!(
            unique(dir.path(), "Makefile"),
            dir.path().join("Makefile-2")
        );
    }
}
