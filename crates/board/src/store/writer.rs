//! The writer thread that saves the canvas, and the stamps that keep its saves from replacing what
//! something else wrote to a file since.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::SystemTime;

use sawit_core::{files, nyawit};

use super::backup::{Backup, keep_backup};

/// A file's length and modification time, as the canvas read or wrote it: when the file there has
/// another stamp, something else wrote to it since. FigCraft stamps a document's file the same way
/// (`library::stat`, #1330), but keeps its stamp to itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    /// The stamp of the file at `path` as it is now, through a symlink. `None` when there is no
    /// file there: nothing, a folder, or a path that can't be looked up.
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok().filter(|meta| meta.is_file())?;
        Some(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// Saves the canvas on its own thread, so editing never waits for the disk.
///
/// Saves are queued, and when several for one path are waiting only the newest is written. Each
/// goes to a private file beside the canvas first, is synced to disk and is then renamed over the
/// file ([`files::write_atomic`]), so a crash or power cut half way can't leave a truncated canvas.
/// The file keeps its permissions, and a symlink stays one: its target gets the save. A canvas
/// tab's writer also keeps its saves from replacing what something else wrote to its file since
/// ([`Writer::guard`]). Dropping the writer writes what is queued, then ends the thread.
pub struct Writer {
    pub(super) tx: Sender<Msg>,
    pub(super) error: Arc<Mutex<Option<String>>>,
    /// What a save that wrote a guarded file again where it had gone left to say
    /// ([`Writer::take_notice`]).
    pub(super) notice: Arc<Mutex<Option<String>>>,
    /// The files guarded against saving over another app's or window's changes, with the stamp
    /// of each as the canvas last read or wrote it ([`Writer::guard`]).
    pub(super) stamps: Arc<Stamps>,
    pub(super) thread: Option<JoinHandle<()>>,
}

/// The guarded files' stamps, which the writer thread checks and updates: `None` while there was
/// no file to stamp.
type Stamps = Mutex<HashMap<PathBuf, Option<Stamp>>>;

/// What goes to the writer thread.
pub(super) enum Msg {
    Save(Save),
    /// Reply on this channel once everything queued before it is on disk.
    Flush(Sender<()>),
    /// Write what is queued, then end the thread.
    Stop,
}

pub(super) struct Save {
    pub(super) path: PathBuf,
    pub(super) bytes: Vec<u8>,
    /// The backup of the canvas the file holds that is kept first, if any ([`keep_backup`]).
    pub(super) backup: Backup,
}

impl Writer {
    /// Starts the writer thread, called "canvas-save".
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let notice = Arc::new(Mutex::new(None));
        let stamps = Arc::new(Stamps::default());
        let started = std::thread::Builder::new()
            .name("canvas-save".into())
            .spawn({
                let (error, notice, stamps) = (error.clone(), notice.clone(), stamps.clone());
                move || run(&rx, &error, &notice, &stamps)
            });
        let thread = match started {
            Ok(thread) => Some(thread),
            Err(e) => {
                // The queue went with the thread, so every save will report that too.
                *lock(&error) = Some(format!("The canvas couldn't start saving: {e}"));
                None
            }
        };
        Self {
            tx,
            error,
            notice,
            stamps,
            thread,
        }
    }

    /// Queues `bytes` to be written to `path`, and returns at once. `backup` says the save takes
    /// the board off the canvas ([`Backup::Board`]): the bytes are a canvas without layers
    /// ([`is_empty`]), or another canvas loaded in its place ([`Writer::save_with`]).
    pub fn save(&self, path: PathBuf, bytes: Vec<u8>, backup: bool) {
        let backup = if backup { Backup::Board } else { Backup::None };
        self.save_with(path, bytes, backup);
    }

    /// Queues `bytes` to be written to `path`, keeping the `backup` it asks for first, and returns
    /// at once.
    ///
    /// With a backup, a file already there that has layers, or that doesn't load, is first kept
    /// beside it, as `<path>.bak` for [`Backup::Board`] and `<path>.del.bak` for
    /// [`Backup::Deletion`], the backups of that kind before it moving along ([`BACKUPS`] of each
    /// are kept), so clearing, replacing or deleting from the canvas can be undone after a
    /// restart; if that copy can't be made, the save is not done and the error is reported. Such
    /// a save is never folded into a newer save of the file, nor the save before it into it
    /// ([`Batch::add`]), so the backup is of the board as it was last handed over. A `.nyawit`
    /// folder that is missing is made with its `.gitignore`, any other missing folder without. A
    /// save over a guarded file that something else wrote to since is refused
    /// ([`Writer::guard`]), its backup included.
    pub fn save_with(&self, path: PathBuf, bytes: Vec<u8>, backup: Backup) {
        let save = Msg::Save(Save {
            path,
            bytes,
            backup,
        });
        if let Err(mpsc::SendError(Msg::Save(lost))) = self.tx.send(save) {
            *lock(&self.error) = Some(format!(
                "The canvas couldn't be saved to {}: its writer has stopped",
                lost.path.display()
            ));
        }
    }

    /// Keeps saves to `path` from replacing what another app or window wrote to the file since the
    /// canvas read or saved it, as FigCraft keeps its own saves from it (#1330). `read` is the
    /// file's stamp from before the canvas read it ([`Stamp::of`]), `None` when there was no file.
    ///
    /// A save to `path` is then refused, right before it would be written, when the writer holds a
    /// stamp of the file and the file there now has another: the stamp it was given, then that of
    /// each save of the file it wrote, so the canvas's own saves never refuse each other. With no
    /// stamp held (there was no file, and none has been written yet) a file there now is refused
    /// too: something made it since the canvas looked, another window's canvas of the same file
    /// say, or a `git pull` (CANVAS-CORE-5). No file there now writes as before, so the first save
    /// of a canvas whose file isn't there yet (a new one, a migrated or an imported one) makes it.
    /// A refusal is the save's error ([`Writer::last_error`]) and leaves the file as it is, and
    /// so does every later save of it, until the canvas opens the file again. A path that isn't
    /// guarded is saved as before, whatever wrote to it since.
    pub fn guard(&self, path: PathBuf, read: Option<Stamp>) {
        lock(&self.stamps).insert(path, read);
    }

    /// The guarded file at `from` was renamed or moved to `to`: saves to `to` are guarded from
    /// now on with the stamp held of it at `from`, as the canvas last read or wrote it, which the
    /// move kept ([`Writer::guard`]). Call it once nothing queued for `from` is still to be written
    /// ([`Writer::flush`]).
    pub fn follow(&self, from: &Path, to: PathBuf) {
        let mut stamps = lock(&self.stamps);
        if let Some(held) = stamps.remove(from) {
            stamps.insert(to, held);
        }
    }

    /// Waits until everything queued so far is on disk or has failed ([`Writer::last_error`]).
    pub fn flush(&self) {
        let (ack, done) = mpsc::channel();
        // A thread that has stopped has nothing left to write, and drops `ack` on its way out, so
        // neither a failed save nor a dead thread leaves this waiting.
        if self.tx.send(Msg::Flush(ack)).is_ok() {
            let _ = done.recv();
        }
    }

    /// A sentence on why the newest save failed, for a toast. `None` when it went through, or none
    /// was made. A later save that goes through clears it; a [`Writer::flush`] alone does not.
    pub fn last_error(&self) -> Option<String> {
        lock(&self.error).clone()
    }

    /// Takes the note a save left when it wrote a guarded file again where it had gone: the file
    /// the canvas read or wrote was no longer there (moved, renamed, put in the Trash or deleted
    /// without the canvas being told), so the save made it again, as FigCraft's own save does,
    /// rather than lose the edits ([`Writer::guard`]). A sentence for a toast, so that this isn't
    /// silent (CANVAS-UI-3). `None` when no save did since the last time.
    pub fn take_notice(&self) -> Option<String> {
        lock(&self.notice).take()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // The thread writes what is queued before it stops, so the newest save is not lost.
        if let Some(thread) = self.thread.take() {
            let _ = self.tx.send(Msg::Stop);
            let _ = thread.join();
        }
    }
}

