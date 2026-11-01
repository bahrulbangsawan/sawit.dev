//! File changes, run off the UI thread one at a time in the order asked, with undo and redo.

use std::path::Path;

use gpui_kit::{Context, Window};
use sawit_core::files;

use super::{After, FilesEvent, FilesPanel, Op, Outcome, Request, Step};

impl FilesPanel {
    /// Asks for a file change. It runs off the UI thread once the changes asked for before it
    /// have landed, so that each undo or redo applies to the history as the changes before it
    /// left it.
    pub(super) fn request(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.queue.push_back(request);
        self.next(window, cx);
    }

    /// Starts the next change asked for, unless one is running.
    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while !self.busy {
            let Some(request) = self.queue.pop_front() else {
                return;
            };
            let (op, step, after) = match request {
                // A copy takes the first free name when it starts: one before it may have
                // taken the name it was given.
                Request::Do(Op::Copy { from, to }, after) => {
                    let name = from.file_name().unwrap_or_default().to_string_lossy();
                    let dir = to.parent().unwrap_or(Path::new("/"));
                    let to = files::free_name(dir, &name, from.is_dir());
                    (Op::Copy { from, to }, Step::Do, after)
                }
                Request::Do(op, after) => (op, Step::Do, after),
                Request::Undo => match self.undo.pop() {
                    Some(op) => (op, Step::Undo, After::default()),
                    None => continue,
                },
                Request::Redo => match self.redo.pop() {
                    Some(op) => (op, Step::Redo, After::default()),
                    None => continue,
                },
            };
            self.start(op, step, after, window, cx);
        }
    }

    /// Runs `op` off the UI thread; [`Self::landed`] takes it from there.
    fn start(
        &mut self,
        op: Op,
        step: Step,
        after: After,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        cx.notify();
        let folder = self.folder;
        let rx = (self.disk.run)(op.clone(), cx);
        cx.spawn_in(window, async move |this, cx| {
            let outcome = rx
                .await
                .unwrap_or_else(|_| Err("The change stopped before it finished.".into()));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.landed(folder, op, step, after, outcome, window, cx);
                this.next(window, cx);
            });
        })
        .detach();
    }

    /// Takes `op`, run as `step` in folder number `folder`, once it has landed: keeps its undo
    /// (or redo), and shows it.
    #[allow(clippy::too_many_arguments)]
    fn landed(
        &mut self,
        folder: u64,
        op: Op,
        step: Step,
        after: After,
        outcome: Outcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if folder != self.folder {
            // Another folder is shown now. A move that happened is still told, for the tabs
            // of what moved to follow it; the rest was that folder's.
            if let (Ok(_), Some((from, to))) = (&outcome, op.moved()) {
                cx.emit(FilesEvent::Moved {
                    from: from.to_path_buf(),
                    to: to.to_path_buf(),
                });
            }
            return;
        }
        match outcome {
            Ok(inverse) => {
                match step {
                    Step::Do => {
                        self.undo.push(inverse);
                        self.redo.clear();
                    }
                    Step::Undo => self.redo.push(inverse),
                    Step::Redo => self.undo.push(inverse),
                }
                self.error = None;
                if let Some((from, to)) = op.moved() {
                    cx.emit(FilesEvent::Moved {
                        from: from.to_path_buf(),
                        to: to.to_path_buf(),
                    });
                }
                if let Op::Trash(gone) = &op {
                    self.forget_selection(gone);
                }
                if let (true, Op::Move { from, .. }) = (after.pastes_cut, &op)
                    && self.clip.as_ref().is_some_and(|c| c.cut && c.path == *from)
                {
                    self.clip = None;
                }
                if let Some(path) = op.result().map(Path::to_path_buf) {
                    if let Some(parent) = path.parent() {
                        self.expand_to(parent, cx);
                    }
                    if matches!((&op, step), (Op::Create { dir: false, .. }, Step::Do)) {
                        cx.emit(FilesEvent::OpenFile(path.clone()));
                    }
                    self.selected = Some(path);
                }
            }
            Err(e) => {
                if let Some(named) = after.named {
                    self.retype(named, window, cx);
                }
                let skipped = match step {
                    Step::Do => self.skip_steps_behind(),
                    Step::Undo | Step::Redo => None,
                };
                self.error = Some(match skipped {
                    Some(note) => format!("{e}\n{note}"),
                    None => e,
                });
                // An undo or redo that failed stays, to try again once what was in its way is
                // dealt with ("… already exists here"). One whose file is gone never can.
                let still = std::fs::symlink_metadata(op.source()).is_ok();
                match step {
                    Step::Undo if still => self.undo.push(op),
                    Step::Redo if still => self.redo.push(op),
                    _ => {}
                }
            }
        }
        self.refresh(cx);
        cx.notify();
    }

    /// Drops the Undo and Redo asked for behind a change that failed, up to the next change
    /// asked for: they were meant for it, and run now, they would undo or redo the change
    /// before it instead (review-files M2). What was dropped, said for the error.
    fn skip_steps_behind(&mut self) -> Option<&'static str> {
        let (mut undo, mut redo) = (false, false);
        while let Some(request) = self.queue.front() {
            match request {
                Request::Undo => undo = true,
                Request::Redo => redo = true,
                Request::Do(..) => break,
            }
            self.queue.pop_front();
        }
        match (undo, redo) {
            (false, false) => None,
            (true, false) => {
                Some("The Undo asked for after it was skipped: nothing else was undone.")
            }
            (false, true) => {
                Some("The Redo asked for after it was skipped: nothing else was redone.")
            }
            (true, true) => Some(
                "The Undo and Redo asked for after it were skipped: nothing else was undone or redone.",
            ),
        }
    }
}
