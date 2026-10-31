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

/// The file the project canvas was in before the native canvas: an Excalidraw scene. Only read
/// now: the native canvas imports it once, leaving it as it is, and `sawit-mcp` falls back to
/// reading it when the importer can't.
pub fn legacy_canvas_file(project: &Path) -> PathBuf {
    dir(project).join("canvas.excalidraw")
}

/// The native canvas's file name, here and in Sawit's own folder (no project).
pub const BOARD_FILE: &str = "canvas.figcraft";

pub fn board_file(project: &Path) -> PathBuf {
    dir(project).join(BOARD_FILE)
}

pub fn exports_dir(project: &Path) -> PathBuf {
    dir(project).join("exports")
}

pub fn diagrams_dir(project: &Path) -> PathBuf {
    dir(project).join("diagrams")
}

/// Creates `sub` (a folder under `.nyawit/`, e.g. [`exports_dir`]) and, the
/// first time, `.nyawit/.gitignore`. Anything at that name, a link too, is
/// left as it is: a link is never written through (CANVAS-CORE-1).
pub fn ensure(project: &Path, sub: &Path) -> std::io::Result<()> {
    use std::io::Write as _;
    std::fs::create_dir_all(sub)?;
    let ignore = dir(project).join(".gitignore");
    if !taken(&ignore) {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ignore)
        {
            Ok(mut file) => file.write_all(GITIGNORE.as_bytes())?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// `dir/name`, or `dir/stem-2.ext`, `-3`, … when that is taken: by anything,
/// a link to nothing too, which a write to the name would make the file it
/// points to (a cloned project's `.nyawit/` can hold one). [`write_unique`]
/// writes a file there.
pub fn unique(dir: &Path, name: &str) -> PathBuf {
    names(dir, name)
        .find(|path| !taken(path))
        .expect("a free name")
}

/// Writes `bytes` as a new file in `dir` at the first name [`unique`] gives,
/// made new there: never through a link, nor over a file another writer made
/// meanwhile, which only moves it on to the next free name. The file's path.
pub fn write_unique(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write as _;
    // Each name tried was free a moment before: only a writer racing this one takes it first.
    for path in names(dir, name).filter(|path| !taken(path)).take(64) {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(bytes) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("no free name for {name} in {}", dir.display()),
    ))
}

/// `dir/name`, then `dir/stem-2.ext`, `-3`, … ([`unique`]).
fn names<'a>(dir: &'a Path, name: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    std::iter::once(dir.join(name)).chain((2..).map(move |n| dir.join(format!("{stem}-{n}{ext}"))))
}

/// Whether something is at `path`: a file, a folder, or a link, wherever it points.
fn taken(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_file_is_the_figcraft_canvas_in_nyawit() {
        assert_eq!(
            board_file(Path::new("/proj")),
            Path::new("/proj/.nyawit/canvas.figcraft")
        );
    }

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

    /// CANVAS-CORE-1: a cloned project's `.nyawit/` can hold links to files that aren't there
    /// yet, anywhere. A link is a name that is taken, though nothing is where it points: an export
    /// gets a name of its own beside it, and `.gitignore` is not made where it points.
    #[test]
    fn a_link_to_nothing_is_a_name_taken() {
        use std::os::unix::fs::symlink;
        let project = tempfile::tempdir().unwrap();
        let exports = exports_dir(project.path());
        std::fs::create_dir_all(&exports).unwrap();
        let elsewhere = project.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        symlink(
            elsewhere.join("ignore"),
            dir(project.path()).join(".gitignore"),
        )
        .unwrap();
        ensure(project.path(), &exports).unwrap();
        symlink(elsewhere.join("page"), exports.join("Page 1.png")).unwrap();
        assert_eq!(unique(&exports, "Page 1.png"), exports.join("Page 1-2.png"));
        let written = write_unique(&exports, "Page 1.png", b"png").unwrap();
        assert_eq!(written, exports.join("Page 1-2.png"));
        assert_eq!(std::fs::read(&written).unwrap(), b"png");
        let again = write_unique(&exports, "Page 1.png", b"again").unwrap();
        assert_eq!(again, exports.join("Page 1-3.png"));
        assert_eq!(std::fs::read(&written).unwrap(), b"png");
        assert!(
            std::fs::read_dir(&elsewhere).unwrap().next().is_none(),
            "a link's target was made"
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
