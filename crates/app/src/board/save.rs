//! Saving the canvas: once it has been still for [`SAVE_DELAY`] after an edit, on File ▸ Save,
//! and when it closes or the app quits; and the copy kept of edits a closing canvas couldn't save.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use figcraft_doc::Document;
use gpui_kit::*;
use sawit_board::BoardError;
use sawit_board::store::{self, Backup};

use super::{BoardView, SAVE_DELAY, TITLE};

impl BoardView {
    /// Saves the document now if it isn't the one in the file, and waits until it is on disk, as
    /// a closing canvas does ([`Self::flush_on_close`]), without keeping a copy of what it can't
    /// save: the tests' way to have what is queued written. When that fails, or the save is
    /// refused, the error is logged, and the next flush or edit tries again; the writer's
    /// [`Writer::last_error`] still says why a write failed.
    #[cfg(test)]
    pub fn flush(&mut self) {
        match self.save_now() {
            Ok(Some(notice)) => log::info!("{notice}"),
            Ok(None) => {}
            Err(err) => log::warn!("the canvas couldn't be saved: {err}"),
        }
    }

    /// The canvas is closing, its tab, its window or the app: saves the document now if it isn't
    /// the one in the file, and waits until it is on disk ([`Self::save_now`]). When that fails,
    /// or the save is refused (it would delete images the file holds, or replace what another app
    /// or window wrote to it since), the error is logged, and the document is kept in a copy of
    /// its own ([`Self::keep_unsaved`]), whose path is logged too:
    /// closing used to drop every edit since the last save that went through, with only a line
    /// in the log, though the refusal's own toast says to close the tab (CANVAS-CORE-4).
    pub(super) fn flush_on_close(&mut self) {
        let err = match self.save_now() {
            Ok(Some(notice)) => return log::info!("{notice}"),
            Ok(None) => return,
            Err(err) => err,
        };
        log::warn!("the canvas couldn't be saved: {err}");
        match self.keep_unsaved(&app_root()) {
            Ok(Some(copy)) => log::warn!(
                "its edits since the last save are kept in {}",
                copy.display()
            ),
            Ok(None) => {}
            Err(e) => log::error!("its edits since the last save couldn't be kept: {e}"),
        }
    }

    /// Keeps the document, which the canvas couldn't save, in a file of its own, as a save would
    /// write it: `<name> (unsaved <date>).sawit` beside the canvas file, numbered when that is
    /// taken, made new and never through a link ([`sawit_core::nyawit::write_unique`]); else, when
    /// nothing can be made there (the folder is read-only, or gone), in `unsaved/` in Sawit's own
    /// folder, `app_root`. The file that was there stays as it is. A document kept already isn't
    /// kept again. The copy it made, if it made one.
    pub(super) fn keep_unsaved(&mut self, app_root: &Path) -> Result<Option<PathBuf>, String> {
        let Ok(board) = &self.board else {
            return Ok(None);
        };
        let doc = board.document_itself().map_err(|e| e.to_string())?;
        if same(&self.kept_unsaved, &doc) {
            return Ok(None);
        }
        let bytes = board.to_bytes().map_err(|e| e.to_string())?;
        let stem = (self.path.file_stem()).map_or_else(|| TITLE.into(), |s| s.to_string_lossy());
        let name = format!(
            "{stem} (unsaved {}).{}",
            sawit_core::date::local_date(),
            sawit_core::files::CANVAS_EXTENSION
        );
        let beside = (self.path.parent())
            .map(|folder| sawit_core::nyawit::write_unique(folder, &name, &bytes));
        let copy = match beside {
            Some(Ok(copy)) => copy,
            _ => {
                let folder = app_root.join("unsaved");
                std::fs::create_dir_all(&folder)
                    .and_then(|()| sawit_core::nyawit::write_unique(&folder, &name, &bytes))
                    .map_err(|e| format!("{}: {e}", folder.join(&name).display()))?
            }
        };
        self.kept_unsaved = Some(doc);
        Ok(Some(copy))
    }

    /// Saves the document now if it isn't the one in the file, and waits until it is on disk,
    /// with why it isn't as the error, a sentence for a toast: what File ▸ Save (⌘S) does
    /// ([`commands`]), and closing the canvas ([`Self::flush_on_close`]). On success, the note of a
    /// save that wrote the file again where it had gone, if one did.
    pub(super) fn save_now(&mut self) -> Result<Option<String>, String> {
        let queued = self.queue_save();
        self.writer.flush();
        let written = self.writer.last_error();
        let notice = self.writer.take_notice();
        if written.is_some() {
            self.saved = None;
        }
        queued.map_err(|err| err.to_string())?;
        written.map_or(Ok(notice), Err)
    }

