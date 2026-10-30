//! Where the canvas file lives, how it is opened, and the writer thread that saves it.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use figcraft_doc::Document;
use sawit_core::nyawit;

use crate::doc::{BoardDoc, BoardError};

/// What a new canvas is called.
const NAME: &str = "Canvas";

/// The canvas file: `<project>/.nyawit/canvas.figcraft`, else `canvas.figcraft` in `app_root`
/// (Sawit's own folder).
pub fn board_file(project: Option<&Path>, app_root: &Path) -> PathBuf {
    match project {
        Some(project) => nyawit::board_file(project),
        None => app_root.join(nyawit::BOARD_FILE),
    }
}

/// Opens the canvas at `path`, as [`open_with_issues`] does, and only logs its notes.
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
/// - A corrupt or truncated file is `Err(BoardError::Format)`, and stays as it is.
/// - With notes, the original is first copied to `<path>.lossy.bak`, so the first save can't
///   silently drop what the loader left out. When that copy can't be made the file is not opened
///   either: it stays as it is rather than get saved over unprotected. The copy has a name of its
///   own: the `<path>.bak` that an emptying save keeps ([`Writer::save`]) would replace it.
pub fn open_with_issues(path: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok((BoardDoc::new_board(NAME)?, Vec::new()));
        }
        Err(e) => {
            return Err(BoardError::Io(format!(
                "couldn't read {}: {e}",
                path.display()
            )));
        }
    };
    let (board, issues) = BoardDoc::from_bytes_with_issues(&bytes)?;
    if !issues.is_empty() {
        let bak = lossy_backup(path);
        std::fs::write(&bak, &bytes).map_err(|e| {
            BoardError::Io(format!(
                "couldn't keep a copy of {} at {} before opening it: {e}",
                path.display(),
                bak.display()
            ))
        })?;
    }
    Ok((board, issues))
}

/// Whether `doc` has no layers on any page: nothing drawn, so nothing to lose.
pub fn is_empty(doc: &Document) -> bool {
    doc.pages
        .iter()
        .all(|page| doc.node(*page).is_none_or(|p| p.children.is_empty()))
}

/// Saves the canvas on its own thread, so editing never waits for the disk.
///
/// Saves are queued, and when several for one path are waiting only the newest is written. Each
/// goes to `<path>.tmp` first and is then renamed over the file, so a crash half way can't leave a
/// truncated canvas. Dropping the writer writes what is queued, then ends the thread.
pub struct Writer {
    tx: Sender<Msg>,
    error: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<()>>,
}

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
    /// `bytes` is a canvas without layers.
    empty: bool,
}

