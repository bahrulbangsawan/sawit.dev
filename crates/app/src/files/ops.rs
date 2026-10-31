//! File changes the tree can undo. Running one returns the change that
//! reverses it, so undo and redo are the same step.

use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSURL};
use sawit_core::files;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Create {
        path: PathBuf,
        dir: bool,
    },
    Move {
        from: PathBuf,
        to: PathBuf,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
    },
    Trash(PathBuf),
    /// Puts back what Trash moved from `to` to `from`, in the Trash: the undo of a Trash. Its
    /// own undo, the redo, is a Trash again, through the Trash rather than a rename into it.
    Restore {
        from: PathBuf,
        to: PathBuf,
    },
}

impl Op {
    /// Does it (blocking); returns the op that undoes it.
    pub fn run(self) -> Result<Op, String> {
        self.run_with(trash)
    }

    /// [`Self::run`], moving things to the Trash with `trash`.
    fn run_with(self, trash: impl FnOnce(&Path) -> Result<PathBuf, String>) -> Result<Op, String> {
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
            Op::Trash(path) => Op::Restore {
                from: trash(&path)?,
                to: path,
            },
            Op::Restore { from, to } => {
                files::move_entry(&from, &to).map_err(|e| e.to_string())?;
                Op::Trash(to)
            }
        };
        Ok(inverse)
    }

    /// Where the change leaves something to select: `None` once it is gone.
    pub fn result(&self) -> Option<&Path> {
        match self {
            Op::Create { path, .. } => Some(path),
            Op::Move { to, .. } | Op::Copy { to, .. } | Op::Restore { to, .. } => Some(to),
            Op::Trash(_) => None,
        }
    }

    /// What it changes: the file or folder it moves, copies or trashes, or for a new one, the
    /// folder it goes in.
    pub fn source(&self) -> &Path {
        match self {
            Op::Create { path, .. } => path.parent().unwrap_or(path),
            Op::Move { from, .. } | Op::Copy { from, .. } | Op::Restore { from, .. } => from,
            Op::Trash(path) => path,
        }
    }

    /// What it renames or moves, from where to where, for the tabs of what moved to follow. Not
    /// what goes to or comes back from the Trash: its tabs say it was deleted, and then that it
    /// is back, where it was.
    pub fn moved(&self) -> Option<(&Path, &Path)> {
        match self {
            Op::Move { from, to } => Some((from, to)),
            _ => None,
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::Op;
    use crate::files::Scratch;

    /// Undo of a Trash puts the item back, and redo moves it to the Trash again, through the
    /// Trash (FILES-2). The undo used to be a plain move out of the Trash, and the redo, its
    /// undo, a rename back into it: not through the Trash, and reported as a move, so the tabs
    /// of the file followed it into the Trash and saved there, and the Trash item was selected.
    #[test]
    fn redo_of_a_trash_trashes_again() {
        let d = Scratch::new("redo-of-a-trash");
        let (project, bin) = (d.0.join("project"), d.0.join("Trash"));
        fs::create_dir(&project).unwrap();
        fs::create_dir(&bin).unwrap();
        let file = project.join("notes.md");
        fs::write(&file, "notes").unwrap();
        let trashed = Cell::new(0);
        let trash = |path: &Path| -> Result<PathBuf, String> {
            trashed.set(trashed.get() + 1);
            let to = bin.join(format!("notes {}.md", trashed.get()));
            fs::rename(path, &to).map_err(|e| e.to_string())?;
            Ok(to)
        };

        // ⌫, then ⌘Z: the file is back, and selected; nothing moved for the tabs to follow.
        let undo = Op::Trash(file.clone()).run_with(trash).unwrap();
        assert!(!file.exists());
        assert_eq!(undo.moved(), None);
        assert_eq!(undo.result(), Some(file.as_path()));
        let redo = undo.run_with(trash).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "notes");

        // ⇧⌘Z: through the Trash again, and nothing to select.
        assert!(matches!(&redo, Op::Trash(p) if *p == file), "{redo:?}");
        assert_eq!((redo.moved(), redo.result()), (None, None));
        let undo = redo.run_with(trash).unwrap();
        assert_eq!(trashed.get(), 2);
        assert!(!file.exists() && bin.join("notes 2.md").exists());
        assert_eq!(undo.moved(), None);

        // A rename is a move both ways.
        let rename = Op::Move {
            from: bin.join("notes 2.md"),
            to: project.join("kept.md"),
        };
        assert!(rename.moved().is_some());
        assert!(rename.run_with(trash).unwrap().moved().is_some());
    }
}