    /// Hands the document to the writer if it isn't the one saved last. The document handed
    /// over, if any. It is the document itself: a version the engine shows in its place is no
    /// edit, and is never saved.
    ///
    /// A save over a file that holds images the loader left out (past what it reads from one
    /// file) would delete them, so it is refused, as FigCraft's own save refuses it: the refusal
    /// is the error, and the file stays as it is.
    ///
    /// A save of a canvas without layers, or the first after an agent's op took layers off it
    /// ([`Self::run_op`]), keeps the board the file holds as a backup first
    /// ([`Writer::save_with`]): the board's (`.bak`) for a save that empties the canvas, else the
    /// one the op asked for.
    pub(super) fn queue_save(&mut self) -> Result<Option<Arc<Document>>, BoardError> {
        let Ok(board) = &self.board else {
            return Ok(None);
        };
        let doc = board.document_itself()?;
        if same(&self.saved, &doc) {
            return Ok(None);
        }
        board.refuse_dropping_images(&self.path)?;
        let bytes = board.to_bytes()?;
        let empties = if store::is_empty(&doc) {
            Backup::Board
        } else {
            Backup::None
        };
        let backup = std::mem::take(&mut self.next_backup).max(empties);
        self.writer.save_with(self.path.clone(), bytes, backup);
        self.saved = Some(doc.clone());
        self.pending = Some(doc.clone());
        Ok(Some(doc))
    }

    /// After a frame: when the document isn't the one saved, saves it once the canvas has been
    /// still for [`SAVE_DELAY`]. Every new document (each edit makes one) starts the wait over,
    /// so a drag is saved once it ends. A failed or refused save shows its error as a toast: the
    /// queue's refusal (images the file holds) or the writer's (the file changed on disk). So
    /// does a save that wrote the file again where it had gone (moved, renamed, put in the Trash
    /// or deleted without the canvas following it), which used to be silent (CANVAS-UI-3).
    pub(super) fn autosave(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.board().and_then(|board| board.document_itself().ok()) else {
            return;
        };
        if same(&self.saved, &doc) || same(&self.pending, &doc) {
            return;
        }
        self.pending = Some(doc);
        self.save_later = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let queued = this.update(cx, |this, cx| match this.queue_save() {
                Ok(queued) => queued.map(|doc| (doc, this.writer.clone())),
                Err(err) => {
                    this.show_error(&err, cx);
                    None
                }
            });
            let Ok(Some((doc, writer))) = queued else {
                return;
            };
            // The write is waited for off the UI thread.
            let (error, notice) = cx
                .background_spawn(async move {
                    writer.flush();
                    (writer.last_error(), writer.take_notice())
                })
                .await;
            if let Some(error) = error {
                let _ = this.update(cx, |this, cx| {
                    // Not on disk: the next flush (closing the tab, quitting) tries again.
                    if same(&this.saved, &doc) {
                        this.saved = None;
                    }
                    this.show_toast(error, cx);
                });
            } else if let Some(notice) = notice {
                let _ = this.update(cx, |this, cx| this.show_toast(notice, cx));
            }
        }));
    }
}

impl Drop for BoardView {
    fn drop(&mut self) {
        // The tab is closing: what isn't on disk yet is written now, or kept in a copy.
        self.flush_on_close();
    }
}

/// Sawit's own folder, where the edits a closing canvas couldn't save go when nothing can be made
/// beside its file ([`BoardView::keep_unsaved`]).
#[cfg(not(test))]
fn app_root() -> PathBuf {
    sawit_core::instances::AppPaths::default_macos().root
}

/// A folder of the test's own in place of Sawit's, one for each test (each runs on a thread of
/// its own), removed with its others once it has ended ([`crate::testing::scratch_dir`]).
#[cfg(test)]
fn app_root() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static TESTS: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static ROOT: PathBuf = crate::testing::scratch_dir(&format!(
            "board-view-app-root-{}",
            TESTS.fetch_add(1, Ordering::Relaxed)
        ));
    }
    ROOT.with(PathBuf::clone)
}

/// Whether `kept` is `doc`: the same document, not only an equal one.
fn same(kept: &Option<Arc<Document>>, doc: &Arc<Document>) -> bool {
    kept.as_ref().is_some_and(|kept| Arc::ptr_eq(kept, doc))
}
