//! The stored canvas, which `sawit-mcp` opens, changes and saves itself while the app is closed:
//! its calls take turns, a save never goes over what is on disk, and one that takes layers off the
//! canvas keeps a backup first.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use sawit_board::store::{self, Stamp, Writer};
use sawit_board::{BoardDoc, ops};
use serde_json::Value;

use super::dispatch::op_reply;

/// The stored canvas is opened, changed and saved by one call after another: two at once (an
/// agent's parallel tool calls) would each open the file as it was, and the later save would drop
/// the work of the earlier. This keeps one process's calls apart; the lock in Sawit's folder
/// ([`lock_stored_canvas`]) keeps them from another process's (a second agent's `sawit-mcp`).
static STORED_CANVAS: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    // A call that panicked has left the file whole (it is written by rename), so the next may go on.
    STORED_CANVAS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The file in Sawit's own folder that a `sawit-mcp` holds locked while one of its calls opens,
/// changes and saves a stored canvas ([`lock_stored_canvas`]).
pub(super) const CANVAS_LOCK: &str = "mcp-canvas.lock";
/// How long a call waits for another process's call on the stored canvas to end.
const CANVAS_LOCK_WAIT: Duration = Duration::from_secs(60);
/// How many times a call reads the stored canvas and runs its op, when something that doesn't
/// take turns (Sawit as it starts, another program) saved the file while the call ran.
pub(super) const CANVAS_ATTEMPTS: usize = 3;

/// Takes the lock that keeps the canvas calls of every `sawit-mcp` of one Sawit home apart
/// (MCP-1): a call holds it from the moment it stamps the file until its save is on disk, so no
/// other agent's call reads, saves or checks the file in between. The stamp alone couldn't do
/// that: two calls could each check it before either wrote, and the later save then dropped the
/// earlier one's edit though that call had answered ok.
///
/// The lock is `flock`'s, on the file `lock` ([`CANVAS_LOCK`] in Sawit's folder, which every
/// agent's `sawit-mcp` shares): let go when the returned file is dropped, and by the system when
/// the process ends however it ends, so a crash never leaves the canvas locked. A call waits
/// [`CANVAS_LOCK_WAIT`] at most. A link at the lock's name is not followed.
pub(super) fn lock_stored_canvas(lock: &Path) -> Result<std::fs::File, String> {
    let failed = |e: &dyn std::fmt::Display| {
        format!(
            "The canvas wasn't touched: its lock {} couldn't be taken: {e}",
            lock.display()
        )
    };
    if let Some(dir) = lock.parent() {
        std::fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    if std::fs::symlink_metadata(lock).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(failed(&"it is a link"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock)
        .map_err(|e| failed(&e))?;
    let deadline = Instant::now() + CANVAS_LOCK_WAIT;
    let mut pause = Duration::from_millis(1);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "Another agent's canvas call has run for over {} s, so this one stopped \
                     waiting for it and changed nothing. Call it again.",
                    CANVAS_LOCK_WAIT.as_secs()
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(failed(&e)),
        }
    }
}

/// Runs canvas op `op` on the canvas stored in `named` while no app is running, as the app would
/// run it, and saves the canvas if the op changed it. `named` is the project's canvas file, or a
/// canvas file the agent named: a `.sawit` file, or an `.excalidraw` file, whose canvas is saved
/// to the `.sawit` file beside it ([`store::bound_file`]). The reply is the op's, and an op that
/// fails is an `Err` that has changed nothing in the file.
///
/// - The canvas is opened as the app opens it ([`store::open_bound`]): a canvas file that isn't
///   there is a new, empty board, and one that is missing beside an old `canvas.excalidraw` is
///   that board, imported; an `.excalidraw` file is its scene, imported, until the `.sawit`
///   file beside it is there. Nothing is written until an op changes the canvas, so the scene is
///   imported once, and the `.excalidraw` file is never touched. A canvas that can't be opened is
///   an error, and then nothing is written, so a blank board never takes its place.
/// - A change is saved as the app's autosave saves it ([`run_and_save`]): through the store's
///   [`Writer`] (a temporary file renamed into place, a backup kept before a save that takes
///   layers off the canvas, and a `.nyawit/` made with its `.gitignore`), with the file's version
///   history and branches, and never over what the save would lose: images the loader left in
///   the file, or what something else wrote to the file since this call read it.
/// - Calls take turns: this process's by [`take_turn`], every `sawit-mcp`'s by the lock `lock`
///   ([`lock_stored_canvas`]), held from the read to the save. So an answer is true: ok when the
///   edit is on disk, an error when nothing was saved.
pub(super) fn headless(named: &Path, lock: &Path, op: &Value) -> Result<Value, String> {
    let _turn = take_turn();
    headless_turn(named, lock, op)
}

