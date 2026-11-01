//! Where the canvas file lives, how it is opened, and the writer thread that saves it.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::SystemTime;

use figcraft_doc::{Document, NodeId};
use sawit_core::{files, nyawit};

use crate::doc::{BoardDoc, BoardError};
use crate::excalidraw;

/// What a new canvas is called.
const NAME: &str = "Canvas";

/// The file the project canvas was in before the native canvas
/// (`<project>/.nyawit/canvas.excalidraw`, [`nyawit::legacy_canvas_file`]), which a canvas file
/// that isn't there yet is imported from. It is found beside the canvas file by its name, because
/// the path of the canvas file is all the store is given.
const OLD_CANVAS: &str = "canvas.excalidraw";

/// The canvas file: `<project>/.nyawit/canvas.figcraft`, else `canvas.figcraft` in `app_root`
/// (Sawit's own folder).
pub fn board_file(project: Option<&Path>, app_root: &Path) -> PathBuf {
    match project {
        Some(project) => nyawit::board_file(project),
        None => app_root.join(nyawit::BOARD_FILE),
    }
}

/// Opens the canvas at `path`, as [`open_with_issues`] does (so it imports the old canvas too), and
/// only logs its notes.
pub fn open(path: &Path) -> Result<BoardDoc, BoardError> {
    let (board, issues) = open_with_issues(path)?;
    for issue in issues {
        log::warn!(
            "canvas file {} loaded with an issue: {issue}",
            path.display()
        );
    }
    Ok(board)
}

/// Opens the canvas at `path`, with the notes on what saving it back would lose
/// ([`BoardDoc::from_bytes_with_issues`]).
///
/// - A missing file is a new, empty board. Nothing is written, not even the folder: the first
///   save does that.
/// - A missing canvas file (`canvas.figcraft`, [`nyawit::BOARD_FILE`]) with a `canvas.excalidraw`
///   beside it is that old canvas, imported ([`excalidraw::import`]). It is the first open only:
///   once the canvas has been saved the file is there, and is what opens. Neither file is
///   written, and no `.bak` is made: the old canvas stays as it is, and the notes are what the
///   import left out. An old canvas with nothing in it (empty, or only white space) is a blank
///   board, as it was on the old canvas, and has nothing to say. Any other old canvas that can't be
///   imported is an error that names it, and then nothing is made either; no new board takes its
///   place, because the first save would hide it.
/// - A corrupt or truncated file is `Err(BoardError::Format)`, and stays as it is.
/// - Only a regular file is read, of at most [`MAX_FILE`] bytes ([`files::read_regular`]): a
///   named pipe, or a link a cloned repository holds to a device such as `/dev/zero`, is refused
///   at once, where its read blocked the UI thread for ever, or until the Mac ran out of memory
///   (CANVAS-CORE-6). The error, `Err(BoardError::Io)`, says so.
/// - A file opens with its version history and branches, which a save writes back
///   ([`BoardDoc::to_bytes`]). Images past the loader's limits stay in the file, unread, so a save
///   over it is refused ([`BoardDoc::refuse_dropping_images`]); one elsewhere isn't.
/// - With notes, the original is first copied to `<path>.lossy.bak`, so the first save can't
///   silently drop what the loader left out. When that copy can't be made the file is not opened
///   either: it stays as it is rather than get saved over unprotected. The copy has a name of its
///   own: the backups that an emptying save keeps ([`Writer::save`]) move along, and go.
pub fn open_with_issues(path: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let bytes = match files::read_regular(path, MAX_FILE) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return open_missing(path),
        Err(e) => {
            return Err(BoardError::Io(format!(
                "couldn't read {}: {e}",
                path.display()
            )));
        }
    };
    let (board, issues) = BoardDoc::from_file_bytes(&bytes, path)?;
    if !issues.is_empty() {
        let bak = lossy_backup(path);
        keep_copy(&bak, &bytes, path).map_err(|e| {
            BoardError::Io(format!(
                "couldn't keep a copy of {} at {} before opening it: {e}",
                path.display(),
                bak.display()
            ))
        })?;
    }
    Ok((board, issues))
}

/// Keeps `bytes`, what the canvas file `of` holds, as the copy at `bak`, with the file's
/// permissions: in place of whatever is at `bak`, a link included, which is never written through
/// ([`files::replace_file`], CANVAS-CORE-1). A cloned repository can hold a link there to any
/// file of the user's.
fn keep_copy(bak: &Path, bytes: &[u8], of: &Path) -> Result<(), sawit_core::CoreError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(of).map_or(0o644, |meta| meta.permissions().mode() & 0o777);
    files::replace_file(bak, bytes, std::fs::Permissions::from_mode(mode))
}

/// The most a canvas file may hold to be opened: 4 GiB, as much as all the images FigCraft reads
/// from one file (its `IMAGE_BUDGET`), and far more than a canvas holds. The file is read whole,
/// on the UI thread, before it is opened.
pub const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;

/// What the canvas file `path`, which isn't there, opens as: the old canvas beside it imported,
/// else a new board.
fn open_missing(path: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    match old_canvas(path) {
        Some(old) => import_old_canvas(&old),
        None => Ok((BoardDoc::new_board(NAME)?, Vec::new())),
    }
}

/// The old canvas that the canvas file `path` replaces: `canvas.excalidraw` in the same folder.
/// Only the canvas file takes over from it, not any other `.figcraft` file that is missing.
fn old_canvas(path: &Path) -> Option<PathBuf> {
    if path.file_name() != Some(OsStr::new(nyawit::BOARD_FILE)) {
        return None;
    }
    let old = path.with_file_name(OLD_CANVAS);
    old.is_file().then_some(old)
}

/// A board that holds the old canvas `old`, and the notes on what the import left out, as
/// [`import_file`] imports a file.
fn import_old_canvas(old: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    import_as(old, NAME)
}

/// The `.figcraft` file that a canvas opened on the file at `path` is bound to, and saves to (spec
/// D14): for an `.excalidraw` file the `<stem>.figcraft` beside it (so the old project canvas is
/// bound to the canvas file), for any other file the file itself.
pub fn bound_file(path: &Path) -> PathBuf {
    let excalidraw = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("excalidraw"));
    if excalidraw {
        path.with_extension("figcraft")
    } else {
        path.to_path_buf()
    }
}

/// The `.excalidraw` file at `path` as a new board named after it, and the notes on what the
/// import left out ([`excalidraw::to_board`]). The board opens as a file does, with no undo step.
/// Nothing is written, and the file stays as it is.
///
/// A file with nothing in it (empty, or only white space) is a blank board with nothing to say,
/// as it was on the old canvas ([`excalidraw::read_file_or_blank`]). Any other file that can't be
/// imported is an error that names it.
pub fn import_file(path: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let name = path
        .file_stem()
        .map_or_else(|| NAME.into(), |stem| stem.to_string_lossy());
    import_as(path, &name)
}

/// The `.excalidraw` file at `path` imported as a new board called `name`, and the notes on what
/// the import left out ([`import_file`]).
fn import_as(path: &Path, name: &str) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let scene = excalidraw::read_file_or_blank(path)?;
    excalidraw::to_board(name, &scene)
        .map_err(|e| BoardError::Format(format!("{} can't be imported: {e}", path.display())))
}

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
/// backup first ([`Writer::save`]). A page alone is no layer: one deleted with nothing on it takes
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
    tx: Sender<Msg>,
    error: Arc<Mutex<Option<String>>>,
    /// What a save that wrote a guarded file again where it had gone left to say
    /// ([`Writer::take_notice`]).
    notice: Arc<Mutex<Option<String>>>,
    /// The files guarded against saving over another app's or window's changes, with the stamp
    /// of each as the canvas last read or wrote it ([`Writer::guard`]).
    stamps: Arc<Stamps>,
    thread: Option<JoinHandle<()>>,
}

