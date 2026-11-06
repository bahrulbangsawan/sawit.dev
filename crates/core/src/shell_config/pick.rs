//! Which shell files an upload or a download moves, as both `shell_files_sync` and `sawit shell`
//! choose them: only the files named, a file changed on both sides only when the caller chose a
//! side, and never a file that has nothing to move that way. And the lines of an upload that look
//! like secrets, which stop it unless the caller allowed them.

use crate::github::FileState;

use super::{FileSync, SyncStatus, find_secrets};

/// The way files move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// This Mac's version to GitHub: a file changed here, or the same on both.
    Upload,
    /// GitHub's version to this Mac.
    Download,
}

impl Direction {
    fn fits(self, state: FileState) -> bool {
        match self {
            Direction::Upload => state.uploads() || state == FileState::Same,
            Direction::Download => state.downloads(),
        }
    }
}

/// The files of `status` named in `names` (their path in the repository, `.zshrc`) that move
/// `way`. Refused, saying why: no names, a name that isn't one of the shell files, a file changed
/// on both sides without `overwrite`, or one with nothing to move that way.
pub fn pick_files<'a>(
    status: &'a SyncStatus,
    names: &[String],
    overwrite: bool,
    way: Direction,
) -> Result<Vec<&'a FileSync>, String> {
    if names.is_empty() {
        return Err("Name the files (as the status lists them).".into());
    }
    names
        .iter()
        .map(|name| {
            let f = status
                .files
                .iter()
                .find(|f| &f.rel == name)
                .ok_or_else(|| format!("{name} isn't one of the shell files the status lists."))?;
            match f.state {
                FileState::ChangedOnBoth if !overwrite => Err(format!(
                    "{name} changed both here and on GitHub: choose a side first (overwrite)."
                )),
                FileState::ChangedOnBoth => Ok(f),
                s if way.fits(s) => Ok(f),
                s => Err(format!(
                    "{name} is {}: nothing to move that way.",
                    s.label()
                )),
            }
        })
        .collect()
}

/// The lines of `files` that look like secrets, as `~/.zshrc:12 OPENAI_API_KEY`.
pub fn secret_lines(files: &[&FileSync]) -> Vec<String> {
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f.path).unwrap_or_default();
        for (line, name) in find_secrets(&text) {
            out.push(format!("~/{}:{line} {name}", f.rel));
        }
    }
    out
}
