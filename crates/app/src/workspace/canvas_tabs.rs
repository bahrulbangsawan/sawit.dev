//! A project's several canvases, each a `.sawit` file in its `.nyawit/` with a tab of its own
//! ([`sawit_core::canvases`]): New Canvas, Rename Canvas (the tab's title becomes a field) and
//! Delete Canvas (to the Trash, after asking) on a canvas tab's menu. The project's
//! `canvas.sawit` is its canvas as before and keeps its name. Which canvases were open is kept,
//! and they open again behind the project's canvas when it opens.

use gpui_kit::component::input::{InputEvent, InputState};
use sawit_core::canvases;

use super::canvas::Reach;
use super::*;

/// What the workspace keeps of its canvas tabs: the one being renamed, the lists last written
/// per project, and the projects whose canvases were opened again this run.
#[derive(Default)]
pub(super) struct CanvasTabs {
    pub rename: Option<CanvasRename>,
    kept: std::collections::HashMap<PathBuf, Vec<String>>,
    restored: HashSet<PathBuf>,
}

/// The canvas tab whose title is being edited.
pub(super) struct CanvasRename {
    pub file: PathBuf,
    pub input: Entity<InputState>,
    _subscription: Subscription,
}

impl Workspace {
    /// The project whose canvas file `file` is (its `.nyawit/`), among the window's projects.
    pub(super) fn canvas_root(&self, file: &Path, cx: &App) -> Option<PathBuf> {
        let dir = file.parent()?;
        self.roots(cx)
            .into_iter()
            .find(|root| sawit_core::nyawit::dir(root) == dir)
    }

    /// The canvas files of `root` open in tabs, in tab order.
    pub(super) fn open_canvases(&self, root: &Path, cx: &App) -> Vec<PathBuf> {
        let dir = sawit_core::nyawit::dir(root);
        (0..self.tabs.len())
            .filter_map(|ix| self.board_at(ix))
            .map(|b| b.read(cx).path().to_path_buf())
            .filter(|p| p.parent() == Some(dir.as_path()))
            .collect()
    }

