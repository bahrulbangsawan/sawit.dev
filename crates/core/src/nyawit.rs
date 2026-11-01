//! `.nyawit/` inside a project folder: what Sawit makes for that project
//! (the canvas board, canvas and diagram exports, database exports) is kept
//! there, next to the code, instead of in Sawit's own data folder.

use std::io::ErrorKind;
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
pub const BOARD_FILE: &str = "canvas.sawit";

/// The name the canvas file had before it took Sawit's own extension. A folder that holds a
/// canvas under it has it moved to [`BOARD_FILE`] when the canvas is next opened
/// ([`migrate_canvas`]).
pub const LEGACY_BOARD_FILE: &str = "canvas.figcraft";

pub fn board_file(project: &Path) -> PathBuf {
    dir(project).join(BOARD_FILE)
}

/// The project's canvas file by the name it had before ([`LEGACY_BOARD_FILE`]).
pub fn legacy_board_file(project: &Path) -> PathBuf {
    dir(project).join(LEGACY_BOARD_FILE)
}

/// What [`migrate_canvas`] did in a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migration {
    /// There was no canvas under the old name (or a folder is at it): nothing moved.
    Nothing,
    /// The canvas moved to its new name, with its backups.
    Moved,
    /// A canvas was under both names: the one under the new name is the canvas, and the one under
    /// the old name, with its backups, was left as it is.
    BothKept,
}

/// Moves the canvas kept in `folder` (a project's `.nyawit/`, or Sawit's own folder) under its old
/// name, [`LEGACY_BOARD_FILE`], to its name now, [`BOARD_FILE`], once: the app's canvas tab and
/// `sawit-mcp` (with the app closed) both call it before they open the canvas, so either one
/// moves it, and the other finds it moved.
///
/// The rule:
/// - Nothing under the old name (or a folder there): nothing to do.
/// - Something under the new name already: it is the canvas, newer than the old file by how it
///   came to be there (only this version of Sawit writes it), and nothing is moved or replaced.
///   The old file and its backups stay where they are, for the user to open or delete.
/// - Else the backups Sawit kept of the old file ([`is_backup_suffix`]: `canvas.figcraft.bak`,
///   `.bak.1`, `.del.bak`, `.lossy.bak`...) move to the same names after the new one
///   (`canvas.sawit.bak`...), so that `canvas_load` and the user find them by the canvas's name
///   now; then the canvas itself moves. A backup whose new name is taken stays where it is.
///
/// Each move is a rename that never replaces what is at the name it goes to, made in one step
/// ([`rename_new`]): a link moves as the link, nothing is copied, and a crash in between leaves
/// every file under one of its two names. The canvas moves last, so a move cut short is finished
/// the next time. Two processes moving at once each find the other's moves done.
pub fn migrate_canvas(folder: &Path) -> std::io::Result<Migration> {
    let old = folder.join(LEGACY_BOARD_FILE);
    let new = folder.join(BOARD_FILE);
    match std::fs::symlink_metadata(&old) {
        Ok(meta) if meta.is_dir() => return Ok(Migration::Nothing),
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Migration::Nothing),
        Err(e) => return Err(e),
    }
    if taken(&new) {
        return Ok(Migration::BothKept);
    }
    for entry in std::fs::read_dir(folder)? {
        let name = entry?.file_name();
        let Some(suffix) = (name.to_str()).and_then(|name| name.strip_prefix(LEGACY_BOARD_FILE))
        else {
            continue;
        };
        if !is_backup_suffix(suffix) {
            continue;
        }
        let to = folder.join(format!("{BOARD_FILE}{suffix}"));
        match rename_new(&folder.join(&name), &to) {
            Ok(()) => {}
            // Taken: that backup stays as it is. Gone: another process moved it.
            Err(e) if matches!(e.kind(), ErrorKind::AlreadyExists | ErrorKind::NotFound) => {}
            Err(e) => return Err(e),
        }
    }
    match rename_new(&old, &new) {
        Ok(()) => Ok(Migration::Moved),
        // Another process made the new file, or moved the old one there, meanwhile.
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(Migration::BothKept),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Migration::Moved),
        Err(e) => Err(e),
    }
}