/// The guarded files' stamps, which the writer thread checks and updates: `None` while there was
/// no file to stamp.
type Stamps = Mutex<HashMap<PathBuf, Option<Stamp>>>;

/// What goes to the writer thread.
enum Msg {
    Save(Save),
    /// Reply on this channel once everything queued before it is on disk.
    Flush(Sender<()>),
    /// Write what is queued, then end the thread.
    Stop,
}

struct Save {
    path: PathBuf,
    bytes: Vec<u8>,
    /// The canvas the file holds is kept as a backup first ([`keep_backup`]).
    backup: bool,
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

    /// Queues `bytes` to be written to `path`, and returns at once.
    ///
    /// `backup` says the save takes the board off the canvas: the bytes are a canvas without
    /// layers ([`is_empty`]), or another canvas loaded in its place. Then a file already there
    /// that has layers, or that doesn't load, is first kept as a backup beside it, `<path>.bak`,
    /// the backups before it moving along ([`BACKUPS`] are kept), so clearing or replacing the
    /// canvas can be undone after a restart; if that copy can't be made, the save is not done and
    /// the error is reported. Such a save is never folded into a newer save of the file, nor the
    /// save before it into it ([`Batch::add`]), so the backup is of the board as it was last
    /// handed over. A `.nyawit` folder that is missing is made with its `.gitignore`, any other
    /// missing folder without. A save over a guarded file that something else wrote to since is
    /// refused ([`Writer::guard`]), its backup included.
    pub fn save(&self, path: PathBuf, bytes: Vec<u8>, backup: bool) {
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
struct Batch {
    /// The saves to write, in order: the newest of each path, and those a save that keeps a
    /// backup keeps from being folded away ([`Batch::add`]).
    saves: Vec<Save>,
    /// Who waits for these saves to be on disk.
    flushes: Vec<Sender<()>>,
    stop: bool,
}

impl Batch {
    /// Takes `msg` into the batch. A save replaces the save of its path queued last, so that only
    /// the newest is written, unless either of them keeps a backup ([`Writer::save`]): then both
    /// are written, in order. The backup of a save that takes the board off the canvas is of
    /// the file the save before it wrote, the board as the canvas last had it (CANVAS-CORE-3);
    /// and a save that keeps a backup is always written, so its backup is always made.
    fn add(&mut self, msg: Msg) {
        match msg {
            Msg::Save(save) => {
                let last = self.saves.iter().rposition(|older| older.path == save.path);
                if let Some(last) = last
                    && !self.saves[last].backup
                    && !save.backup
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
    if save.backup {
        keep_backup(path).map_err(failed)?;
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

/// How many backups of a canvas file are kept beside it ([`keep_backup`]): `<name>.bak`, the
/// newest, then `<name>.bak.1` to `<name>.bak.4`, each older than the one before.
pub const BACKUPS: usize = 5;

/// The backup of the canvas file `path` that `n` newer ones came after ([`keep_backup`]):
/// `<path>.bak`, the newest, for 0, else `<path>.bak.<n>`.
pub fn backup_file(path: &Path, n: usize) -> PathBuf {
    match n {
        0 => sibling(path, ".bak"),
        n => sibling(path, &format!(".bak.{n}")),
    }
}

/// Before a save that takes the board off the canvas replaces the file at `path` (a canvas
/// without layers, or another canvas loaded in its place): when that file loads as a canvas
/// with layers, keeps it as the newest backup, `<path>.bak`, so "clear canvas" (or an agent's
/// clear or load) can be undone even after the app restarts. So is a file that doesn't load at
/// all (cut short, or written by a newer FigCraft), which may hold layers too, as the old canvas
/// kept a scene it couldn't read; an empty file has nothing to keep. On failure the error says
/// why, and the save must not go on.
///
/// The backups before it move along one name each, `.bak` to `.bak.1` and so on, and the oldest
/// of [`BACKUPS`] goes (CANVAS-CORE-3): one `.bak`, replaced by the next clear, lost the board an
/// agent cleared first when it started over twice. A board the newest backup holds already isn't
/// kept again, so that a clear of a board cleared before moves none along. A folder at one of
/// their names stops it, and the save: it is moved nowhere.
fn keep_backup(path: &Path) -> Result<(), String> {
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
    let newest = backup_file(path, 0);
    if holds(&newest, &old) {
        return Ok(());
    }
    for n in (1..BACKUPS).rev() {
        let (from, to) = (backup_file(path, n - 1), backup_file(path, n));
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

/// Where [`open_with_issues`] keeps the original of a file it opens with notes:
/// `<path>.lossy.bak`. That is apart from the backups an emptying save keeps ([`backup_file`]),
/// so that clearing the canvas after edits can't put the edited canvas over the original.
fn lossy_backup(path: &Path) -> PathBuf {
    sibling(path, ".lossy.bak")
}

/// `path` with `suffix` added to its name: `canvas.figcraft` becomes `canvas.figcraft.bak`.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// What the writer shares with its thread (its last error, the stamps of the files it guards),
/// locked. A poisoned lock still holds a usable value.
fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use figcraft_doc::{Document, NodeKind};
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::doc::{DocKind, file_with_a_damaged_image, file_with_an_image_past_the_limit};

    /// A board with one sticky for each of `texts`.
    fn board_with(texts: &[&str]) -> BoardDoc {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        for (n, text) in texts.iter().enumerate() {
            let sticky = json!({"x": n as f64 * 300.0, "y": 0, "text": text});
            board.execute("board.sticky", &sticky).unwrap();
        }
        board
    }

    fn bytes_with(texts: &[&str]) -> Vec<u8> {
        board_with(texts).to_bytes().unwrap()
    }

    fn blank() -> Vec<u8> {
        bytes_with(&[])
    }

    fn has_text(board: &BoardDoc, text: &str) -> bool {
        let doc = board.doc().unwrap();
        doc.nodes()
            .any(|n| n.kind.text().is_some_and(|t| t.characters == text))
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A `.figcraft` file of a board with a sticky "kept", and a version history.
    fn file_with_a_history() -> Vec<u8> {
        let doc = board_with(&["kept"]).doc().unwrap();
        let history = [figcraft_format::Version::new(
            "v1",
            "First",
            "",
            1,
            doc.clone(),
        )];
        figcraft_format::save_bytes_with_versions(&doc, None, &history).unwrap()
    }

    #[test]
    fn board_file_paths() {
        let project = Path::new("/work/app");
        let root = Path::new("/support/sawit");
        assert_eq!(
            board_file(Some(project), root),
            project.join(".nyawit/canvas.figcraft")
        );
        assert_eq!(board_file(None, root), root.join("canvas.figcraft"));
    }

    #[test]
    fn open_missing_file_is_a_new_board_and_writes_nothing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".nyawit/canvas.figcraft");
        let board = open(&path).unwrap();
        assert_eq!(board.kind(), DocKind::Board);
        assert!(!path.exists());
        assert!(names(dir.path()).is_empty(), "not even the folder is made");
        let (board, issues) = open_with_issues(&path).unwrap();
        assert_eq!(board.kind(), DocKind::Board);
        assert!(issues.is_empty());
    }

    #[test]
    fn open_corrupt_file_errors_and_never_saves() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        std::fs::write(&path, b"not a figcraft file").unwrap();
        assert!(matches!(open(&path), Err(BoardError::Format(_))));
        assert!(matches!(
            open_with_issues(&path),
            Err(BoardError::Format(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"not a figcraft file");
        // No `.bak` or temporary file beside it either.
        assert_eq!(names(dir.path()), ["canvas.figcraft"]);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_io_error() {
        let dir = tempdir().unwrap();
        // A folder where the file should be.
        let path = dir.path().join("canvas.figcraft");
        std::fs::create_dir(&path).unwrap();
        let err = open(&path).err().unwrap();
        assert!(matches!(err, BoardError::Io(_)), "{err:?}");
        assert!(err.to_string().contains("canvas.figcraft"), "{err}");
    }

    /// CANVAS-CORE-6: opening a canvas reads its file on the UI thread (and in sawit-mcp, holding
    /// every other canvas call back). A file that is a named pipe blocked it for ever, and one a
    /// cloned repository links to `/dev/zero` read zeros until the Mac ran out of memory. Both
    /// are refused at once now, as an `.excalidraw` file opened from Files is.
    #[test]
    fn a_canvas_file_that_is_a_pipe_is_refused_at_once() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        pipe_at(&path);
        let opened = within_seconds(move || open_with_issues(&path).map(|_| ()));
        let error = opened.expect("opening a pipe never returned").unwrap_err();
        assert!(matches!(error, BoardError::Io(_)), "{error:?}");
        let error = error.to_string();
        assert!(
            error.contains("canvas.figcraft") && error.contains("not a regular file"),
            "{error}"
        );
        let scene = dir.path().join("sketch.excalidraw");
        pipe_at(&scene);
        let imported = within_seconds(move || import_file(&scene).map(|_| ()));
        let error = imported
            .expect("importing a pipe never returned")
            .unwrap_err();
        let error = error.to_string();
        assert!(
            error.contains("sketch.excalidraw") && error.contains("not a regular file"),
            "{error}"
        );
    }

    /// CANVAS-CORE-6: a canvas file a cloned repository links to `/dev/zero` is refused at once.
    /// Read, it never ended (the probe's process grew past 2.5 GB in a quarter of a second), so
    /// this has no run against the old read.
    #[test]
    fn a_canvas_file_linked_to_a_device_is_refused_at_once() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        std::os::unix::fs::symlink("/dev/zero", &path).unwrap();
        let opened = within_seconds(move || open_with_issues(&path).map(|_| ()));
        let error = opened
            .expect("opening /dev/zero never returned")
            .unwrap_err();
        assert!(error.to_string().contains("not a regular file"), "{error}");
    }

    /// CANVAS-CORE-6: a canvas file past [`MAX_FILE`] is refused before it is read. The file is
    /// sparse: it takes no room on the disk.
    #[test]
    fn a_canvas_file_past_the_limit_is_refused_unread() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_FILE + 1).unwrap();
        let opened = within_seconds(move || open_with_issues(&path).map(|_| ()));
        let error = opened.expect("opening never returned").unwrap_err();
        assert!(matches!(error, BoardError::Io(_)), "{error:?}");
        assert!(error.to_string().contains("more than 4096 MB"), "{error}");
    }

    #[test]
    fn open_with_issues_backs_up_the_original() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let bak = lossy_backup(&path);
        assert_eq!(bak, dir.path().join("canvas.figcraft.lossy.bak"));
        let original = file_with_a_damaged_image();
        std::fs::write(&path, &original).unwrap();

        let (board, issues) = open_with_issues(&path).unwrap();
        assert!(has_text(&board, "kept"));
        assert!(!issues.is_empty());
        assert_eq!(std::fs::read(&bak).unwrap(), original);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "opening saves nothing"
        );

        // `open` does the same, and only drops the notes.
        std::fs::remove_file(&bak).unwrap();
        assert!(open(&path).is_ok());
        assert_eq!(std::fs::read(&bak).unwrap(), original);
    }

    #[test]
    fn open_with_issues_leaves_a_healthy_file_alone() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let original = bytes_with(&["hi"]);
        std::fs::write(&path, &original).unwrap();

        let (board, issues) = open_with_issues(&path).unwrap();
        assert!(has_text(&board, "hi"));
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(names(dir.path()), ["canvas.figcraft"], "no backup");
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn open_with_issues_refuses_a_file_it_cannot_back_up() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let original = file_with_a_damaged_image();
        std::fs::write(&path, &original).unwrap();
        // A folder where the backup should go.
        std::fs::create_dir(lossy_backup(&path)).unwrap();

        let err = open_with_issues(&path).err().unwrap();
        assert!(matches!(err, BoardError::Io(_)), "{err:?}");
        assert!(
            err.to_string().contains("canvas.figcraft.lossy.bak"),
            "{err}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn a_file_with_a_history_opens_without_notes_or_a_backup() {
        // A save writes the history back, so there is nothing to warn about or to keep a copy of.
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        std::fs::write(&path, file_with_a_history()).unwrap();

        let (board, issues) = open_with_issues(&path).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(has_text(&board, "kept"));
        assert_eq!(board.session.ds().unwrap().versions.len(), 1);
        assert_eq!(names(dir.path()), ["canvas.figcraft"], "no backup");
    }

    #[test]
    fn images_left_in_a_file_make_saving_over_it_refused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("design.figcraft");
        let original = file_with_an_image_past_the_limit();
        std::fs::write(&path, &original).unwrap();

        let (board, issues) = open_with_issues(&path).unwrap();
        assert!(!issues.is_empty());
        let refused = board.refuse_dropping_images(&path).unwrap_err().to_string();
        assert!(
            refused.starts_with("design wasn't saved: 1 of its images weren't loaded"),
            "{refused}"
        );
        // A copy anywhere else loses nothing of this file.
        let copy = dir.path().join("copy.figcraft");
        assert!(board.refuse_dropping_images(&copy).is_ok());
        // `open` records them too, and the original is kept beside the file, as for any open
        // with notes.
        assert!(open(&path).unwrap().refuse_dropping_images(&path).is_err());
        assert_eq!(std::fs::read(lossy_backup(&path)).unwrap(), original);
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn an_excalidraw_file_is_bound_to_the_figcraft_beside_it() {
        let bound = |path: &str| bound_file(Path::new(path));
        assert_eq!(
            bound("/p/sketch.excalidraw"),
            Path::new("/p/sketch.figcraft")
        );
        assert_eq!(bound("/p/v1.2.Excalidraw"), Path::new("/p/v1.2.figcraft"));
        // The old project canvas is bound to the project canvas.
        assert_eq!(
            bound("/p/.nyawit/canvas.excalidraw"),
            nyawit::board_file(Path::new("/p"))
        );
        // A `.figcraft` file is bound to itself.
        assert_eq!(bound("/p/board.figcraft"), Path::new("/p/board.figcraft"));
    }

    #[test]
    fn importing_a_file_draws_it_on_a_board_named_after_it_and_writes_nothing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sketch.excalidraw");
        std::fs::write(&path, FIXTURE).unwrap();

        let (board, issues) = import_file(&path).unwrap();
        assert_eq!(board.kind(), DocKind::Board);
        assert_eq!(top_level(&board), 9);
        assert_eq!(board.doc().unwrap().name, "sketch");
        assert_eq!(issues, ["skipped 1 embed"]);
        // It opens as a file does: nothing to undo.
        assert_eq!(board.session.undo_depth(), (0, 0));
        // The file is as it was, and nothing is made beside it.
        assert_eq!(std::fs::read(&path).unwrap(), FIXTURE.as_bytes());
        assert_eq!(names(dir.path()), ["sketch.excalidraw"]);
    }

    #[test]
    fn a_file_with_nothing_in_it_imports_blank_and_any_other_failure_names_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sketch.excalidraw");
        for blank in [
            &b""[..],
            b" \n",
            br#"{"type": "excalidraw", "elements": []}"#,
        ] {
            std::fs::write(&path, blank).unwrap();
            let (board, issues) = import_file(&path).unwrap();
            assert!(is_empty(&board.doc().unwrap()), "{blank:?}");
            assert!(issues.is_empty(), "{blank:?}: {issues:?}");
        }
        for (scene, says) in [
            (&b"{ not json"[..], "isn't valid JSON"),
            (br#"{"type": "excalidraw"}"#, "`elements`"),
        ] {
            std::fs::write(&path, scene).unwrap();
            let error = import_file(&path).err().unwrap();
            assert!(matches!(error, BoardError::Format(_)), "{error:?}");
            let message = error.to_string();
            assert!(
                message.contains("sketch.excalidraw") && message.contains(says),
                "{message}"
            );
        }
        assert_eq!(names(dir.path()), ["sketch.excalidraw"]);
    }

    /// The old canvas as Excalidraw saved it: nine layers on the page, a frame's child among the
    /// ones inside, an embed that can't be imported, and a deleted rectangle.
    const FIXTURE: &str = include_str!("../tests/fixtures/excalidraw/basic.excalidraw");

    /// How many layers are on the first page, not counting what is inside them.
    fn top_level(board: &BoardDoc) -> usize {
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        doc.node(page).unwrap().children.len()
    }

    #[test]
    fn migration_imports_once_and_keeps_original() {
        let dir = tempdir().unwrap();
        let nyawit = dir.path().join(".nyawit");
        std::fs::create_dir(&nyawit).unwrap();
        let old = nyawit.join("canvas.excalidraw");
        let path = nyawit.join("canvas.figcraft");
        std::fs::write(&old, FIXTURE).unwrap();

        // The first open imports the old canvas. Nothing is written: not the new file, and the
        // old one is as it was.
        let mut board = open(&path).unwrap();
        assert_eq!(board.kind(), DocKind::Board);
        assert_eq!(top_level(&board), 9);
        assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
        assert!(!path.exists());
        assert_eq!(names(&nyawit), ["canvas.excalidraw"]);

        // The first save writes the native canvas, and from then on that is what opens.
        board
            .execute("board.sticky", &json!({"x": 2000, "y": 0, "text": "after"}))
            .unwrap();
        let writer = Writer::spawn();
        writer.save(path.clone(), board.to_bytes().unwrap(), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());

        // Not imported again: the nine layers and the sticky, not the old canvas a second time.
        let again = open(&path).unwrap();
        assert!(has_text(&again, "after"));
        assert_eq!(top_level(&again), 10);
        assert_eq!(names(&nyawit), ["canvas.excalidraw", "canvas.figcraft"]);
    }

    /// A project folder whose `.nyawit/` holds the old canvas `scene`: the folder, the old canvas
    /// and the canvas file that isn't there.
    fn project_with_old_canvas(scene: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempdir().unwrap();
        let nyawit = dir.path().join(".nyawit");
        std::fs::create_dir(&nyawit).unwrap();
        let old = nyawit.join("canvas.excalidraw");
        std::fs::write(&old, scene).unwrap();
        (dir, old, nyawit.join("canvas.figcraft"))
    }

    #[test]
    fn migration_says_what_it_left_out_and_makes_no_backup() {
        let (dir, old, path) = project_with_old_canvas(FIXTURE.as_bytes());
        let nyawit = dir.path().join(".nyawit");

        let (board, issues) = open_with_issues(&path).unwrap();
        assert_eq!(top_level(&board), 9);
        // The notes of the import are what is shown, as for any file that opens with something said.
        assert_eq!(issues, ["skipped 1 embed"]);
        // Neither file is written, and nothing is kept beside them: there is no `.bak`, because
        // the old canvas isn't touched and no canvas file was there to save over.
        assert_eq!(names(&nyawit), ["canvas.excalidraw"]);
        assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());

        // A canvas the import left nothing out of opens without a word.
        let clean = br#"{"type": "excalidraw", "elements": [
            {"id": "a", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10}]}"#;
        let (dir, _, path) = project_with_old_canvas(clean);
        let (board, issues) = open_with_issues(&path).unwrap();
        assert_eq!(top_level(&board), 1);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(names(&dir.path().join(".nyawit")), ["canvas.excalidraw"]);
    }

