//! Files in tabs: a text file's editor or an image's, PDF's or video's viewer, Find in Folder, and
//! the tabs of a file or folder that is renamed or moved following it.

use super::*;

impl Workspace {
    /// Files shown in tabs (editors and viewers), for `sawit --wait`.
    pub fn open_files(&self, cx: &App) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter_map(|t| match &t.item {
                PaneItem::File(e) => Some(e.read(cx).path.clone()),
                PaneItem::Other { file, .. } => file.clone(),
                PaneItem::Server => None,
            })
            .collect()
    }

    /// Opens `path` (from the CLI) with the cursor on a 1-based line and
    /// column, bringing the tabs back if they were hidden.
    pub fn open_file_at(
        &mut self,
        path: PathBuf,
        position: Option<(u32, u32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        self.open_file(path, window, cx);
        if let Some((line, column)) = position
            && let Some(PaneItem::File(editor)) = self.tabs.get(self.active).map(|t| t.item.clone())
        {
            editor.update(cx, |e, cx| e.go_to(line, column, window, cx));
        }
    }

    /// Opens `kept.path` with the unsaved changes the app kept for it when it last quit without
    /// asking ([`crate::editor::kept`]): they're back in its buffer, unsaved, and `from`, the file
    /// that kept them, goes once they're in.
    pub fn restore_kept(
        &mut self,
        kept: crate::editor::kept::Kept,
        from: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file_at(kept.path.clone(), None, window, cx);
        let editor = self.tabs.iter().find_map(|t| match &t.item {
            PaneItem::File(e) if e.read(cx).path == kept.path => Some(e.clone()),
            _ => None,
        });
        if let Some(editor) = editor {
            editor.update(cx, |e, cx| e.restore(kept, from, window, cx));
        }
    }

    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let kind = classify(&path);
        // Canvas files open on a canvas (spec D14), which takes the keyboard, as a file editor
        // does once its file has loaded (CANVAS-UI-2).
        if matches!(kind, FileKind::Canvas | FileKind::Excalidraw) {
            if let Some(board) = self.open_board_file(path, kind, cx) {
                board.update(cx, |board, cx| board.take_keyboard(cx));
            }
            return;
        }
        let viewer = match kind {
            FileKind::Image => Some(Lucide::Image),
            FileKind::Pdf => Some(Lucide::FileText),
            FileKind::Video => Some(Lucide::Film),
            _ => None,
        };
        if let Some(icon) = viewer {
            let key: SharedString = format!("file\u{0}{}", path.display()).into();
            if let Some(ix) = self
                .tabs
                .iter()
                .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
            {
                // As a text file's tab: shown with its project.
                self.activate_tab(ix, cx);
                return;
            }
            let title: SharedString = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into();
            let path_for_icon = path.clone();
            let view: AnyView = match kind {
                FileKind::Image => cx.new(|cx| ImageView::new(path, cx)).into(),
                FileKind::Pdf => cx.new(|cx| PdfView::new(path, cx)).into(),
                _ => cx.new(|cx| VideoView::new(path, window, cx)).into(),
            };
            let group = self.group_of(&path_for_icon, cx);
            self.push_tab(
                PaneItem::Other {
                    view,
                    key,
                    title,
                    icon,
                    file: Some(path_for_icon),
                },
                group,
                cx,
            );
            cx.notify();
            return;
        }
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::File(e) if e.read(cx).path == path))
        {
            self.activate_tab(ix, cx);
        } else {
            let group = self.group_of(&path, cx);
            let editor = cx.new(|cx| FileEditor::open(path, group.clone(), window, cx));
            cx.subscribe(&editor, |_, _, _: &FileEditorEvent, cx| cx.notify())
                .detach();
            self.push_tab(PaneItem::File(editor), group, cx);
        }
        cx.notify();
    }

    /// Find in Folder, one tab per folder.
    pub(super) fn open_folder_search(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_center(cx);
        let key = FolderSearch::key(&dir);
        if self.activate_key(&key) {
            if let Some(PaneItem::Other { view, .. }) = self.tabs.get(self.active).map(|t| &t.item)
                && let Ok(search) = view.clone().downcast::<FolderSearch>()
            {
                let query = search.read(cx).query.clone();
                query.update(cx, |s, cx| s.focus(window, cx));
            }
        } else {
            let name = folder_name(&dir);
            let dir_for_group = dir.clone();
            let view = cx.new(|cx| FolderSearch::new(dir, window, cx));
            cx.subscribe_in(
                &view,
                window,
                |this, _, e: &SearchEvent, window, cx| match e {
                    SearchEvent::Open { path, line } => {
                        this.open_file_at(path.clone(), Some((*line, 1)), window, cx)
                    }
                },
            )
            .detach();
            let group = self.group_of(&dir_for_group, cx);
            self.open_item_in(
                view.into(),
                key.into(),
                format!("Search: {name}").into(),
                Lucide::Search,
                group,
                cx,
            );
        }
        cx.notify();
    }

    /// Open tabs of a renamed or moved file (or of files in a moved folder)
    /// follow it: a file editor's, a canvas file's, whose canvas saves
    /// there from now on ([`BoardView::rebind`], CANVAS-UI-3), an image's, a
    /// PDF's and a video's, and a Find in Folder tab its folder and the files it
    /// found. A project's canvas stays the project's, `.nyawit/canvas.sawit`
    /// by definition. A History tab keeps the path it lists the commits of: git
    /// knows them by that path until the move is committed.
    pub(super) fn follow_move(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        for tab in &mut self.tabs {
            follow_tab(&mut tab.item, from, to, cx);
        }
        cx.notify();
    }
}