/// One call's turn at the canvas stored in `named`: what [`headless`] does once this process's
/// other calls are out of the way. It holds the lock `lock`, which keeps other processes' calls
/// out, for the whole turn.
pub(super) fn headless_turn(named: &Path, lock: &Path, op: &Value) -> Result<Value, String> {
    let _held = lock_stored_canvas(lock)?;
    // The fonts the app's text has: Sawit's own (Inter), then the system's, which no app is
    // scanning here, so they are read the first time text is laid out.
    sawit_board::fonts::register();
    sawit_board::scan_system_fonts_on_demand();
    edit_stored(named, op, || {})
}

/// Reads the canvas stored in `named` ([`store::open_bound`]), runs `op` on it and saves it to its
/// canvas file ([`run_and_save`]).
///
/// Every `sawit-mcp` takes turns ([`lock_stored_canvas`]), but Sawit as it starts, or another
/// program, may still save the file while a call runs: then the call's save is refused (its edit
/// would drop theirs), and it starts again from what is on disk now, up to [`CANVAS_ATTEMPTS`]
/// times, so the agent's op goes through on top of theirs. When the file changed every time, the
/// call saved nothing, and its error says so in words for an agent. `between` runs after each
/// read, before the op: a test writes the file there.
pub(super) fn edit_stored(
    named: &Path,
    op: &Value,
    mut between: impl FnMut(),
) -> Result<Value, String> {
    let file = store::bound_file(named);
    let mut attempt = 1;
    loop {
        // Stamped before it is read, as the app stamps a canvas file it opens: a save made in
        // between by something else makes this call's save refuse, rather than drop its edit.
        let read = Stamp::of(&file);
        let opened = store::open_bound(named);
        let mut board = opened.map_err(|e| format!("{}: {e}", named.display()))?;
        between();
        match run_and_save(&mut board, &file, read, op) {
            Err(_) if Stamp::of(&file) != read && attempt < CANVAS_ATTEMPTS => attempt += 1,
            Err(_) if Stamp::of(&file) != read => return Err(changed_while_running(&file)),
            done => return done,
        }
    }
}

/// The error of a call on the canvas stored in `file` that saved nothing, because something else
/// saved the file each time the call read it.
fn changed_while_running(file: &Path) -> String {
    format!(
        "Nothing was saved: something else (Sawit as it opened, or another program) saved {} \
         while this call ran, each of the {CANVAS_ATTEMPTS} times it read it. Call it again; \
         canvas_get shows the canvas as it is now.",
        file.display()
    )
}

/// Runs `op` on `board`, read from `file` when the file had stamp `read` ([`Stamp::of`], `None`
/// when there was no file), and saves it there if the op changed the document itself: a new
/// document, as the engine makes one for every edit, is how the app tells too. Selection and view
/// changes are not in it, and neither is a version the engine shows in its place, so none of them
/// saves anything.
///
/// The save is refused, as the app's autosave refuses it, with the reason as the error and the
/// file as it was:
/// - when the file holds images the loader left out, past what it reads from one file, which the
///   save would delete ([`BoardDoc::refuse_dropping_images`]);
/// - when something else wrote to the file since it was read ([`Writer::guard`]): another
///   `sawit-mcp`, say, whose edit the save would drop though its call answered ok.
///
/// A save that takes layers off the canvas keeps the file it replaces as a backup first, the
/// backups of its kind before it moving along ([`Writer::save_with`], [`store::backup_after`]):
/// `<file>.bak` for a `replace` of a canvas with layers, a `clear` (of a page while other pages
/// keep theirs too) and a save that empties the canvas; `<file>.del.bak` for any other op that
/// deletes a layer, on any page ([`store::removes_layers`]): a `delete`, an engine command such as
/// `edit.delete` or `page.delete`. The app can undo them; here every call opens the canvas
/// afresh, so the backups are all that is left of those layers, and one was not enough: an agent
/// that started over twice lost the board it replaced first (CANVAS-CORE-3). Deletions keep
/// theirs apart, so that five small ones never push out the board a clear kept. A refused save
/// keeps no backup either.
pub(super) fn run_and_save(
    board: &mut BoardDoc,
    file: &Path,
    read: Option<Stamp>,
    op: &Value,
) -> Result<Value, String> {
    let before = board.document_itself().map_err(|e| e.to_string())?;
    let reply = op_reply(ops::run(board, op))?;
    let after = board.document_itself().map_err(|e| e.to_string())?;
    if Arc::ptr_eq(&before, &after) {
        return Ok(reply);
    }
    board
        .refuse_dropping_images(file)
        .map_err(|e| e.to_string())?;
    let bytes = board.to_bytes().map_err(|e| e.to_string())?;
    let backup = store::backup_after(op["op"].as_str().unwrap_or_default(), &before, &after);
    let writer = Writer::spawn();
    writer.guard(file.to_path_buf(), read);
    writer.save_with(file.to_path_buf(), bytes, backup);
    writer.flush();
    match writer.last_error() {
        Some(error) => Err(error),
        None => Ok(reply),
    }
}
