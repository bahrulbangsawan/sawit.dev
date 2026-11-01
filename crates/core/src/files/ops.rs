//! Creating, moving, copying and deleting files and folders, and a free name for a copy.

use std::path::{Path, PathBuf};

use crate::Result;

/// Makes an empty file or folder; fails if something is already there.
pub fn create(path: &Path, dir: bool) -> Result<()> {
    if dir {
        std::fs::create_dir(path)?;
    } else {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
    }
    Ok(())
}

/// Renames or moves `from` to `to`, refusing to replace anything there.
pub fn move_entry(from: &Path, to: &Path) -> Result<()> {
    if to.starts_with(from) && to != from {
        return Err(std::io::Error::other("A folder can't be moved into itself.").into());
    }
    if std::fs::symlink_metadata(to).is_ok() && !same_entry(from, to) {
        return Err(already_exists(to));
    }
    match std::fs::rename(from, to) {
        // Another volume: copy, then remove the original.
        Err(e) if e.raw_os_error() == Some(18) => {
            copy_entry(from, to)?;
            remove(from)
        }
        other => Ok(other?),
    }
}

/// Copies a file, or a folder and everything in it (links stay links).
/// Refuses to replace anything at `to`.
///
/// Only files, folders and links are copied. A folder's pipes, sockets and devices are left
/// out: copying a pipe opens it, which waits for a writer for ever, a socket can't be opened, and
/// a copy of either would be of no use. One of them on its own is refused. A folder copy that
/// fails part way is removed, so nothing half made is left that nothing could undo.
pub fn copy_entry(from: &Path, to: &Path) -> Result<()> {
    if to.starts_with(from) {
        return Err(std::io::Error::other("A folder can't be copied into itself.").into());
    }
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(already_exists(to));
    }
    if !copied(&std::fs::symlink_metadata(from)?.file_type()) {
        let name = from.file_name().unwrap_or_default().to_string_lossy();
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("“{name}” is a pipe, socket or device, so it can't be copied."),
        )
        .into());
    }
    copy_tree(from, to)?;
    Ok(())
}

/// Whether [`copy_entry`] copies an entry of this type: a file, a folder or a link.
fn copied(kind: &std::fs::FileType) -> bool {
    kind.is_file() || kind.is_dir() || kind.is_symlink()
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
    } else if kind.is_dir() {
        std::fs::create_dir(to)?;
        let filled = (|| {
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                copy_tree(&entry.path(), &to.join(entry.file_name()))?;
            }
            std::fs::set_permissions(to, meta.permissions())
        })();
        if filled.is_err() {
            // Made just now, so it holds only what this copy put there.
            let _ = std::fs::remove_dir_all(to);
        }
        filled
    } else if kind.is_file() {
        std::fs::copy(from, to).map(|_| ())
    } else {
        // A pipe, socket or device in a folder: left out.
        Ok(())
    }
}

/// Deletes a file or a whole folder, for good.
pub fn remove(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Whether `a` and `b` name one entry in two cases: names that differ only in case, in one
/// folder, for one file on disk. On a volume that ignores case (macOS's default) a rename between
/// them changes the case of the name. On one that tells case apart (a case-sensitive APFS volume,
/// a share), `README.md` and `readme.md` can be two files, and renaming one to the other would
/// replace it; then they aren't one entry.
fn same_entry(a: &Path, b: &Path) -> bool {
    a.parent() == b.parent()
        && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
        && same_file(a, b)
}

/// Whether `a` and `b` lead to one file, folder or link on disk (one device and inode; links
/// aren't followed). False when either can't be looked up.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => false,
    }
}

fn already_exists(path: &Path) -> crate::CoreError {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("“{name}” already exists here."),
    )
    .into()
}

/// `dir/name` if nothing is there, else the first free "name copy",
/// "name copy 2", … (before the extension, for files).
pub fn free_name(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let path = dir.join(name);
    if std::fs::symlink_metadata(&path).is_err() {
        return path;
    }
    let as_path = Path::new(name);
    let (stem, ext) = match (is_dir, as_path.file_stem(), as_path.extension()) {
        (false, Some(stem), Some(ext)) => (
            stem.to_string_lossy().into_owned(),
            format!(".{}", ext.to_string_lossy()),
        ),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{stem} copy{ext}")),
            n => dir.join(format!("{stem} copy {n}{ext}")),
        })
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .expect("some name is free")
}