    #[test]
    fn a_migrated_canvas_opens_as_a_file_does_with_no_undo_step() {
        let (_dir, _, path) = project_with_old_canvas(FIXTURE.as_bytes());
        let board = open(&path).unwrap();
        assert_eq!(board.kind(), DocKind::Board);
        // Undoing is for what the user did after it opened: it can't take the canvas away.
        assert_eq!(board.session.undo_depth(), (0, 0));
        assert!(board.session.selection().is_empty());
        // The document is a board's, and what a save writes of it opens as the same canvas.
        let doc = board.doc().unwrap();
        assert_eq!(doc.pages.len(), 1);
        let again = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
        assert_eq!(again.kind(), DocKind::Board);
        assert_eq!(top_level(&again), 9);
        assert_eq!(again.doc().unwrap().nodes().count(), doc.nodes().count());
    }

    #[test]
    fn only_the_canvas_file_takes_over_from_the_old_canvas() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("canvas.excalidraw"), FIXTURE).unwrap();

        // Any other file that is missing is a new board, even beside a `canvas.excalidraw`.
        let other = open(&dir.path().join("other.figcraft")).unwrap();
        assert_eq!(top_level(&other), 0);
        let named_alike = open(&dir.path().join("my-canvas.figcraft")).unwrap();
        assert_eq!(top_level(&named_alike), 0);
        // The canvas of a window with no project is `canvas.figcraft` in Sawit's own folder, with
        // the old one beside it.
        let app = open(&board_file(None, dir.path())).unwrap();
        assert_eq!(top_level(&app), 9);
        assert_eq!(names(dir.path()), ["canvas.excalidraw"]);

        // No old canvas, or one that is a folder, is nothing to import.
        let empty = tempdir().unwrap();
        assert_eq!(
            top_level(&open(&empty.path().join("canvas.figcraft")).unwrap()),
            0
        );
        std::fs::create_dir(empty.path().join("canvas.excalidraw")).unwrap();
        let (board, issues) = open_with_issues(&empty.path().join("canvas.figcraft")).unwrap();
        assert_eq!(top_level(&board), 0);
        assert!(issues.is_empty());
    }

    #[test]
    fn the_old_canvas_is_never_imported_over_a_canvas_file_that_is_there() {
        let (_dir, old, path) = project_with_old_canvas(FIXTURE.as_bytes());
        std::fs::write(&path, bytes_with(&["native"])).unwrap();
        let board = open(&path).unwrap();
        assert!(has_text(&board, "native"));
        assert_eq!(top_level(&board), 1);

        // Not when the canvas file is corrupt either: it stays as it is, and so does the old one.
        std::fs::write(&path, b"not a figcraft file").unwrap();
        assert!(matches!(open(&path), Err(BoardError::Format(_))));
        assert_eq!(std::fs::read(&path).unwrap(), b"not a figcraft file");
        assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
    }

    #[test]
    fn an_old_canvas_with_nothing_on_it_opens_blank() {
        let deleted = br#"{"type": "excalidraw", "elements": [
            {"id": "a", "type": "rectangle", "isDeleted": true}]}"#;
        // A scene with nothing drawn, and a file with nothing in it at all: empty, only white
        // space, or only a byte order mark. Today's canvas opens each of them blank, and there is
        // nothing in them to protect.
        let lone_bom = "\u{feff}".as_bytes();
        let spaced_bom = "\u{feff} \r\n\t".as_bytes();
        for scene in [
            &deleted[..],
            br#"{"elements": []}"#,
            b"",
            b" \n",
            lone_bom,
            spaced_bom,
        ] {
            let (dir, old, path) = project_with_old_canvas(scene);
            for (board, issues) in [
                open_with_issues(&path).unwrap(),
                (open(&path).unwrap(), Vec::new()),
            ] {
                assert_eq!(board.kind(), DocKind::Board, "{scene:?}");
                assert_eq!(top_level(&board), 0, "{scene:?}");
                assert!(issues.is_empty(), "{scene:?}: {issues:?}");
                assert!(is_empty(&board.doc().unwrap()));
                // Opened as a file is: nothing to undo.
                assert_eq!(board.session.undo_depth(), (0, 0));
            }
            // Neither file is written, and nothing is kept beside them.
            assert!(!path.exists(), "{scene:?}");
            assert_eq!(std::fs::read(&old).unwrap(), scene);
            assert_eq!(names(&dir.path().join(".nyawit")), ["canvas.excalidraw"]);
        }
    }

    #[test]
    fn an_old_canvas_that_cannot_be_imported_is_an_error_and_nothing_is_made() {
        // A new board would take its place at the first save, and hide it.
        let invalid_utf8 = [0xFF, 0xFE, 0x00];
        for (scene, kind, says) in [
            (&b"{ not json"[..], "format", "isn't valid JSON"),
            (&br#"{"type": "excalidraw"}"#[..], "format", "`elements`"),
            (&br#"[1, 2]"#[..], "format", "`elements`"),
            (&invalid_utf8[..], "io", "couldn't read"),
        ] {
            let (dir, old, path) = project_with_old_canvas(scene);
            for error in [
                open(&path).err().unwrap(),
                open_with_issues(&path).err().unwrap(),
            ] {
                let right_kind = match kind {
                    "format" => matches!(error, BoardError::Format(_)),
                    _ => matches!(error, BoardError::Io(_)),
                };
                assert!(right_kind, "{error:?}");
                let message = error.to_string();
                assert!(
                    message.contains("canvas.excalidraw") && message.contains(says),
                    "{message}"
                );
            }
            assert!(!path.exists());
            assert_eq!(std::fs::read(&old).unwrap(), scene);
            assert_eq!(names(&dir.path().join(".nyawit")), ["canvas.excalidraw"]);
        }
    }

    #[test]
    fn is_empty_means_no_layers_on_any_page() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        assert!(is_empty(&board.doc().unwrap()));
        let sticky = board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        assert!(!is_empty(&board.doc().unwrap()));
        board
            .execute("edit.delete", &json!({"ids": [sticky["id"]]}))
            .unwrap();
        assert!(is_empty(&board.doc().unwrap()));

        // A layer on a page other than the first counts too; no pages at all is empty.
        let mut doc = Document::new("Two pages");
        let second = doc.add_page("Page 2");
        assert!(is_empty(&doc));
        doc.create(second, NodeKind::Rectangle, "R", 0.0, 0.0, 10.0, 10.0)
            .unwrap();
        assert!(!is_empty(&doc));
        assert!(is_empty(&Document::without_pages("None")));
    }

    /// A save takes layers off the canvas when a layer the board had, on any page and at any
    /// depth, is gone from what it saves: then the file's board is kept as a backup first. An
    /// edit that adds, moves or renames layers takes nothing off, nor does deleting an empty page.
    #[test]
    fn a_layer_gone_from_any_page_is_a_save_that_takes_layers_off() {
        let mut board = board_with(&["one", "two"]);
        let page = board.doc().unwrap().pages[0];
        let first = board.doc().unwrap().node(page).unwrap().children[0];
        for (command, params, takes_off) in [
            ("board.sticky", json!({"x": 900, "y": 0}), false),
            ("node.set", json!({"id": first, "x": 40, "y": 40}), false),
            ("node.set", json!({"id": first, "name": "First"}), false),
            ("edit.delete", json!({"ids": [first]}), true),
        ] {
            let before = board.doc().unwrap();
            board.execute(command, &params).unwrap();
            let after = board.doc().unwrap();
            assert_eq!(
                removes_layers(&before, &after),
                takes_off,
                "{command} {params}"
            );
        }

        // On another page: a layer in a frame there, the page with it, or an empty page.
        let mut doc = Document::new("Two pages");
        let second = doc.add_page("Page 2");
        let empty = doc.add_page("Page 3");
        let frame = (doc.create(second, NodeKind::Frame, "F", 0.0, 0.0, 400.0, 400.0)).unwrap();
        let nested = (doc.create(frame, NodeKind::Rectangle, "R", 10.0, 10.0, 50.0, 50.0)).unwrap();
        let without = |id| {
            let mut after = doc.clone();
            after.remove(id).unwrap();
            removes_layers(&doc, &after)
        };
        assert!(without(nested), "a nested layer");
        assert!(without(second), "a page with layers on it");
        assert!(!without(empty), "a page with nothing on it");
        assert!(!removes_layers(&doc, &doc.clone()));
    }

    #[test]
    fn writer_creates_nyawit_with_gitignore() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".nyawit/canvas.figcraft");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["hi"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&path).unwrap(), "hi"));
        let ignore = dir.path().join(".nyawit/.gitignore");
        assert!(std::fs::read_to_string(ignore).unwrap().ends_with("*\n"));
    }

    #[test]
    fn a_gitignore_the_user_deleted_stays_deleted() {
        // "Delete this file to commit them": the next autosave must not put it back.
        let dir = tempdir().unwrap();
        let path = dir.path().join(".nyawit/canvas.figcraft");
        let ignore = dir.path().join(".nyawit/.gitignore");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["one"]), false);
        writer.flush();
        assert!(ignore.exists());
        std::fs::remove_file(&ignore).unwrap();
        writer.save(path.clone(), bytes_with(&["two"]), false);
        writer.flush();
        assert!(has_text(&open(&path).unwrap(), "two"));
        assert!(!ignore.exists());
    }

    #[test]
    fn other_folders_are_made_without_a_gitignore() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a/b/canvas.figcraft");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["hi"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&path).unwrap(), "hi"));
        assert_eq!(names(&dir.path().join("a/b")), ["canvas.figcraft"]);
        assert_eq!(names(&dir.path().join("a")), ["b"]);
    }

    #[test]
    fn newest_save_wins() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["first"]), false);
        writer.save(path.clone(), bytes_with(&["second"]), false);
        writer.flush();
        let board = open(&path).unwrap();
        assert!(has_text(&board, "second"));
        assert!(!has_text(&board, "first"));
    }

    #[test]
    fn a_burst_of_saves_leaves_the_newest_of_each_path() {
        let dir = tempdir().unwrap();
        let (one, two) = (
            dir.path().join("one.figcraft"),
            dir.path().join("two.figcraft"),
        );
        let saves: Vec<(Vec<u8>, Vec<u8>)> = (0..40)
            .map(|n| {
                (
                    bytes_with(&[&format!("one {n}")]),
                    bytes_with(&[&format!("two {n}")]),
                )
            })
            .collect();
        let writer = Writer::spawn();
        // Queued faster than the thread writes, so most are replaced before they reach the disk.
        for (a, b) in saves {
            writer.save(one.clone(), a, false);
            writer.save(two.clone(), b, false);
        }
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&one).unwrap(), "one 39"));
        assert!(has_text(&open(&two).unwrap(), "two 39"));
    }

    #[test]
    fn a_batch_keeps_the_newest_save_of_each_path() {
        let save = |path: &str, byte: u8| {
            Msg::Save(Save {
                path: path.into(),
                bytes: vec![byte],
                backup: false,
            })
        };
        let mut batch = Batch::default();
        for msg in [save("a", 1), save("b", 2), save("a", 3)] {
            batch.add(msg);
        }
        let (ack, _) = std::sync::mpsc::channel();
        batch.add(Msg::Flush(ack));
        let mut kept: Vec<(PathBuf, Vec<u8>)> = batch
            .saves
            .iter()
            .map(|s| (s.path.clone(), s.bytes.clone()))
            .collect();
        kept.sort();
        assert_eq!(kept, [("a".into(), vec![3]), ("b".into(), vec![2])]);
        assert_eq!(batch.flushes.len(), 1);
        assert!(!batch.stop);
    }

    #[test]
    fn emptying_keeps_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["keep me"]), false);
        writer.flush();
        writer.save(path.clone(), blank(), true);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        let backup = open(&sibling(&path, ".bak")).unwrap();
        assert!(!is_empty(&backup.doc().unwrap()));
        assert!(has_text(&backup, "keep me"));
        assert!(is_empty(&open(&path).unwrap().doc().unwrap()));
    }

    #[test]
    fn backups_are_made_only_when_layers_are_emptied() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let bak = sibling(&path, ".bak");
        let writer = Writer::spawn();
        let save = |bytes: Vec<u8>, empty: bool| {
            writer.save(path.clone(), bytes, empty);
            writer.flush();
        };
        // Nothing on disk yet: nothing to keep.
        save(blank(), true);
        assert!(!bak.exists());
        // A save that keeps layers keeps no backup, even over a file with layers.
        save(bytes_with(&["one"]), false);
        let two = bytes_with(&["two"]);
        save(two.clone(), false);
        assert!(!bak.exists());
        // Emptying keeps the file as it was...
        save(blank(), true);
        assert_eq!(std::fs::read(&bak).unwrap(), two);
        // ...and emptying an empty canvas again leaves that backup alone.
        save(blank(), true);
        assert_eq!(std::fs::read(&bak).unwrap(), two);
        assert_eq!(writer.last_error(), None);
    }

    #[test]
    fn clearing_after_a_lossy_open_keeps_the_original() {
        // Opened with notes, edited and saved (the save drops the damaged image), then cleared,
        // as an agent's add then clear does: the emptying save keeps the edited canvas, and the
        // original stays where the open put it.
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let original = file_with_a_damaged_image();
        std::fs::write(&path, &original).unwrap();
        let (_, issues) = open_with_issues(&path).unwrap();
        assert!(!issues.is_empty());

        let writer = Writer::spawn();
        let edited = bytes_with(&["kept", "added"]);
        writer.save(path.clone(), edited.clone(), false);
        writer.flush();
        writer.save(path.clone(), blank(), true);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert_eq!(std::fs::read(sibling(&path, ".bak")).unwrap(), edited);
        assert_eq!(std::fs::read(lossy_backup(&path)).unwrap(), original);
    }

    /// What the backups beside the canvas file `path` hold: `<name>.bak`, then any older one, in
    /// the order of their names.
    fn backups(path: &Path) -> Vec<Vec<u8>> {
        let dir = path.parent().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        names(dir)
            .into_iter()
            .filter(|n| n.starts_with(&format!("{name}.bak")))
            .map(|n| std::fs::read(dir.join(n)).unwrap())
            .collect()
    }

    /// CANVAS-CORE-3: an agent that starts over twice, a clear, a drawing and a clear again, left
    /// one `.bak`, of its own drawing: the board that was there first, which the first clear kept,
    /// was in no file any more. Each backup now moves the ones before it along, so it is still
    /// there.
    #[test]
    fn clearing_twice_keeps_the_board_cleared_first() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let first = bytes_with(&["the user's board"]);
        std::fs::write(&path, &first).unwrap();
        let writer = Writer::spawn();
        let save = |bytes: Vec<u8>, empty: bool| {
            writer.save(path.clone(), bytes, empty);
            writer.flush();
            assert_eq!(writer.last_error(), None);
        };
        save(blank(), true);
        let drawn = bytes_with(&["the agent's drawing"]);
        save(drawn.clone(), false);
        save(blank(), true);
        let kept = backups(&path);
        assert!(kept.contains(&first), "the first board is in no backup");
        assert!(kept.contains(&drawn), "the drawing is in no backup");
    }

    /// CANVAS-CORE-3: a save that keeps a backup is never folded into another save of its file,
    /// nor the one before it into it: that one is what the backup is of, the board as the canvas
    /// last handed it over. Saves between two such saves still fold, the newest written.
    #[test]
    fn a_batch_writes_a_save_that_keeps_a_backup_and_the_one_before_it() {
        let save = |byte: u8, backup: bool| {
            Msg::Save(Save {
                path: "a".into(),
                bytes: vec![byte],
                backup,
            })
        };
        let mut batch = Batch::default();
        for msg in [
            save(1, false),
            save(2, false),
            save(3, true),
            save(4, false),
            save(5, false),
            save(6, true),
            save(7, true),
        ] {
            batch.add(msg);
        }
        let written: Vec<u8> = batch.saves.iter().map(|s| s.bytes[0]).collect();
        assert_eq!(written, [2, 3, 5, 6, 7]);
    }

    /// CANVAS-CORE-3: the newest backup is `.bak`; each one before it moves one name along, and
    /// the oldest of [`BACKUPS`] goes. A board the newest backup holds already moves none along.
    #[test]
    fn backups_move_along_and_the_oldest_goes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let writer = Writer::spawn();
        let boards: Vec<Vec<u8>> = (0..7)
            .map(|n| bytes_with(&[&format!("board {n}")]))
            .collect();
        for board in &boards {
            writer.save(path.clone(), board.clone(), false);
            writer.save(path.clone(), blank(), true);
            writer.flush();
            assert_eq!(writer.last_error(), None);
        }
        let newest_first: Vec<Vec<u8>> = boards.iter().rev().take(BACKUPS).cloned().collect();
        let kept: Vec<Vec<u8>> = (0..BACKUPS)
            .map(|n| std::fs::read(backup_file(&path, n)).unwrap())
            .collect();
        assert!(
            kept == newest_first,
            "the backups aren't the newest, in order"
        );
        assert!(!backup_file(&path, BACKUPS).exists());
        assert_eq!(
            backup_file(&path, 0).file_name().unwrap(),
            "canvas.figcraft.bak"
        );
        assert_eq!(
            backup_file(&path, 4).file_name().unwrap(),
            "canvas.figcraft.bak.4"
        );

        // The same board cleared again, saved by another app's undo, say.
        std::fs::write(&path, &boards[6]).unwrap();
        writer.save(path.clone(), blank(), true);
        writer.flush();
        let again: Vec<Vec<u8>> = (0..BACKUPS)
            .map(|n| std::fs::read(backup_file(&path, n)).unwrap())
            .collect();
        assert!(
            again == kept,
            "the backups moved along for a board kept already"
        );
    }

    /// CANVAS-CORE-3: a save the changed-on-disk guard refuses changes nothing: not the file, and
    /// not the backups either. The backup used to be made before the check, and replaced the one
    /// there with the other app's file.
    #[test]
    fn a_refused_save_leaves_the_backups_as_they_were() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("design.figcraft");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["kept"]), false);
        writer.flush();
        writer.save(path.clone(), blank(), true);
        writer.flush();
        let before = backups(&path);
        assert_eq!(before.len(), 1);
        std::fs::write(&path, bytes_with(&["read"])).unwrap();
        writer.guard(path.clone(), Stamp::of(&path));
        let theirs = bytes_with(&["theirs", "and more"]);
        std::fs::write(&path, &theirs).unwrap();

        writer.save(path.clone(), blank(), true);
        writer.flush();
        let error = writer.last_error().unwrap();
        assert!(error.contains("changed on disk"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), theirs);
        assert!(backups(&path) == before, "the backup was replaced");
    }

    #[test]
    fn emptying_keeps_a_file_that_does_not_load() {
        // A canvas file another app left unreadable (cut short, or from a newer FigCraft): an
        // emptying save can't tell that it had no layers, so it keeps it, as the old canvas
        // kept a scene it couldn't parse. An empty file has nothing to keep.
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let bak = sibling(&path, ".bak");
        let writer = Writer::spawn();
        let save_blank = || {
            writer.save(path.clone(), blank(), true);
            writer.flush();
        };
        std::fs::write(&path, b"").unwrap();
        save_blank();
        assert!(!bak.exists());

        let unreadable = bytes_with(&["cut short"])[..100].to_vec();
        assert!(figcraft_format::load_bytes(&unreadable).is_err());
        std::fs::write(&path, &unreadable).unwrap();
        save_blank();
        assert_eq!(writer.last_error(), None);
        assert_eq!(std::fs::read(&bak).unwrap(), unreadable);
        assert!(is_empty(&open(&path).unwrap().doc().unwrap()));
    }

    #[test]
    fn a_failed_backup_stops_the_emptying_save() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let full = bytes_with(&["keep me"]);
        let writer = Writer::spawn();
        writer.save(path.clone(), full.clone(), false);
        writer.flush();
        // A folder where the backup should go.
        std::fs::create_dir(sibling(&path, ".bak")).unwrap();
        writer.save(path.clone(), blank(), true);
        writer.flush();
        let error = writer.last_error().unwrap();
        assert!(error.contains("canvas.figcraft.bak"), "{error}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            full,
            "the canvas is as it was"
        );
    }

    #[test]
    fn writer_reports_errors() {
        let dir = tempdir().unwrap();
        // A regular file where the folder should be.
        let blocker = dir.path().join("not-a-folder");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("canvas.figcraft");
        let writer = Writer::spawn();
        assert_eq!(writer.last_error(), None);
        writer.save(path, bytes_with(&["hi"]), false);
        writer.flush();
        let error = writer.last_error().unwrap();
        assert!(error.contains("canvas.figcraft"), "{error}");
        assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
    }

    #[test]
    fn a_later_good_save_clears_the_error() {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("not-a-folder");
        std::fs::write(&blocker, b"x").unwrap();
        let good = dir.path().join("canvas.figcraft");
        let writer = Writer::spawn();
        writer.save(blocker.join("canvas.figcraft"), bytes_with(&["a"]), false);
        writer.flush();
        assert!(writer.last_error().is_some());
        // A flush with nothing queued doesn't count as a save that went through.
        writer.flush();
        assert!(writer.last_error().is_some());
        writer.save(good.clone(), bytes_with(&["b"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&good).unwrap(), "b"));
    }

    #[test]
    fn saves_leave_no_temporary_file() {
        let dir = tempdir().unwrap();
        let good = dir.path().join("good.figcraft");
        // A folder where the file should be: the rename fails, after the temporary file is made.
        let bad = dir.path().join("bad.figcraft");
        std::fs::create_dir(&bad).unwrap();
        let writer = Writer::spawn();
        writer.save(good, bytes_with(&["a"]), false);
        writer.save(bad.clone(), bytes_with(&["b"]), false);
        writer.flush();
        assert!(writer.last_error().is_some());
        assert!(bad.is_dir());
        assert_eq!(names(dir.path()), ["bad.figcraft", "good.figcraft"]);
    }

    #[test]
    fn a_save_goes_through_a_temporary_file() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempdir().unwrap();
        let folder = dir.path().join("boards");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("canvas.figcraft");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["old"]), false);
        writer.flush();
        // A folder nothing can be made in, around a canvas file that can be written: the
        // temporary file can't be made, so the save fails, and the canvas is untouched. Written
        // straight to the file, the save would have gone through.
        let mode = |mode| std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(&folder, mode(0o555)).unwrap();
        writer.save(path.clone(), bytes_with(&["new"]), false);
        writer.flush();
        std::fs::set_permissions(&folder, mode(0o755)).unwrap();
        assert!(writer.last_error().is_some());
        let board = open(&path).unwrap();
        assert!(has_text(&board, "old"));
        assert!(!has_text(&board, "new"));
        assert_eq!(names(&folder), ["canvas.figcraft"]);
    }

    #[test]
    fn a_canvas_file_that_is_a_symlink_stays_one_and_its_target_gets_the_save() {
        // A link the user made to a board kept elsewhere: the save goes to that board, and the
        // link still points at it.
        let dir = tempdir().unwrap();
        let elsewhere = dir.path().join("boards");
        std::fs::create_dir(&elsewhere).unwrap();
        let target = elsewhere.join("plan.figcraft");
        std::fs::write(&target, bytes_with(&["old"])).unwrap();
        let link = dir.path().join("plan.figcraft");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let writer = Writer::spawn();
        writer.save(link.clone(), bytes_with(&["new"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        let link_kind = std::fs::symlink_metadata(&link).unwrap().file_type();
        assert!(link_kind.is_symlink(), "the link was replaced by a copy");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        let saved = open(&target).unwrap();
        assert!(has_text(&saved, "new"));
        assert!(!has_text(&saved, "old"));
        // Nothing is left beside either of them.
        assert_eq!(names(dir.path()), ["boards", "plan.figcraft"]);
        assert_eq!(names(&elsewhere), ["plan.figcraft"]);
    }

    /// CANVAS-CORE-1: a cloned repository can hold links at the names the canvas writes beside its
    /// file: its backups (`.lossy.bak`, `.bak`) and where a save was staged
    /// (`.canvas.figcraft.sawit-tmp`), to any file of the user's (`~/.zshrc`). Opening the file
    /// with notes, emptying it and saving it never write through them: each link is replaced by
    /// the file, or left alone, and what it names stays as it was.
    #[test]
    fn the_canvas_never_writes_through_a_link_beside_its_file() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let folder = dir.path().join(".nyawit");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("canvas.figcraft");
        let original = file_with_a_damaged_image();
        std::fs::write(&path, &original).unwrap();
        let victim = |name: &str| {
            let victim = dir.path().join(name);
            std::fs::write(&victim, format!("{name} of the user's")).unwrap();
            victim
        };
        let (lossy, bak) = (victim("lossy"), victim("bak"));
        symlink(&lossy, lossy_backup(&path)).unwrap();
        symlink(&bak, sibling(&path, ".bak")).unwrap();
        // A link to a file that isn't there yet, by a relative path, as git checks one out.
        symlink("../hook", folder.join(".canvas.figcraft.sawit-tmp")).unwrap();

        // Opened with notes, the original is kept in place of the link.
        let (_, issues) = open_with_issues(&path).unwrap();
        assert!(!issues.is_empty());
        assert_eq!(std::fs::read(lossy_backup(&path)).unwrap(), original);
        let kept = std::fs::symlink_metadata(lossy_backup(&path)).unwrap();
        assert!(kept.file_type().is_file(), "the backup is still a link");
        // Saved, then emptied.
        let writer = Writer::spawn();
        let edited = bytes_with(&["edited"]);
        writer.save(path.clone(), edited.clone(), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        writer.save(path.clone(), blank(), true);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert_eq!(std::fs::read(sibling(&path, ".bak")).unwrap(), edited);
        let kept = std::fs::symlink_metadata(sibling(&path, ".bak")).unwrap();
        assert!(kept.file_type().is_file(), "the backup is still a link");
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_file()
        );
        assert!(is_empty(&open(&path).unwrap().doc().unwrap()));
        // What the links named is as it was, and nothing was made where one pointed.
        assert_eq!(
            std::fs::read_to_string(&lossy).unwrap(),
            "lossy of the user's"
        );
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "bak of the user's");
        assert!(!dir.path().join("hook").exists());
    }

    #[test]
    fn a_saved_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        // A file only its owner may read stays so: a save doesn't make it readable by everyone.
        let dir = tempdir().unwrap();
        let path = dir.path().join("private.figcraft");
        std::fs::write(&path, bytes_with(&["old"])).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["new"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&path).unwrap(), "new"));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "saved as {mode:o}");
    }

    #[test]
    fn a_stamp_tells_a_file_written_since() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        assert_eq!(Stamp::of(&path), None, "no file");
        let folder = dir.path().join("folder.figcraft");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(Stamp::of(&folder), None, "a folder");
        std::fs::write(&path, b"one").unwrap();
        let first = Stamp::of(&path).unwrap();
        assert_eq!(Stamp::of(&path), Some(first), "unchanged");
        std::fs::write(&path, b"three").unwrap();
        assert_ne!(Stamp::of(&path), Some(first), "written since");
        // Through a symlink, it is its target's.
        let link = dir.path().join("link.figcraft");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(Stamp::of(&link), Stamp::of(&path));
    }

    #[test]
    fn a_guarded_file_changed_on_disk_is_not_saved_over() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("design.figcraft");
        std::fs::write(&path, bytes_with(&["read"])).unwrap();
        let writer = Writer::spawn();
        // Stamped before the canvas reads it, as a canvas tab opens its file.
        writer.guard(path.clone(), Stamp::of(&path));
        // Another app (FigCraft, figcraft-cli, git) or another window writes the file.
        let theirs = bytes_with(&["theirs", "and more"]);
        std::fs::write(&path, &theirs).unwrap();

        writer.save(path.clone(), bytes_with(&["mine"]), false);
        writer.flush();
        assert_eq!(
            writer.last_error().as_deref(),
            Some(
                "design wasn't saved: the file changed on disk since it was opened or saved here \
                 (another app or window wrote to it), and saving would replace those changes. \
                 Save it under another name to keep this version, or close its tab and open the \
                 file again to load those changes."
            )
        );
        assert_eq!(std::fs::read(&path).unwrap(), theirs);
        // The canvas's next save is refused too: it still hasn't read what is there.
        writer.save(path.clone(), bytes_with(&["mine", "again"]), false);
        writer.flush();
        assert!(writer.last_error().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), theirs);
        assert_eq!(names(dir.path()), ["design.figcraft"]);
    }

    #[test]
    fn a_writers_own_saves_of_a_guarded_file_all_go_through() {
        let dir = tempdir().unwrap();
        // A file that was there when the canvas read it, and one that wasn't, which its first
        // save makes.
        let read = dir.path().join("read.figcraft");
        std::fs::write(&read, bytes_with(&["read"])).unwrap();
        let new = dir.path().join("new.figcraft");
        let writer = Writer::spawn();
        for path in [&read, &new] {
            writer.guard(path.clone(), Stamp::of(path));
        }
        for n in 0..3 {
            let text = format!("save {n}");
            for path in [&read, &new] {
                writer.save(path.clone(), bytes_with(&[&text]), false);
            }
            writer.flush();
            assert_eq!(writer.last_error(), None, "{text}");
            for path in [&read, &new] {
                assert!(has_text(&open(path).unwrap(), &text), "{text}");
            }
        }
        // Saves queued faster than the thread writes go through too, and so does emptying.
        for n in 0..10 {
            writer.save(read.clone(), bytes_with(&[&format!("burst {n}")]), false);
        }
        writer.flush();
        writer.save(read.clone(), blank(), true);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(is_empty(&open(&read).unwrap().doc().unwrap()));
        assert!(has_text(&open(&sibling(&read, ".bak")).unwrap(), "burst 9"));
    }

    /// CANVAS-CORE-5: a canvas that found no file at its path (a project's canvas not saved yet,
    /// an imported `.excalidraw` file's `.figcraft`) refuses to save over a file made there since:
    /// another window's canvas of the same file, `sawit -n`, a `git pull` of a committed
    /// `.nyawit/`, a headless sawit-mcp. Its first save used to replace that file without a word.
    /// The writer's own first save is still written, and guards the file from then on.
    #[test]
    fn a_file_made_since_the_canvas_found_none_is_not_saved_over() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let writer = Writer::spawn();
        writer.guard(path.clone(), Stamp::of(&path));
        let theirs = bytes_with(&["theirs"]);
        std::fs::write(&path, &theirs).unwrap();

        writer.save(path.clone(), bytes_with(&["mine"]), false);
        writer.flush();
        let error = writer.last_error().expect("the save went through");
        assert!(error.starts_with("canvas wasn't saved: "), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), theirs);
        // Nor is an emptying save, whose backup isn't made either.
        writer.save(path.clone(), blank(), true);
        writer.flush();
        assert!(writer.last_error().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), theirs);
        assert_eq!(names(dir.path()), ["canvas.figcraft"]);
    }

    #[test]
    fn an_unguarded_file_is_saved_whatever_wrote_to_it_since() {
        // As sawit-mcp saves a canvas file that no canvas tab has open: every save writes.
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        std::fs::write(&path, bytes_with(&["theirs"])).unwrap();
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["one"]), false);
        writer.flush();
        std::fs::write(&path, bytes_with(&["theirs", "again"])).unwrap();
        writer.save(path.clone(), bytes_with(&["two"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&path).unwrap(), "two"));

        // A guarded file that is gone has nothing to replace: the save makes it again.
        let gone = dir.path().join("gone.figcraft");
        std::fs::write(&gone, bytes_with(&["read"])).unwrap();
        writer.guard(gone.clone(), Stamp::of(&gone));
        std::fs::remove_file(&gone).unwrap();
        writer.save(gone.clone(), bytes_with(&["saved"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&gone).unwrap(), "saved"));
    }

    /// CANVAS-UI-3: a guarded file that went from where the canvas read or wrote it (moved,
    /// renamed, put in the Trash or deleted elsewhere) is written there again, as FigCraft's rule
    /// has it, but not silently: the writer leaves a note for the canvas to show, once.
    #[test]
    fn a_guarded_file_written_again_where_it_had_gone_leaves_a_note() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        std::fs::write(&path, bytes_with(&["read"])).unwrap();
        let writer = Writer::spawn();
        writer.guard(path.clone(), Stamp::of(&path));
        writer.save(path.clone(), bytes_with(&["one"]), false);
        writer.flush();
        assert_eq!(writer.take_notice(), None);

        std::fs::remove_file(&path).unwrap();
        writer.save(path.clone(), bytes_with(&["two"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&path).unwrap(), "two"));
        let note = writer.take_notice().expect("no note");
        assert!(
            note.starts_with("canvas.figcraft was no longer in ")
                && note.contains("saved it there again"),
            "{note}"
        );
        // Closing the tab alone leaves the file it wrote again: the note says to delete it too.
        assert!(
            note.ends_with("If it was meant to go, close its tab, then delete the file again."),
            "{note}"
        );
        assert_eq!(writer.take_notice(), None);
        // The next save of the file it wrote is a plain one.
        writer.save(path.clone(), bytes_with(&["three"]), false);
        writer.flush();
        assert_eq!(writer.take_notice(), None);

        // A file that wasn't there when the canvas opened, which its first save makes, is no
        // file that went: no note.
        let new = dir.path().join("new.figcraft");
        writer.guard(new.clone(), Stamp::of(&new));
        writer.save(new.clone(), bytes_with(&["new"]), false);
        writer.flush();
        assert_eq!(writer.take_notice(), None);
    }

    /// CANVAS-UI-3: the guard of a renamed or moved file follows it, with the stamp the canvas
    /// held: another app's write to it, at its new place, is still not saved over.
    #[test]
    fn a_guard_follows_its_file_to_its_new_place() {
        let dir = tempdir().unwrap();
        let (from, to) = (dir.path().join("a.figcraft"), dir.path().join("b.figcraft"));
        std::fs::write(&from, bytes_with(&["read"])).unwrap();
        let writer = Writer::spawn();
        writer.guard(from.clone(), Stamp::of(&from));
        std::fs::rename(&from, &to).unwrap();
        writer.follow(&from, to.clone());
        writer.save(to.clone(), bytes_with(&["mine"]), false);
        writer.flush();
        assert_eq!(writer.last_error(), None);
        assert!(has_text(&open(&to).unwrap(), "mine"));
        let theirs = bytes_with(&["theirs", "and more"]);
        std::fs::write(&to, &theirs).unwrap();
        writer.save(to.clone(), bytes_with(&["mine", "again"]), false);
        writer.flush();
        assert!(writer.last_error().is_some());
        assert_eq!(std::fs::read(&to).unwrap(), theirs);
    }

    #[test]
    fn a_writer_whose_thread_is_gone_reports_saves_and_does_not_hang_on_flush() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let writer = Writer {
            tx,
            error: Arc::default(),
            notice: Arc::default(),
            stamps: Arc::default(),
            thread: None,
        };
        writer.save("/nowhere/canvas.figcraft".into(), blank(), false);
        let error = writer.last_error().unwrap();
        assert!(error.contains("/nowhere/canvas.figcraft"), "{error}");
        writer.flush();
    }

    #[test]
    fn dropping_the_writer_finishes_what_is_queued() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        {
            let writer = Writer::spawn();
            writer.save(path.clone(), bytes_with(&["hi"]), false);
        }
        assert!(has_text(&open(&path).unwrap(), "hi"));
    }
}