/// What the writer thread took off its queue in one go.
#[derive(Default)]
pub(super) struct Batch {
    /// The saves to write, in order: the newest of each path, and those a save that keeps a
    /// backup keeps from being folded away ([`Batch::add`]).
    pub(super) saves: Vec<Save>,
    /// Who waits for these saves to be on disk.
    pub(super) flushes: Vec<Sender<()>>,
    pub(super) stop: bool,
}

impl Batch {
    /// Takes `msg` into the batch. A save replaces the save of its path queued last, so that only
    /// the newest is written, unless either of them keeps a backup ([`Writer::save_with`]): then
    /// both are written, in order. The backup of a save that takes layers off the canvas is of
    /// the file the save before it wrote, the board as the canvas last had it (CANVAS-CORE-3);
    /// and a save that keeps a backup is always written, so its backup is always made.
    pub(super) fn add(&mut self, msg: Msg) {
        match msg {
            Msg::Save(save) => {
                let last = self.saves.iter().rposition(|older| older.path == save.path);
                if let Some(last) = last
                    && self.saves[last].backup == Backup::None
                    && save.backup == Backup::None
                {
                    self.saves.remove(last);
                }
                self.saves.push(save);
            }
            Msg::Flush(ack) => self.flushes.push(ack),
            Msg::Stop => self.stop = true,
        }
    }
}