impl Writer {
    /// Starts the writer thread, called "canvas-save".
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let started = std::thread::Builder::new()
            .name("canvas-save".into())
            .spawn({
                let error = error.clone();
                move || run(&rx, &error)
            });
        let thread = match started {
            Ok(thread) => Some(thread),
            Err(e) => {
                // The queue went with the thread, so every save will report that too.
                *lock(&error) = Some(format!("The canvas couldn't start saving: {e}"));
                None
            }
        };
        Self { tx, error, thread }
    }

    /// Queues `bytes` to be written to `path`, and returns at once.
    ///
    /// `empty` says the bytes are a canvas without layers ([`is_empty`]). Then a file already
    /// there that has layers, or that doesn't load, is first copied to `<path>.bak`, so clearing
    /// the canvas can be undone after a restart; if that copy can't be made, the save is not done
    /// and the error is reported. A `.nyawit` folder that is missing is made with its `.gitignore`, any other
    /// missing folder without.
    pub fn save(&self, path: PathBuf, bytes: Vec<u8>, empty: bool) {
        let save = Msg::Save(Save { path, bytes, empty });
        if let Err(mpsc::SendError(Msg::Save(lost))) = self.tx.send(save) {
            *lock(&self.error) = Some(format!(
                "The canvas couldn't be saved to {}: its writer has stopped",
                lost.path.display()
            ));
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
    /// The newest save of each path.
    saves: Vec<Save>,
    /// Who waits for these saves to be on disk.
    flushes: Vec<Sender<()>>,
    stop: bool,
}

impl Batch {
    fn add(&mut self, msg: Msg) {
        match msg {
            Msg::Save(save) => {
                self.saves.retain(|older| older.path != save.path);
                self.saves.push(save);
            }
            Msg::Flush(ack) => self.flushes.push(ack),
            Msg::Stop => self.stop = true,
        }
    }
}

/// The writer thread: take everything queued, write the newest save of each path, then answer
/// whoever waits.
fn run(rx: &Receiver<Msg>, error: &Mutex<Option<String>>) {
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
                if let Err(e) = write(save) {
                    failure = Some(e);
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
fn write(save: &Save) -> Result<(), String> {
    let path = &save.path;
    let failed = |why: String| format!("The canvas couldn't be saved to {}: {why}", path.display());
    make_folder(path).map_err(|e| failed(e.to_string()))?;
    if save.empty {
        keep_backup_before_emptying(path).map_err(failed)?;
    }
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, &save.bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            failed(e.to_string())
        })
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

/// Before a canvas without layers replaces the file at `path`: when that file loads as a canvas
/// with layers, keeps it as `<path>.bak`, so "clear canvas" (or an agent's clear) can be undone
/// even after the app restarts. So is a file that doesn't load at all (cut short, or written by a
/// newer FigCraft), which may hold layers too, as the old canvas kept a scene it couldn't read; an
/// empty file has nothing to keep. On failure the error says why, and the save must not go on.
fn keep_backup_before_emptying(path: &Path) -> Result<(), String> {
    let old = match std::fs::read(path) {
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
    let bak = sibling(path, ".bak");
    std::fs::write(&bak, old).map_err(|e| {
        format!(
            "the canvas already there couldn't be copied to {}: {e}",
            bak.display()
        )
    })
}

/// Where [`open_with_issues`] keeps the original of a file it opens with notes:
/// `<path>.lossy.bak`. That is apart from the copy an emptying save keeps (`<path>.bak`), so that
/// clearing the canvas after edits can't put the edited canvas over the original.
fn lossy_backup(path: &Path) -> PathBuf {
    sibling(path, ".lossy.bak")
}

/// `path` with `suffix` added to its name: `canvas.figcraft` becomes `canvas.figcraft.bak`.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// The writer's last error. A poisoned lock still holds a usable value.
fn lock(error: &Mutex<Option<String>>) -> MutexGuard<'_, Option<String>> {
    error.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use figcraft_doc::{Document, NodeKind};
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::doc::DocKind;

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

    /// A `.figcraft` file that loads, with one note that saving it would drop its version history.
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
        // No `.bak` or `.tmp` beside it either.
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

    #[test]
    fn open_with_issues_backs_up_the_original() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let bak = lossy_backup(&path);
        assert_eq!(bak, dir.path().join("canvas.figcraft.lossy.bak"));
        let original = file_with_a_history();
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
        let original = file_with_a_history();
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
                empty: false,
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
        // Opened with notes, edited and saved (the save drops the history), then cleared, as an
        // agent's add then clear does: the emptying save keeps the edited canvas, and the
        // original stays where the open put it.
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let original = file_with_a_history();
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
        let dir = tempdir().unwrap();
        let path = dir.path().join("canvas.figcraft");
        let writer = Writer::spawn();
        writer.save(path.clone(), bytes_with(&["old"]), false);
        writer.flush();
        // A folder where the temporary file should be: writing it fails, and the canvas is
        // untouched. Written straight to the file, the save would have gone through.
        std::fs::create_dir(sibling(&path, ".tmp")).unwrap();
        writer.save(path.clone(), bytes_with(&["new"]), false);
        writer.flush();
        assert!(writer.last_error().is_some());
        let board = open(&path).unwrap();
        assert!(has_text(&board, "old"));
        assert!(!has_text(&board, "new"));
    }

    #[test]
    fn a_writer_whose_thread_is_gone_reports_saves_and_does_not_hang_on_flush() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let writer = Writer {
            tx,
            error: Arc::default(),
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
