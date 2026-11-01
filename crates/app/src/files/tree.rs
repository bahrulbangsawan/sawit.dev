//! The folders the tree shows: read off the UI thread, again every few seconds, opened and closed.

use std::path::{Path, PathBuf};

use gpui_kit::{Context, PathPromptOptions, UniformListScrollHandle, Window};
use sawit_core::files::{Entry, list_dir};

use super::{FilesEvent, FilesPanel, Listing};
use crate::rt;

/// Folders Expand All reads before it stops.
const MAX_EXPAND: usize = 2000;

impl FilesPanel {
    /// Re-reads every open folder, one after the other off the UI thread; the tree only
    /// changes if they did.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let dirs: Vec<PathBuf> = self
            .expanded
            .iter()
            .filter(|d| self.children.contains_key(*d))
            .cloned()
            .collect();
        if !dirs.is_empty() {
            self.load_all(dirs, cx);
        }
    }

    /// Asks for a folder to open as the project, for the workspace to open
    /// ([`FilesEvent::OpenFolder`]).
    pub fn prompt_open(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Project Folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |_, cx| cx.emit(FilesEvent::OpenFolder(dir)));
        })
        .detach();
    }

    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.forget_folder();
        self.expanded.insert(root.clone());
        self.load(root.clone(), cx);
        // The project's Git store looks its repository up ([`Self::set_repo`]).
        cx.emit(FilesEvent::RootChanged(root.clone()));
        self.root = Some(root);
        cx.notify();
    }

    /// Forgets the folder shown so far, for [`Self::set_root`]: its tree and how far it was
    /// scrolled, its repository, the entry selected and the one cut or copied there, its undo and
    /// redo, a name being typed and the last error. No row of the next folder's tree would show
    /// the old selection or Cut, yet ⌫ (Trash), Rename and Paste would act on them.
    pub(super) fn forget_folder(&mut self) {
        self.folder += 1;
        self.expanded.clear();
        self.children.clear();
        self.scroll = UniformListScrollHandle::new();
        self.selected = None;
        self.clip = None;
        self.root_error = None;
        self.repo = None;
        self.undo.clear();
        self.redo.clear();
        self.queue.clear();
        self.edit = None;
        self.error = None;
    }

    fn load(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.load_all(vec![dir], cx);
    }

    /// Reads `dirs`, one after the other off the UI thread.
    fn load_all(&mut self, dirs: Vec<PathBuf>, cx: &mut Context<Self>) {
        let folder = self.folder;
        let rx = (self.disk.list)(dirs, cx);
        cx.spawn(async move |this, cx| {
            let Ok(listings) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                for (dir, listing) in listings {
                    this.listed(folder, dir, listing, cx);
                }
            });
        })
        .detach();
    }

    /// The repository the folder shown is in, as the project's Git store has it: found again
    /// after Initialize Repository, gone once `.git` is.
    pub fn set_repo(&mut self, repo: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.repo != repo {
            self.repo = repo;
            cx.notify();
        }
    }

    /// Shows what reading `dir`, of folder number `folder`, found.
    pub(super) fn listed(
        &mut self,
        folder: u64,
        dir: PathBuf,
        listing: Listing,
        cx: &mut Context<Self>,
    ) {
        if folder != self.folder {
            return;
        }
        match listing {
            Ok(entries) => {
                if self.root.as_ref() == Some(&dir) && self.root_error.take().is_some() {
                    cx.notify();
                }
                if self.children.get(&dir) != Some(&entries) {
                    self.children.insert(dir, entries);
                    cx.notify();
                }
            }
            // A read that failed but for a while (too many files open at once, say) leaves what
            // was read before.
            Err(e) if !gone(&e) && self.children.contains_key(&dir) => {}
            // The folder was deleted or renamed: forget it (or, for the
            // project folder, say so).
            Err(e) => {
                if self.root.as_ref() == Some(&dir) {
                    self.root_error = Some(e.to_string());
                }
                self.children.remove(&dir);
                self.expanded.remove(&dir);
                cx.notify();
            }
        }
    }

    pub(super) fn toggle(&mut self, entry: &Entry, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selected = Some(entry.path.clone());
        if entry.is_dir {
            if !self.expanded.remove(&entry.path) {
                // Read again, though it was read before: it may have changed since it closed.
                self.expanded.insert(entry.path.clone());
                self.load(entry.path.clone(), cx);
            }
        } else {
            cx.emit(FilesEvent::OpenFile(entry.path.clone()));
        }
        cx.notify();
    }

    /// Opens `dir` (and the folders above it, up to the project) in the tree.
    pub(super) fn expand_to(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        for dir in dir.ancestors().take_while(|d| d.starts_with(&root)) {
            if self.expanded.insert(dir.to_path_buf()) && !self.children.contains_key(dir) {
                self.load(dir.to_path_buf(), cx);
            }
        }
    }

    /// Opens every folder under the target folder (or the project).
    pub(super) fn expand_all(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self
            .selection()
            .filter(|p| p.is_dir())
            .or_else(|| self.root.clone())
        else {
            return;
        };
        self.expand_to(&dir, cx);
        let folder = self.folder;
        let rx = rt::spawn_blocking(cx, move || read_tree(dir));
        cx.spawn(async move |this, cx| {
            let Ok(tree) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.expanded_all(folder, tree, cx));
        })
        .detach();
    }

    /// Opens the folders Expand All read in folder number `folder`.
    pub(super) fn expanded_all(
        &mut self,
        folder: u64,
        tree: Vec<(PathBuf, Vec<Entry>)>,
        cx: &mut Context<Self>,
    ) {
        if folder != self.folder {
            return;
        }
        for (dir, entries) in tree {
            self.expanded.insert(dir.clone());
            self.children.insert(dir, entries);
        }
        cx.notify();
    }

    /// Closes every folder but the project folder. An entry selected in one is given up for
    /// that folder at the top, which stays shown.
    pub(super) fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.expanded.retain(|d| *d == root);
        if let Some(selected) = &self.selected
            && let Ok(rel) = selected.strip_prefix(&root)
            && let Some(top) = rel.components().next()
        {
            self.selected = Some(root.join(top));
        }
        cx.notify();
    }
}

/// Whether reading a folder failed because it can't be read any more: it isn't there (deleted,
/// renamed, or a file now), or may not be read. Not when too many files are open at once, say.
fn gone(error: &sawit_core::CoreError) -> bool {
    use std::io::ErrorKind::{NotADirectory, NotFound, PermissionDenied};
    matches!(error, sawit_core::CoreError::Io(e)
        if matches!(e.kind(), NotFound | NotADirectory | PermissionDenied))
}

/// Every folder under `dir` with its entries, breadth first, up to `MAX_EXPAND`.
pub(super) fn read_tree(dir: PathBuf) -> Vec<(PathBuf, Vec<Entry>)> {
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::from([dir]);
    while let Some(dir) = queue.pop_front() {
        if out.len() >= MAX_EXPAND {
            break;
        }
        let Ok(entries) = list_dir(&dir) else {
            continue;
        };
        // A link to a folder opens by hand only: one to a folder above, or to its own, would
        // take Expand All out of the project or round in circles.
        let linked =
            |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
        queue.extend(
            entries
                .iter()
                .filter(|e| e.is_dir && !linked(&e.path))
                .map(|e| e.path.clone()),
        );
        out.push((dir, entries));
    }
    out
}
