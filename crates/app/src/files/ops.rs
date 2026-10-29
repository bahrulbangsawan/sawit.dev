//! File changes the tree can undo. Running one returns the change that
//! reverses it, so undo and redo are the same step.

use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSURL};
use sawit_core::files;

#[derive(Clone, Debug)]
pub enum Op {
    Create { path: PathBuf, dir: bool },
    Move { from: PathBuf, to: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
    Trash(PathBuf),
}

impl Op {
    /// Does it (blocking); returns the op that undoes it.
    pub fn run(self) -> Result<Op, String> {
        let inverse = match self {
            Op::Create { path, dir } => {
                files::create(&path, dir).map_err(|e| e.to_string())?;
                Op::Trash(path)
            }
            Op::Move { from, to } => {
                files::move_entry(&from, &to).map_err(|e| e.to_string())?;
                Op::Move { from: to, to: from }
            }
            Op::Copy { from, to } => {
                files::copy_entry(&from, &to).map_err(|e| e.to_string())?;
                Op::Trash(to)
            }
            Op::Trash(path) => Op::Move {
                from: trash(&path)?,
                to: path,
            },
        };
        Ok(inverse)
    }

    /// Where the change leaves something to select: `None` once it is gone.
    pub fn result(&self) -> Option<&Path> {
        match self {
            Op::Create { path, .. } => Some(path),
            Op::Move { to, .. } | Op::Copy { to, .. } => Some(to),
            Op::Trash(_) => None,
        }
    }
}

/// Moves `path` to the Trash (Finder's Put Back works); returns where it went.
fn trash(path: &Path) -> Result<PathBuf, String> {
    let url = NSURL::from_file_path(path).ok_or("Not a file path.")?;
    let mut resulting = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|e| e.localizedDescription().to_string())?;
    resulting
        .and_then(|url| url.to_file_path())
        .ok_or_else(|| "Moved to the Trash, but its new location is unknown.".into())
}
