//! What the tree's commands act on, and the commands: Cut, Copy, Paste, Duplicate, Trash, Delete…

use std::path::{Path, PathBuf};

use gpui_kit::{ClipboardItem, Context, PromptLevel, Window};
use sawit_core::files;

use super::{After, Clip, FilesPanel, Op, Request, ask_to_trash};
use crate::rt;

/// File operations from the context menu and their shortcuts.
impl FilesPanel {
    /// The selected entry, if its row is shown. The commands that act on an entry (Trash,
    /// Delete, Rename, Duplicate, Cut, Copy) never act on anything else: not on an entry in a
    /// folder that isn't open or that went, nor on the project folder or anything outside it.
    pub(super) fn selection(&self) -> Option<PathBuf> {
        self.selected.clone().filter(|path| self.shown(path))
    }

    /// Whether `path` has a row in the tree: it is listed, and its folder and the folders
    /// above it, up to the project folder, are open.
    pub(super) fn shown(&self, path: &Path) -> bool {
        let (Some(root), Some(dir)) = (&self.root, path.parent()) else {
            return false;
        };
        dir.starts_with(root)
            && self.root_error.is_none()
            && (self.children.get(dir))
                .is_some_and(|entries| entries.iter().any(|e| e.path == path))
            && dir
                .ancestors()
                .take_while(|d| d.starts_with(root))
                .all(|d| self.expanded.contains(d))
    }

    /// The selected entry, else the project folder.
    pub(super) fn target(&self) -> Option<PathBuf> {
        self.selection().or_else(|| self.root.clone())
    }

    /// What Trash moves: the selected entry, or nothing when none is selected. Never the project
    /// folder, which [`Self::target`] falls back to for the commands that show, open or copy a
    /// path: ⌫ in a tree with no selection would move the whole project to the Trash.
    pub(super) fn trash_target(&self) -> Option<PathBuf> {
        self.selection()
    }

    /// The target if it is a folder, else the folder it is in.
    pub(super) fn target_dir(&self) -> Option<PathBuf> {
        let path = self.target()?;
        if path.is_dir() {
            Some(path)
        } else {
            path.parent().map(Path::to_path_buf)
        }
    }

    pub(super) fn pick(&mut self, cut: bool, cx: &mut Context<Self>) {
        if let Some(path) = self.selection() {
            self.clip = Some(Clip { path, cut });
            cx.notify();
        }
    }

    /// Pastes what was cut (moving it) or copied. The Cut stays until its move lands, so that
    /// one that fails ("… already exists here") can be pasted elsewhere.
    pub(super) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(clip), Some(dir)) = (self.clip.clone(), self.target_dir()) else {
            return;
        };
        let Some(name) = clip
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            return;
        };
        if clip.cut {
            let to = dir.join(&name);
            if to != clip.path {
                let after = After {
                    pastes_cut: true,
                    ..After::default()
                };
                let op = Op::Move {
                    from: clip.path,
                    to,
                };
                self.request(Request::Do(op, after), window, cx);
            }
        } else {
            let to = files::free_name(&dir, &name, clip.path.is_dir());
            let op = Op::Copy {
                from: clip.path,
                to,
            };
            self.request(Request::Do(op, After::default()), window, cx);
        }
    }

    pub(super) fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.selection() else {
            return;
        };
        let (Some(dir), Some(name)) = (from.parent(), from.file_name()) else {
            return;
        };
        let to = files::free_name(dir, &name.to_string_lossy(), from.is_dir());
        let op = Op::Copy { from, to };
        self.request(Request::Do(op, After::default()), window, cx);
    }

    pub(super) fn copy_path(&mut self, relative: bool, cx: &mut Context<Self>) {
        let Some(path) = self.target() else {
            return;
        };
        let text = match (&self.root, relative) {
            (Some(root), true) => match path.strip_prefix(root) {
                Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
                Ok(rel) => rel.to_string_lossy().into_owned(),
                Err(_) => path.to_string_lossy().into_owned(),
            },
            _ => path.to_string_lossy().into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Adds the target to the repository's `.gitignore`, or to its private
    /// `.git/info/exclude` ([`sawit_core::git::ignore`]).
    pub(super) fn ignore(&mut self, exclude: bool, cx: &mut Context<Self>) {
        let (Some(repo), Some(path)) = (self.repo.clone(), self.target()) else {
            return;
        };
        let rx = rt::spawn(cx, async move {
            sawit_core::git::ignore(&repo, &path, exclude)
                .await
                .map(drop)
                .map_err(|e| e.to_string())
        });
        let folder = self.folder;
        cx.spawn(async move |this, cx| {
            if let Ok(Err(e)) = rx.await {
                let _ = this.update(cx, |this, cx| this.failed(folder, e, cx));
            }
        })
        .detach();
    }

    /// Selects nothing if the entry selected was `gone` or in it.
    pub(super) fn forget_selection(&mut self, gone: &Path) {
        if self.selected.as_ref().is_some_and(|s| s.starts_with(gone)) {
            self.selected = None;
        }
    }

    /// Shows why something done in folder number `folder` failed.
    pub(super) fn failed(&mut self, folder: u64, error: String, cx: &mut Context<Self>) {
        if folder == self.folder {
            self.error = Some(error);
            cx.notify();
        }
    }

    /// Moves the selected entry to the Trash, with `ask`, once the user says so.
    pub(super) fn trash(&mut self, ask: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.trash_target() else {
            return;
        };
        if !ask {
            self.request(Request::Do(Op::Trash(path), After::default()), window, cx);
            return;
        }
        let detail = "You can undo this, or put it back from the Trash.";
        let answer = ask_to_trash(&path, detail, window, cx);
        let folder = self.folder;
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if this.folder == folder {
                    this.request(Request::Do(Op::Trash(path), After::default()), window, cx);
                }
            });
        })
        .detach();
    }

    /// Deletes the selected entry for good, after asking.
    pub(super) fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selection() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete “{name}” permanently?"),
            Some("It won't go to the Trash, and this can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        let folder = self.folder;
        cx.spawn(async move |this, cx| {
            if !matches!(answer.await, Ok(0)) {
                return;
            }
            let gone = path.clone();
            let Ok(rx) = this.update(cx, |_, cx| {
                rt::spawn_blocking(cx, move || files::remove(&gone).map_err(|e| e.to_string()))
            }) else {
                return;
            };
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok(Err(e)) => this.failed(folder, e, cx),
                    Ok(Ok(())) if folder == this.folder => this.forget_selection(&path),
                    _ => {}
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