/// [`Workspace::follow_move`] for the tab showing `item`.
pub(super) fn follow_tab(item: &mut PaneItem, from: &Path, to: &Path, cx: &mut App) {
    match item {
        PaneItem::File(editor) => editor.update(cx, |e, cx| {
            if let Some(path) = moved_to(&e.path, from, to) {
                e.path = path;
                cx.notify();
            }
        }),
        PaneItem::Other {
            view,
            key,
            title,
            file,
            ..
        } if key.starts_with("file\u{0}") => {
            let board = view.clone().downcast::<BoardView>().ok();
            let shown = match &board {
                Some(board) => board.read(cx).path().to_path_buf(),
                None => file.clone().unwrap_or_default(),
            };
            let Some(path) = moved_to(&shown, from, to) else {
                return;
            };
            if let Some(board) = board {
                board.update(cx, |board, cx| board.rebind(path.clone(), cx));
            } else if let Ok(image) = view.clone().downcast::<ImageView>() {
                image.update(cx, |image, cx| image.follow(path.clone(), cx));
            } else if let Ok(pdf) = view.clone().downcast::<PdfView>() {
                pdf.update(cx, |pdf, cx| {
                    pdf.path = path.clone();
                    cx.notify();
                });
            } else if let Ok(video) = view.clone().downcast::<VideoView>() {
                // Its player has the file open, wherever it goes.
                video.update(cx, |video, cx| {
                    video.path = path.clone();
                    cx.notify();
                });
            }
            *key = format!("file\u{0}{}", path.display()).into();
            *title = (path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into();
            *file = Some(path);
        }
        PaneItem::Other {
            view, key, title, ..
        } if key.starts_with("search\u{0}") => {
            let Ok(search) = view.clone().downcast::<FolderSearch>() else {
                return;
            };
            if let Some(dir) = search.update(cx, |search, cx| search.follow_move(from, to, cx)) {
                *key = FolderSearch::key(&dir).into();
                *title = format!("Search: {}", folder_name(&dir)).into();
            }
        }
        PaneItem::Other { .. } | PaneItem::Server => {}
    }
}

/// The name a folder's tabs show: its own, or its path for `/`.
fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// Where `path` is once `from` has been renamed or moved to `to`: `to` itself, or for a path in a
/// moved folder its place in `to`. `None` when the move left it where it was.
pub(super) fn moved_to(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}