    /// Opens the canvas `file` of a project: the project's canvas for its `canvas.sawit`, else a
    /// canvas file's tab. Shown for the user; behind the tab on show for an agent.
    pub(super) fn open_canvas_file(
        &mut self,
        file: PathBuf,
        reach: Reach,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BoardView>> {
        self.open_board_file(file, FileKind::Canvas, reach, cx)
    }

    /// New Canvas: a new canvas in the project of canvas tab `ix` (`canvas-2`, …), shown. Its
    /// file is written on its first change.
    pub(super) fn new_canvas(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let open = self.open_canvases(&root, cx);
        let file = canvases::new_canvas_file(&root, &open);
        if let Some(board) = self.open_canvas_file(file, Reach::Show, cx) {
            board.update(cx, |board, cx| board.take_keyboard(cx));
        }
    }

    /// Rename Canvas: the canvas tab's title becomes a field with its name; Return renames the
    /// file (its tab follows), Escape or a click away leaves it.
    pub(super) fn begin_canvas_rename(
        &mut self,
        file: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = canvases::canvas_name(&file);
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(name.clone(), window, cx);
            state.set_selected_range(0..name.len(), cx);
            state
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => {
                    let name = input.read(cx).value().to_string();
                    this.finish_canvas_rename(Some(name), window, cx);
                }
                InputEvent::Blur => this.finish_canvas_rename(None, window, cx),
                _ => {}
            },
        );
        input.update(cx, |s, cx| s.focus(window, cx));
        self.canvas_tabs.rename = Some(CanvasRename {
            file,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn finish_canvas_rename(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = self.canvas_tabs.rename.take() else {
            return;
        };
        cx.notify();
        let Some(name) = name else {
            return;
        };
        if let Err(e) = self.rename_canvas(&rename.file, &name, cx) {
            drop(crate::notice::prompt(
                window,
                PromptLevel::Warning,
                "The canvas wasn't renamed",
                Some(&e),
                &["OK"],
                cx,
            ));
        }
    }

    /// Renames the canvas `file` to `name` (its backups with it), and its tab follows. The
    /// project's own canvas keeps its name.
    pub(super) fn rename_canvas(
        &mut self,
        file: &Path,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, String> {
        let root = self
            .canvas_root(file, cx)
            .ok_or("That canvas isn't one of a project's.")?;
        if file == sawit_core::nyawit::board_file(&root) {
            return Err("The project's canvas keeps its name, canvas.".into());
        }
        let to = canvases::canvas_file(&root, name)?;
        if to == file {
            return Ok(to);
        }
        if std::fs::symlink_metadata(&to).is_ok() || self.open_canvases(&root, cx).contains(&to) {
            return Err(format!(
                "There is a canvas named “{}” already.",
                canvases::canvas_name(&to)
            ));
        }
        if file.exists() {
            std::fs::rename(file, &to).map_err(|e| e.to_string())?;
            let from_name = file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let to_name = to
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            for entry in std::fs::read_dir(file.parent().unwrap_or(Path::new(".")))
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(suffix) = name.strip_prefix(&from_name)
                    && sawit_core::nyawit::is_backup_suffix(suffix)
                {
                    let _ = std::fs::rename(
                        entry.path(),
                        to.with_file_name(format!("{to_name}{suffix}")),
                    );
                }
            }
        }
        self.follow_move(file, &to, cx);
        self.retitle_canvas(&to, cx);
        Ok(to)
    }

    /// The title of `file`'s canvas tab, after its name.
    fn retitle_canvas(&mut self, file: &Path, cx: &mut Context<Self>) {
        let title: SharedString = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
            .into();
        let key: SharedString = format!("file\u{0}{}", file.display()).into();
        for tab in &mut self.tabs {
            if let PaneItem::Other {
                view,
                title: t,
                key: k,
                file: f,
                ..
            } = &mut tab.item
                && view
                    .clone()
                    .downcast::<BoardView>()
                    .is_ok_and(|b| b.read(cx).path() == file)
            {
                *t = title.clone();
                *k = key.clone();
                *f = Some(file.to_path_buf());
            }
        }
        cx.notify();
    }

    /// Delete Canvas: asks first (Cancel is the default), then moves its file (and its backups)
    /// to the Trash and closes its tab. The project's own canvas isn't deleted this way.
    pub(super) fn delete_canvas(
        &mut self,
        file: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = canvases::canvas_name(&file);
        let answer =
            crate::confirm::Confirm::new(format!("Delete the canvas {name}?"), "Delete Canvas")
                .thing(name.to_string())
                .detail("Its file goes to the Trash, where Finder can put it back.")
                .ask(window, cx);
        cx.spawn(async move |this, cx| {
            if !answer.await {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = this.remove_canvas(&file, cx) {
                    eprintln!("sawit: the canvas wasn't deleted: {e}");
                }
            });
        })
        .detach();
    }

    /// Closes the canvas `file`'s tab and moves the file to the Trash (asked already).
    fn remove_canvas(&mut self, file: &Path, cx: &mut Context<Self>) -> Result<(), String> {
        if let Some(root) = self.canvas_root(file, cx)
            && file == sawit_core::nyawit::board_file(&root)
        {
            return Err("The project's canvas isn't deleted.".into());
        }
        if let Some((ix, _)) = self.board_tab(file, cx) {
            self.remove_tabs(&[ix], cx);
        }
        if file.exists() {
            crate::files::trash(file)?;
        }
        Ok(())
    }

    /// Keeps which canvases of each open project are open in tabs, written when that changes
    /// (none open keeps the last list: closing a project closes them all).
    pub(super) fn keep_canvas_tabs(&mut self, cx: &mut Context<Self>) {
        for root in self.roots(cx) {
            let open: Vec<String> = self
                .open_canvases(&root, cx)
                .iter()
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .collect();
            if open.is_empty() || self.canvas_tabs.kept.get(&root) == Some(&open) {
                continue;
            }
            self.canvas_tabs.kept.insert(root.clone(), open.clone());
            drop(crate::rt::spawn_blocking(cx, move || {
                canvases::save_open(&root, &open)
            }));
        }
    }

    /// The project's canvas opened: the other canvases that were open with it open again behind
    /// it, once per run.
    pub(super) fn restore_canvas_tabs(&mut self, root: &Path, cx: &mut Context<Self>) {
        if !self.canvas_tabs.restored.insert(root.to_path_buf()) {
            return;
        }
        let main = sawit_core::nyawit::board_file(root);
        for name in canvases::load_open(root) {
            let Ok(file) = canvases::canvas_file(root, &name) else {
                continue;
            };
            if file != main && file.is_file() {
                self.open_canvas_file(file, Reach::Quietly, cx);
            }
        }
    }
}
