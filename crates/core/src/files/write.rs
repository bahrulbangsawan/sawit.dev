//! Saving files atomically: through a private temp file beside the file, synced to disk, then
//! renamed over it.

use std::path::{Path, PathBuf};

use crate::Result;

/// Writes via a private temp file in the same directory + rename, keeping
/// the original file's permissions. A symlink stays a symlink: its target is
/// what gets replaced ([`write_atomic`]).
pub fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    write_atomic(path, text.as_bytes())
}

/// Writes `bytes` to `path` so that a crash or power cut leaves the old file
/// or the new one, never part of either: to a private temp file in the same
/// directory ([`staging_file`]), synced to disk, then renamed over the file.
/// The file keeps its permissions (a new one gets 0644). A symlink stays a
/// symlink: its target is what gets replaced. Nothing else is written through
/// a link ([`replace_file`]).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let path = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let mode = std::fs::metadata(&path)
        .map(|m| m.permissions())
        .unwrap_or_else(|_| std::fs::Permissions::from_mode(0o644));
    replace_file(&path, bytes, mode)
}

/// Puts a file of `bytes`, with permissions `mode`, at `path` in place of
/// whatever is there, as [`write_atomic`] writes: through a private temp file
/// beside it, synced to disk, then renamed over `path`. A symlink at `path`
/// is replaced, never written through, unlike [`write_atomic`]'s: for copies
/// Sawit keeps beside a file (a canvas's backups), where a link can only have
/// come from somewhere else, a cloned repository say.
///
/// The temp file has a name no other write uses ([`staging_file`]), and is
/// made new, owner-only, and given `mode` through its own handle. So a file or
/// a link already at one of those names (a cloned repository can hold one) is
/// never truncated, followed or chmodded, and two writers of one file, in one
/// process or two, never share a temp file. On failure it is removed, and
/// `path` is as it was.
pub fn replace_file(path: &Path, bytes: &[u8], mode: std::fs::Permissions) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static STAGED: AtomicU64 = AtomicU64::new(0);
    let names =
        std::iter::repeat_with(|| staging_file(path, STAGED.fetch_add(1, Ordering::Relaxed)));
    Ok(stage_and_rename(
        path,
        bytes,
        mode,
        names.take(STAGING_TRIES),
    )?)
}

/// How many temp file names [`replace_file`] tries before it gives up: only a
/// leftover of a process that had this one's id, or a name made on purpose,
/// can be in the way of one.
const STAGING_TRIES: usize = 3;

/// [`replace_file`] staging the file at the first of `names` that is free.
pub(super) fn stage_and_rename(
    path: &Path,
    bytes: &[u8],
    mode: std::fs::Permissions,
    names: impl IntoIterator<Item = PathBuf>,
) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    for tmp in names {
        // Made new (`O_EXCL`, which never follows a link), owner-only until it is complete.
        let made = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp);
        let mut file = match made {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let result = (|| {
            file.write_all(bytes)?;
            file.set_permissions(mode)?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        return result;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!(
            "no free name to write {} through beside it",
            path.file_name().unwrap_or_default().to_string_lossy()
        ),
    ))
}

/// Where [`replace_file`] (and so [`write_atomic`]) can put the bytes for the
/// file at `path` before they replace it: `.<name>.sawit-tmp-<process>-<n>`
/// beside it, hidden, and of this process alone. Each write takes the next `n`.
pub fn staging_file(path: &Path, n: u64) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!(".{name}.sawit-tmp-{}-{n}", std::process::id()))
}