/// The writer thread: take everything queued, write the newest save of each path, then answer
/// whoever waits. A save that wrote a guarded file again where it had gone leaves its note in
/// `notice`.
fn run(
    rx: &Receiver<Msg>,
    error: &Mutex<Option<String>>,
    notice: &Mutex<Option<String>>,
    stamps: &Stamps,
) {
    while let Ok(first) = rx.recv() {
        let mut batch = Batch::default();
        batch.add(first);
        while let Ok(next) = rx.try_recv() {
            batch.add(next);
        }
        // A batch of only flushes saved nothing, so it says nothing about the last save.
        if !batch.saves.is_empty() {
            let mut failure = None;
            for save in &batch.saves {
                match write(save, stamps) {
                    Ok(Some(note)) => *lock(notice) = Some(note),
                    Ok(None) => {}
                    Err(e) => failure = Some(e),
                }
            }
            *lock(error) = failure;
        }
        for ack in batch.flushes {
            let _ = ack.send(());
        }
        if batch.stop {
            break;
        }
    }
}

/// Writes one save. On failure the error is a sentence for the user.
///
/// A save of a guarded file ([`Writer::guard`]) is refused right before its write when the file
/// changed since the stamp held of it, before anything else is done: its backup isn't made
/// either. Once written, the file's new stamp is the one held. When it had gone (the writer held
/// a stamp of it, and nothing is there now), the save makes it again, and gives a note that says
/// so ([`Writer::take_notice`]).
fn write(save: &Save, stamps: &Stamps) -> Result<Option<String>, String> {
    let path = &save.path;
    let failed = |why: String| format!("The canvas couldn't be saved to {}: {why}", path.display());
    let guarded = lock(stamps).get(path).copied();
    let gone = matches!(guarded, Some(Some(_))) && Stamp::of(path).is_none();
    if let Some(held) = guarded {
        refuse_overwriting_changes(path, held)?;
    }
    make_folder(path).map_err(|e| failed(e.to_string()))?;
    if let Some(suffix) = save.backup.suffix() {
        keep_backup(path, suffix).map_err(failed)?;
    }
    files::write_atomic(path, &save.bytes).map_err(|e| failed(e.to_string()))?;
    if guarded.is_some() {
        // The canvas's own write: the next save is checked against it, not against the file as
        // it was read.
        lock(stamps).insert(path.clone(), Stamp::of(path));
    }
    Ok(gone.then(|| written_again(path)))
}

/// The note of a save that wrote the guarded file at `path` again where it had gone. Closing the
/// tab leaves that file there, so it says to delete it again too; and a save in flight during a
/// rename lands after the tab follows the new name, when closing the tab isn't the whole answer.
fn written_again(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let folder = path.parent().unwrap_or(path);
    format!(
        "{name} was no longer in {} (moved, renamed or deleted since it was opened or saved \
         here), so the canvas saved it there again. If it was meant to go, close its tab, then \
         delete the file again.",
        folder.display()
    )
}

/// Refuses a save over the file at `path` when something else wrote to it since the canvas read
/// or saved it: the file there now has another stamp than `held`, or there was no file
/// (`held` is `None`) and there is one now (CANVAS-CORE-5). No file there has nothing to
/// replace. It is FigCraft's own rule and sentence (`refuse_overwriting_changes`, #1330), with
/// Sawit's way out in place of FigCraft's Revert to saved and Save as: Save as first, which keeps
/// this version (CANVAS-CORE-4); a tab that closes keeps it in a copy too, beside the file.
fn refuse_overwriting_changes(path: &Path, held: Option<Stamp>) -> Result<(), String> {
    let name = path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("Untitled");
    match (held, Stamp::of(path)) {
        (Some(held), Some(now)) if now != held => Err(format!(
            "{name} wasn't saved: the file changed on disk since it was opened or saved here \
             (another app or window wrote to it), and saving would replace those changes. Save \
             it under another name to keep this version, or close its tab and open the file \
             again to load those changes."
        )),
        (None, Some(_)) => Err(format!(
            "{name} wasn't saved: a file was made on disk at its place since it was opened here \
             (another app or window wrote it), and saving would replace it. Save it under \
             another name to keep this version, or close its tab and open the file again to load \
             that one."
        )),
        _ => Ok(()),
    }
}

/// Makes the folder `path` goes in. A `.nyawit/` that is new gets its `.gitignore` too
/// ([`nyawit::ensure`]), but only then: deleting that file is how a user chooses to commit the
/// canvas, and the next autosave must not put it back.
fn make_folder(path: &Path) -> io::Result<()> {
    let Some(folder) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Ok(());
    };
    match folder.parent() {
        Some(project) if folder.file_name() == Some(OsStr::new(nyawit::DIR)) => {
            if folder.is_dir() {
                Ok(())
            } else {
                nyawit::ensure(project, folder)
            }
        }
        _ => std::fs::create_dir_all(folder),
    }
}

/// What the writer shares with its thread (its last error, the stamps of the files it guards),
/// locked. A poisoned lock still holds a usable value.
fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}