/// Whether `suffix`, what follows a canvas file's name, names a backup Sawit keeps of it beside
/// it: `.bak` and `.del.bak`, the newest of each kind, the older `.bak.<n>` and `.del.bak.<n>`,
/// and the `.lossy.bak` of a file opened with notes. Only these, in lower case, as the store writes
/// them: an export (`canvas.sawit.png`) or anything else that starts with the canvas file's name
/// is not one.
pub fn is_backup_suffix(suffix: &str) -> bool {
    let numbered = |kind: &str| {
        suffix
            .strip_prefix(kind)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    matches!(suffix, ".bak" | ".del.bak" | ".lossy.bak")
        || numbered(".bak.")
        || numbered(".del.bak.")
}

/// Renames `from` to `to` in one step, refusing to replace anything at `to`, a link to nothing
/// included: `AlreadyExists` then, and both stay as they are. A link at `from` moves as the link.
/// On a volume that can't refuse in the rename itself, `to` is made a second name of `from`'s
/// file, which fails when `to` is taken, and then `from` goes; on one without second names either
/// (exFAT, say), it is a rename once `to` is seen free, which only a writer in between could race.
pub fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    match rename_exclusive(from, to) {
        Err(e) if e.raw_os_error() == Some(libc::ENOTSUP) => match std::fs::hard_link(from, to) {
            Ok(()) => std::fs::remove_file(from),
            Err(e) if matches!(e.kind(), ErrorKind::AlreadyExists | ErrorKind::NotFound) => Err(e),
            Err(_) if taken(to) => Err(ErrorKind::AlreadyExists.into()),
            Err(_) => std::fs::rename(from, to),
        },
        other => other,
    }
}

/// `renamex_np(from, to, RENAME_EXCL)`: a rename that fails with `EEXIST` when `to` is taken.
#[cfg(target_os = "macos")]
fn rename_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;
    let c = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from(ErrorKind::InvalidInput))
    };
    let (from, to) = (c(from)?, c(to)?);
    // SAFETY: both are valid NUL-terminated paths that outlive the call.
    match unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Elsewhere there is no rename that refuses, so [`rename_new`] makes a second name instead.
