//! Project files for the text editor: directory listing, safe reading
//! (size and encoding limits) and atomic saving.

mod ignore;
mod kind;
mod ops;
mod read;
mod write;

pub use ignore::{add_ignore_line, ignore_pattern};
pub use kind::{CANVAS_EXTENSION, FileKind, classify, language_for, language_label};
pub use ops::{copy_entry, create, free_name, move_entry, remove};
pub use read::{TextFile, check_regular, read_regular, read_text};
pub use write::{replace_file, staging_file, write_atomic, write_text_atomic};

use std::path::{Path, PathBuf};

use crate::Result;

/// Files larger than this open as a read-only notice instead of text.
pub const MAX_TEXT_BYTES: u64 = 5 * 1024 * 1024;

pub(crate) const HIDDEN: [&str; 6] = [
    ".git",
    "node_modules",
    "target",
    ".DS_Store",
    ".superpowers",
    ".turbo",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Directory contents: folders first, then files, each case-insensitively
/// sorted; VCS/build noise hidden.
pub fn list_dir(dir: &Path) -> Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            let kind = e.file_type().ok()?;
            let path = e.path();
            // A link to a folder lists as a folder; nothing else needs following.
            let is_dir = kind.is_dir() || (kind.is_symlink() && path.is_dir());
            Some(Entry { name, path, is_dir })
        })
        .collect();
    // Each name lowercased once, not twice per comparison.
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
    Ok(entries)
}

/// `path` with the links on its way resolved, as far as it exists: the longest part of it that
/// is there, as [`std::fs::canonicalize`] gives it, then the rest as written. Two names of one
/// place give one path this way, whether the file is there yet or not: on macOS
/// `/tmp/p/.nyawit/canvas.sawit` and `/private/tmp/p/.nyawit/canvas.sawit`, `/tmp` being a
/// link, before `.nyawit/` is made. A path none of which can be looked up stays as it is.
pub fn resolve(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut there = path;
    loop {
        if let Ok(real) = there.canonicalize() {
            return rest.iter().rev().fold(real, |real, name| real.join(name));
        }
        match (there.parent(), there.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                there = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests;
