//! The backups a save keeps of the board the canvas file holds before it replaces it: which kind,
//! if any, and the files beside the canvas file that hold them.

use std::io;
use std::path::{Path, PathBuf};

use figcraft_doc::{Document, NodeId};
use sawit_core::files;

use super::MAX_FILE;

/// Whether `doc` has no layers on any page: nothing drawn, so nothing to lose.
pub fn is_empty(doc: &Document) -> bool {
    doc.pages
        .iter()
        .all(|page| doc.node(*page).is_none_or(|p| p.children.is_empty()))
}

/// How many nodes an edit may touch for [`removes_layers`] to look at those alone; past that it
/// looks at every node of the board as it was.
const CHANGES_LOOKED_AT: usize = 4096;

/// Whether `after` lacks a layer that `before` has, on any page, a nested one included: a save of
/// `after` takes layers off the canvas that held `before` (a clear, a deletion, a page deleted with
/// what is on it, another canvas loaded in its place), so the board the file holds is kept as a
/// backup first ([`backup_after`]). A page alone is no layer: one deleted with nothing on it takes
/// nothing off. Nor does an edit that only adds or changes layers.
pub fn removes_layers(before: &Document, after: &Document) -> bool {
    let gone = |id: NodeId| !after.contains(id) && !before.pages.contains(&id);
    // An edit shares the rest of the document with the version it came from, so the nodes it
    // touched are found in about the time of the edit, not of the document.
    match after.changed_ids(before, CHANGES_LOOKED_AT) {
        Some(changed) => changed.into_iter().any(gone),
        None => before.ids().any(gone),
    }
}

/// Which backups a save keeps of the board the file holds before it replaces it
/// ([`Writer::save_with`]). Each kind has [`BACKUPS`] files of its own beside the canvas file, so
/// that one kind never pushes the other's out. The stronger of two is the later one here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backup {
    /// None: the save takes nothing off the canvas, or only what the user deleted, which they
    /// can undo.
    #[default]
    None,
    /// The save takes some layers off the canvas and leaves others, after an agent's op: a
    /// deletion by id, or an engine command such as `edit.delete`, `edit.cut` or `page.delete`.
    /// Kept as `<file>.del.bak` ([`deletion_backup_file`]). An agent deletes often and a little at
    /// a time: with the backups of clears and loads, five small deletions took the board an agent
    /// cleared first out of the five kept (CANVAS-CORE-3).
    Deletion,
    /// The save takes the board off the canvas: it leaves the canvas without layers
    /// ([`is_empty`]), puts another canvas in its place, or follows an agent's clear. Kept as
    /// `<file>.bak` ([`backup_file`]).
    Board,
}

impl Backup {
    /// What follows the canvas file's name in the names of this kind's backups, before the number
    /// of an older one ([`ring_file`]). `None` for a save that keeps none.
    pub(super) fn suffix(self) -> Option<&'static str> {
        match self {
            Backup::None => None,
            Backup::Deletion => Some(DELETION_BACKUP),
            Backup::Board => Some(BOARD_BACKUP),
        }
    }
}

/// The backup the save after an agent's op keeps ([`Writer::save_with`]): `op` is the op's name
/// (`clear`, `delete`, `replace`, `command`...), and it changed the canvas `before` into `after`.
/// A `replace` (another canvas loaded in place of this one) and an op that leaves the canvas
/// without layers take the board off it, and so does a `clear` of the page on show while other
/// pages keep theirs: [`Backup::Board`]. Any other op that takes layers off ([`removes_layers`])
/// is a deletion, [`Backup::Deletion`]; one that only adds or changes layers keeps none.
pub fn backup_after(op: &str, before: &Document, after: &Document) -> Backup {
    if op == "replace" || is_empty(after) {
        return Backup::Board;
    }
    match removes_layers(before, after) {
        true if op == "clear" => Backup::Board,
        true => Backup::Deletion,
        false => Backup::None,
    }
}

/// How many backups of each kind ([`Backup`]) a canvas file keeps beside it ([`keep_backup`]):
/// `<name>.bak`, the newest, then `<name>.bak.1` to `<name>.bak.4`, each older than the one before;
/// and `<name>.del.bak` to `<name>.del.bak.4` the same way.
pub const BACKUPS: usize = 5;

/// What follows a canvas file's name in the name of the newest backup of a save that takes the
/// board off the canvas ([`Backup::Board`]).
const BOARD_BACKUP: &str = ".bak";

/// What follows a canvas file's name in the name of the newest backup of an agent's deletion
/// ([`Backup::Deletion`]).
const DELETION_BACKUP: &str = ".del.bak";

/// The backup of the canvas file `path` that `n` newer ones came after, of a save that took the
/// board off the canvas ([`Backup::Board`], [`keep_backup`]): `<path>.bak`, the newest, for 0,
/// else `<path>.bak.<n>`.
pub fn backup_file(path: &Path, n: usize) -> PathBuf {
    ring_file(path, BOARD_BACKUP, n)
}