#[cfg(not(target_os = "macos"))]
fn rename_exclusive(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(libc::ENOTSUP))
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
    fn the_board_file_is_the_sawit_canvas_in_nyawit() {
        assert_eq!(
            board_file(Path::new("/proj")),
            Path::new("/proj/.nyawit/canvas.sawit")
        );
        assert_eq!(
            legacy_board_file(Path::new("/proj")),
            Path::new("/proj/.nyawit/canvas.figcraft")
        );
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A folder holding the canvas under its old name, with every kind of backup the store keeps
    /// of it, and files that only start with its name.
    fn folder_with_the_old_canvas() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        for name in [
            "canvas.figcraft",
            "canvas.figcraft.bak",
            "canvas.figcraft.bak.1",
            "canvas.figcraft.bak.4",
            "canvas.figcraft.del.bak",
            "canvas.figcraft.del.bak.2",
            "canvas.figcraft.lossy.bak",
            // Not backups: an export named after the file, and a save cut short.
            "canvas.figcraft.png",
            ".canvas.figcraft.sawit-tmp-1-0",
            "canvas.excalidraw",
        ] {
            std::fs::write(folder.path().join(name), name).unwrap();
        }
        folder
    }

    /// The canvas under its old name moves to its new one with its backups, each holding what it
    /// held; what only starts with its name, and the older Excalidraw canvas, stay as they are.
    /// It moves once: the next call finds nothing to move.
    #[test]
    fn the_old_canvas_moves_to_its_new_name_with_its_backups() {
        let folder = folder_with_the_old_canvas();
        let dir = folder.path();
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::Moved);
        assert_eq!(
            names(dir),
            [
                ".canvas.figcraft.sawit-tmp-1-0",
                "canvas.excalidraw",
                "canvas.figcraft.png",
                "canvas.sawit",
                "canvas.sawit.bak",
                "canvas.sawit.bak.1",
                "canvas.sawit.bak.4",
                "canvas.sawit.del.bak",
                "canvas.sawit.del.bak.2",
                "canvas.sawit.lossy.bak",
            ]
        );
        let held = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        assert_eq!(held("canvas.sawit"), "canvas.figcraft");
        assert_eq!(held("canvas.sawit.bak.4"), "canvas.figcraft.bak.4");
        assert_eq!(held("canvas.sawit.del.bak"), "canvas.figcraft.del.bak");
        assert_eq!(held("canvas.sawit.lossy.bak"), "canvas.figcraft.lossy.bak");
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::Nothing);
    }

    /// With a canvas under both names, the new one is the canvas: nothing moves, and nothing is
    /// replaced, the backups of either included.
    #[test]
    fn a_canvas_under_both_names_keeps_both_as_they_are() {
        let folder = folder_with_the_old_canvas();
        let dir = folder.path();
        std::fs::write(dir.join("canvas.sawit"), "new").unwrap();
        std::fs::write(dir.join("canvas.sawit.bak"), "new backup").unwrap();
        let before = names(dir);
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::BothKept);
        assert_eq!(names(dir), before);
        assert_eq!(
            std::fs::read_to_string(dir.join("canvas.sawit")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("canvas.sawit.bak")).unwrap(),
            "new backup"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("canvas.figcraft")).unwrap(),
            "canvas.figcraft"
        );
    }

    /// A move cut short (backups moved, the canvas not yet) is finished by the next call, and a
    /// backup whose new name is taken stays under its old one, where nothing replaces it.
    #[test]
    fn a_move_cut_short_is_finished_and_a_taken_backup_name_is_left() {
        let folder = folder_with_the_old_canvas();
        let dir = folder.path();
        std::fs::rename(
            dir.join("canvas.figcraft.bak"),
            dir.join("canvas.sawit.bak"),
        )
        .unwrap();
        std::fs::write(dir.join("canvas.sawit.del.bak"), "taken").unwrap();
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::Moved);
        let held = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        assert_eq!(held("canvas.sawit"), "canvas.figcraft");
        assert_eq!(held("canvas.sawit.bak"), "canvas.figcraft.bak");
        assert_eq!(held("canvas.sawit.del.bak"), "taken");
        assert_eq!(held("canvas.figcraft.del.bak"), "canvas.figcraft.del.bak");
    }

    /// No canvas under the old name, a folder at it, or no folder at all: nothing moves.
    #[test]
    fn nothing_to_move_moves_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let dir = folder.path();
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::Nothing);
        assert_eq!(
            migrate_canvas(&dir.join("no such folder")).unwrap(),
            Migration::Nothing
        );
        std::fs::write(dir.join("canvas.figcraft.bak"), "orphan").unwrap();
        std::fs::create_dir(dir.join("canvas.figcraft")).unwrap();
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::Nothing);
        assert_eq!(names(dir), ["canvas.figcraft", "canvas.figcraft.bak"]);
    }

    /// CANVAS-CORE-1: a link at the old name moves as the link, never through it, and a link to
    /// nothing at the new name is a canvas there: nothing is written where either points.
    #[test]
    fn links_move_as_links_and_a_link_at_the_new_name_is_kept() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let dir = folder.path();
        let elsewhere = dir.join("elsewhere");
        std::fs::write(&elsewhere, "the user's").unwrap();
        symlink(&elsewhere, dir.join("canvas.figcraft")).unwrap();
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::Moved);
        assert_eq!(
            std::fs::read_link(dir.join("canvas.sawit")).unwrap(),
            elsewhere
        );
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "the user's");

        let other = tempfile::tempdir().unwrap();
        let dir = other.path();
        std::fs::write(dir.join("canvas.figcraft"), "old").unwrap();
        symlink(dir.join("nowhere"), dir.join("canvas.sawit")).unwrap();
        assert_eq!(migrate_canvas(dir).unwrap(), Migration::BothKept);
        assert!(!dir.join("nowhere").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("canvas.figcraft")).unwrap(),
            "old"
        );
    }

    #[test]
    fn rename_new_never_replaces() {
        let folder = tempfile::tempdir().unwrap();
        let (a, b) = (folder.path().join("a"), folder.path().join("b"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let taken = rename_new(&a, &b).unwrap_err();
        assert_eq!(taken.kind(), ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b");
        std::fs::remove_file(&b).unwrap();
        rename_new(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "a");
        assert_eq!(rename_new(&a, &b).unwrap_err().kind(), ErrorKind::NotFound);
    }

    #[test]
    fn backup_suffixes_are_the_stores_names() {
        for suffix in [
            ".bak",
            ".bak.1",
            ".bak.12",
            ".del.bak",
            ".del.bak.4",
            ".lossy.bak",
        ] {
            assert!(is_backup_suffix(suffix), "{suffix}");
        }
        for suffix in [
            "",
            ".png",
            ".bak.",
            ".bak.x",
            ".del",
            ".del.bak.",
            ".tmp",
            ".BAK",
        ] {
            assert!(!is_backup_suffix(suffix), "{suffix}");
        }
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
