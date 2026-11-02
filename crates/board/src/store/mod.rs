//! Where the canvas file lives, how it is opened, and the writer thread that saves it.

#[cfg(test)]
use std::path::Path;
#[cfg(test)]
use std::sync::mpsc;

mod backup;
mod open;
#[cfg(test)]
mod tests;
mod writer;

pub use backup::{
    BACKUPS, Backup, backup_after, backup_file, deletion_backup_file, is_empty, removes_layers,
};
pub use open::{board_file, bound_file, import_file, open, open_bound, open_with_issues};
pub use writer::{Stamp, Writer};

/// The most a canvas file may hold to be opened: 4 GiB, as much as all the images FigCraft reads
/// from one file (its `IMAGE_BUDGET`), and far more than a canvas holds. The file is read whole,
/// on the UI thread, before it is opened.
pub const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;

/// Runs `f` on a thread of its own and gives what it returned, or `None` when it hadn't returned
/// within 5 s (the thread is left as it is): for a read that could block for ever.
#[cfg(test)]
pub(crate) fn within_seconds<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
}

/// Makes a named pipe at `path`.
#[cfg(test)]
pub(crate) fn pipe_at(path: &Path) {
    let made = std::process::Command::new("mkfifo").arg(path).status();
    assert!(made.unwrap().success(), "mkfifo {}", path.display());
}