/// The backup of the canvas file `path` that `n` newer ones came after, of an agent's deletion
/// ([`Backup::Deletion`]): `<path>.del.bak`, the newest, for 0, else `<path>.del.bak.<n>`.
pub fn deletion_backup_file(path: &Path, n: usize) -> PathBuf {
    ring_file(path, DELETION_BACKUP, n)
}

/// The backup of the canvas file `path` named with `suffix` (`.bak`, `.del.bak`) that `n` newer
/// ones came after: `<path><suffix>` for 0, else `<path><suffix>.<n>`.
fn ring_file(path: &Path, suffix: &str, n: usize) -> PathBuf {
    match n {
        0 => sibling(path, suffix),
        n => sibling(path, &format!("{suffix}.{n}")),
    }
}

/// Before a save that takes layers off the canvas replaces the file at `path` (a canvas without
/// layers, another canvas loaded in its place, an agent's clear or deletion): when that file
/// loads as a canvas with layers, keeps it as the newest backup of its kind, `<path><suffix>`
/// (`.bak`, or `.del.bak` for a deletion: [`Backup`]), so "clear canvas" (or an agent's clear,
/// load or deletion) can be undone even after the app restarts. So is a file that doesn't load
/// at all (cut short, or written by a newer Sawit), which may hold layers too, as the old
/// canvas kept a scene it couldn't read; an empty file has nothing to keep. On failure the error
/// says why, and the save must not go on.
///
/// The backups of that kind before it move along one name each, `.bak` to `.bak.1` and so on, and
/// the oldest of [`BACKUPS`] goes (CANVAS-CORE-3): one `.bak`, replaced by the next clear, lost
/// the board an agent cleared first when it started over twice. The other kind's stay as they
/// are. A board the newest backup holds already isn't kept again, so that a clear of a board
/// cleared before moves none along. A folder at one of their names stops it, and the save: it is
/// moved nowhere.
pub(super) fn keep_backup(path: &Path, suffix: &str) -> Result<(), String> {
    let old = match files::read_regular(path, MAX_FILE) {
        Ok(old) => old,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!(
                "the canvas already there couldn't be read to keep a copy of it: {e}"
            ));
        }
    };
    let may_have_layers = !old.is_empty()
        && figcraft_format::load_bytes(&old).map_or(true, |(doc, _)| !is_empty(&doc));
    if !may_have_layers {
        return Ok(());
    }
    let newest = ring_file(path, suffix, 0);
    if holds(&newest, &old) {
        return Ok(());
    }
    for n in (1..BACKUPS).rev() {
        let (from, to) = (ring_file(path, suffix, n - 1), ring_file(path, suffix, n));
        let moved = match std::fs::symlink_metadata(&from) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Ok(meta) if meta.is_dir() => Err(io::Error::other("a folder is in its place")),
            // A file, or a link, which is moved and never followed.
            Ok(_) => std::fs::rename(&from, &to),
            Err(e) => Err(e),
        };
        moved.map_err(|e| {
            format!(
                "the backups beside it couldn't be moved along to keep a new one ({}: {e})",
                from.display()
            )
        })?;
    }
    keep_copy(&newest, &old, path).map_err(|e| {
        format!(
            "the canvas already there couldn't be copied to {}: {e}",
            newest.display()
        )
    })
}

/// Whether `file` is a file (not a link) that holds `bytes`.
fn holds(file: &Path, bytes: &[u8]) -> bool {
    let is_like = std::fs::symlink_metadata(file)
        .is_ok_and(|meta| meta.is_file() && meta.len() == bytes.len() as u64);
    is_like && std::fs::read(file).is_ok_and(|held| held == bytes)
}

/// Keeps `bytes`, what the canvas file `of` holds, as the copy at `bak`, with the file's
/// permissions: in place of whatever is at `bak`, a link included, which is never written through
/// ([`files::replace_file`], CANVAS-CORE-1). A cloned repository can hold a link there to any
/// file of the user's.
pub(super) fn keep_copy(bak: &Path, bytes: &[u8], of: &Path) -> Result<(), sawit_core::CoreError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(of).map_or(0o644, |meta| meta.permissions().mode() & 0o777);
    files::replace_file(bak, bytes, std::fs::Permissions::from_mode(mode))
}

/// Where [`open_with_issues`] keeps the original of a file it opens with notes:
/// `<path>.lossy.bak`. That is apart from the backups an emptying save keeps ([`backup_file`]),
/// so that clearing the canvas after edits can't put the edited canvas over the original.
pub(super) fn lossy_backup(path: &Path) -> PathBuf {
    sibling(path, ".lossy.bak")
}

/// `path` with `suffix` added to its name: `canvas.sawit` becomes `canvas.sawit.bak`.
pub(super) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}
