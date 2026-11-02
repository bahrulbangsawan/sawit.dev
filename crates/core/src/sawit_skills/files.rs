//! Putting the block into one instruction file and taking it out again: the file read whole and
//! written whole (to a file beside it, then moved over it, so nothing ever reads half of one), a
//! file Sawit hasn't changed before copied into Sawit's data folder first, what was written read
//! back and checked, and nothing written when nothing would change. A link is followed: the file
//! it points at changes, and the link stays.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::block::{self, Damage};
use super::ledger::Ledger;
use super::{VERSION, block_text, version_of};

/// What happened to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// Sawit made the file, with the block in it.
    Created,
    /// The block went in after what the file had.
    Added,
    /// The block there is now this Sawit's.
    Updated,
    /// It already had this Sawit's block: nothing written.
    Unchanged,
    /// It has the block of a newer Sawit (its version), which this one leaves alone.
    Newer(u32),
    /// The block came out; the rest of the file stays.
    Removed,
    /// The block came out of a file Sawit made, which held nothing else: the file is gone.
    Deleted,
    /// It has no block (or isn't there): nothing to take out.
    Absent,
}

impl Done {
    /// Whether the file was written (or deleted).
    pub fn changed(&self) -> bool {
        matches!(
            self,
            Done::Created | Done::Added | Done::Updated | Done::Removed | Done::Deleted
        )
    }

    pub fn verb(&self) -> String {
        match self {
            Done::Created => "created".into(),
            Done::Added => "added".into(),
            Done::Updated => "updated".into(),
            Done::Unchanged => "already current".into(),
            Done::Newer(v) => format!("kept: a newer Sawit's block (v{v})"),
            Done::Removed => "removed".into(),
            Done::Deleted => "removed, and the file Sawit made deleted".into(),
            Done::Absent => "nothing to remove".into(),
        }
    }
}

/// One file's change: what happened, and where its first copy went when Sawit made one now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub done: Done,
    pub backup: Option<PathBuf>,
}

/// What a file holds where the block goes, beside this Sawit's block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holds {
    /// No block (or no file).
    Nothing,
    /// This Sawit's block.
    Current,
    /// An older Sawit's block (its version), or this one's edited.
    Older(Option<u32>),
    /// A newer Sawit's.
    Newer(u32),
    /// Its markers are damaged: what is wrong.
    Damaged(String),
    /// It can't be read.
    Unreadable(String),
}

impl Holds {
    /// It has a Sawit block, of any version.
    pub fn any(&self) -> bool {
        matches!(self, Holds::Current | Holds::Older(_) | Holds::Newer(_))
    }

    /// Nothing to bring up to date: this Sawit's block or a newer one.
    pub fn current(&self) -> bool {
        matches!(self, Holds::Current | Holds::Newer(_))
    }
}

/// The file's text, `None` when it isn't there.
fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
            format!(
                "{} isn't UTF-8 text: Sawit leaves it alone.",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Couldn't read {}: {e}", path.display())),
    }
}

/// What `path` holds, beside this Sawit's block.
pub fn holds(path: &Path) -> Holds {
    let text = match read(path) {
        Ok(Some(text)) => text,
        Ok(None) => return Holds::Nothing,
        Err(e) => return Holds::Unreadable(e),
    };
    match block::first(&text) {
        Err(damage) => Holds::Damaged(damage.describe(&path.display().to_string())),
        Ok(None) => Holds::Nothing,
        Ok(Some(there)) if there.trim_end() == block_text().trim_end() => Holds::Current,
        Ok(Some(there)) => match version_of(&there) {
            Some(v) if v > VERSION => Holds::Newer(v),
            v => Holds::Older(v),
        },
    }
}

/// The file a write changes: what `path` points at when it is a link, else `path`.
fn real(path: &Path) -> PathBuf {
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

fn damaged(path: &Path) -> impl Fn(Damage) -> String + '_ {
    move |d| d.describe(&path.display().to_string())
}

/// Puts this Sawit's block into `path`: in place of the one there, else after its last line, else
/// in a new file (its folder made when it isn't there). A newer Sawit's block stays.
pub fn put(path: &Path, ledger: &mut Ledger) -> Result<Change, String> {
    let file = real(path);
    let before = read(&file)?;
    let text = before.as_deref().unwrap_or("");
    if let Some(there) = block::first(text).map_err(damaged(path))?
        && let Some(v) = version_of(&there).filter(|v| *v > VERSION)
    {
        return Ok(unchanged(Done::Newer(v)));
    }
    let after = block::put(text, &block_text()).map_err(damaged(path))?;
    if before.as_deref() == Some(after.as_str()) {
        return Ok(unchanged(Done::Unchanged));
    }
    let done = match &before {
        None => Done::Created,
        Some(text) if block::find(text).is_ok_and(|b| !b.is_empty()) => Done::Updated,
        Some(_) => Done::Added,
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Couldn't make {}: {e}", parent.display()))?;
    }
    let backup = ledger.first_change(&file, before.is_some())?;
    write_whole(&file, &after)?;
    check(&file, &after, true)?;
    Ok(Change { done, backup })
}

/// Takes the block out of `path`, and deletes the file when Sawit made it and nothing else is left.
pub fn take(path: &Path, ledger: &mut Ledger) -> Result<Change, String> {
    let file = real(path);
    let Some(before) = read(&file)? else {
        return Ok(unchanged(Done::Absent));
    };
    let Some(after) = block::take(&before).map_err(damaged(path))? else {
        return Ok(unchanged(Done::Absent));
    };
    if after.is_empty() && ledger.created(&file) {
        std::fs::remove_file(&file)
            .map_err(|e| format!("Couldn't delete {}: {e}", file.display()))?;
        ledger.forget(&file)?;
        return Ok(unchanged(Done::Deleted));
    }
    let backup = ledger.first_change(&file, true)?;
    write_whole(&file, &after)?;
    check(&file, &after, false)?;
    Ok(Change {
        done: Done::Removed,
        backup,
    })
}

fn unchanged(done: Done) -> Change {
    Change { done, backup: None }
}

/// Writes `text` to `file` whole: into a file of its own beside it, with `file`'s permissions,
/// then moved over it.
pub(super) fn write_whole(file: &Path, text: &str) -> Result<(), String> {
    static WRITES: AtomicUsize = AtomicUsize::new(0);
    let n = WRITES.fetch_add(1, Ordering::Relaxed);
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let part = file.with_file_name(format!(".{name}.sawit-{}-{n}", std::process::id()));
    let written = std::fs::write(&part, text)
        .and_then(|()| match std::fs::metadata(file) {
            Ok(meta) => std::fs::set_permissions(&part, meta.permissions()),
            Err(_) => Ok(()),
        })
        .and_then(|()| std::fs::rename(&part, file));
    written.map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("Couldn't write {}: {e}", file.display())
    })
}

/// Reads `file` back: it must hold `text` exactly, with this Sawit's block once when `with_block`,
/// else none.
fn check(file: &Path, text: &str, with_block: bool) -> Result<(), String> {
    let there = read(file)?.unwrap_or_default();
    let blocks = block::find(&there).map(|b| b.len());
    if there != text || blocks != Ok(usize::from(with_block)) {
        return Err(format!(
            "{} doesn't read back as Sawit wrote it: check it, and its copy in Sawit's data folder.",
            file.display()
        ));
    }
    Ok(())
}
