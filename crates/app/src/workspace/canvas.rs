//! Canvas tabs: each project's canvas and any canvas file's, one tab per file, and telling each
//! canvas whether it is on screen.
//!
//! The user reaches a canvas by showing it ([`Reach::Show`]); an agent's canvas op reaches it
//! where it is ([`Reach::Quietly`]): the tab, the project and the layout the user has stay as they
//! are, and a canvas not open yet opens in a tab behind the one on show. Its tab says agents are at
//! work on it with a dot ([`crate::board::AgentActivity`]).

use super::*;

/// How a canvas tab is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reach {
    /// As the user goes to it: shown, with its project, the tab area on screen.
    Show,
    /// For an agent's op: nothing on screen changes.
    Quietly,
}

impl Workspace {
    /// Opens the active project's brainstorming canvas (one tab per
    /// project, saved in its `.nyawit/`), or the app-wide one without a
    /// project. Its file is `.nyawit/canvas.sawit` (else `canvas.sawit`
    /// in Sawit's own folder).
    fn open_canvas(&mut self, cx: &mut Context<Self>) -> Option<Entity<BoardView>> {
        let root = self.root(cx);
        self.open_canvas_of(root, cx)
    }

    /// Opens the active project's canvas as the user's ⌃⌘C and the title bar's Canvas button do:
    /// with the keyboard ([`BoardView::take_keyboard`], CANVAS-UI-2). An agent's canvas op and
    /// `app_open_page canvas` open the canvas of the agent's own project without
    /// ([`Self::open_canvas_of`]), leaving the keyboard where the user has it.
    pub(super) fn show_canvas(&mut self, cx: &mut Context<Self>) {
        if let Some(board) = self.open_canvas(cx) {
            board.update(cx, |board, cx| board.take_keyboard(cx));
        }
    }

    /// Opens the canvas of the project at `root`, or the app-wide one for `None`, as
    /// [`Self::open_canvas`] opens the active project's. A canvas whose file is open in a canvas
    /// file's tab already (opened before its project was in this window) is that tab: two
    /// canvases of one file would each save over the other's edits.
    pub(super) fn open_canvas_of(
        &mut self,
        root: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BoardView>> {
        self.canvas_of(root, Reach::Show, cx)
    }

    /// The canvas of the project at `root` (the app-wide one for `None`), reached as `reach`
    /// says: [`Self::open_canvas_of`] for the user, quietly for an agent's op.
    pub(super) fn canvas_of(
        &mut self,
        root: Option<PathBuf>,
        reach: Reach,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BoardView>> {
        if reach == Reach::Show {
            self.leave_welcome(CENTER_ONLY, cx);
            if !self.layout.center {
                self.layout.toggle(Area::Center);
            }
        }
        let key: SharedString = match &root {
            Some(root) => format!("canvas\u{0}{}", root.display()).into(),
            None => "canvas".into(),
        };
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            // With its project: another project's canvas file opened by its path, say.
            if reach == Reach::Show {
                self.activate_tab(ix, cx);
            }
            // The canvas is the tab's own view.
            let PaneItem::Other { view, .. } = &self.tabs[ix].item else {
                return None;
            };
            return view.clone().downcast::<BoardView>().ok();
        }
        let file = board_file(root.as_deref(), &AppPaths::default_macos().root);
        if let Some((ix, board)) = self.board_tab(&file, cx) {
            if reach == Reach::Show {
                self.activate_tab(ix, cx);
            }
            return Some(board);
        }
        let view = cx.new(|cx| BoardView::open(file.clone(), cx));
        self.watch_board(&view, cx);
        self.boards.retain(|b| b.upgrade().is_some());
        self.boards.push(view.downgrade());
        self.add_tab(
            reach,
            PaneItem::Other {
                view: view.clone().into(),
                key,
                title: crate::board::TITLE.into(),
                icon: crate::board::ICON,
                // The file it shows, for `sawit --wait` on it (CANVAS-UI-7) and app_state.
                file: Some(file),
            },
            root,
            cx,
        );
        cx.notify();
        Some(view)
    }

    /// Opens a canvas file in a canvas tab (spec D14): a `.sawit` file bound to itself, or an
    /// `.excalidraw` file imported and bound to the `<stem>.sawit` beside it
    /// ([`BoardView::import`]).
    ///
    /// A file has one canvas in the window, whichever of its two names opens it, because two
    /// canvases of one file would each save over the other's edits. A project's canvas file is
    /// that project's canvas tab ([`Self::open_canvas_of`]); any other file's tab is keyed by the
    /// canvas file it is bound to ([`store::bound_file`]), and shown again when the file is opened
    /// again. The canvas shown.
    pub(super) fn open_board_file(
        &mut self,
        path: PathBuf,
        kind: FileKind,
        reach: Reach,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BoardView>> {
        let file = store::bound_file(&path);
        let roots: Vec<Option<PathBuf>> = self.projects.iter().map(|p| p.root(cx)).collect();
        if let Some(root) = canvas_project(&file, roots, &AppPaths::default_macos().root) {
            return self.canvas_of(root, reach, cx);
        }
        if let Some((ix, board)) = self.board_tab(&file, cx) {
            if reach == Reach::Show {
                self.activate_tab(ix, cx);
            }
            return Some(board);
        }
        let view = cx.new(|cx| match kind {
            FileKind::Excalidraw => BoardView::import(path, cx),
            _ => BoardView::open(file.clone(), cx),
        });
        self.watch_board(&view, cx);
        let title: SharedString = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into();
        let group = self.group_of(&file, cx);
        self.add_tab(
            reach,
            PaneItem::Other {
                view: view.clone().into(),
                key: format!("file\u{0}{}", file.display()).into(),
                title,
                icon: crate::board::ICON,
                file: Some(file),
            },
            group,
            cx,
        );
        cx.notify();
        Some(view)
    }

    /// Runs `command` in a new terminal tab in `project`, the terminal shown: a canvas screen sent
    /// to a coding agent's CLI. The new terminal takes the keyboard, as the agent may ask something
    /// first (whether to trust the folder).
    pub(crate) fn run_agent_in_new_terminal(
        &mut self,
        project: Option<PathBuf>,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        self.terminal.update(cx, |t, cx| {
            t.run_in_new(project, command, crate::terminal::Origin::User, window, cx)
        })?;
        cx.notify();
        Ok(())
    }

    /// Listens to a new canvas: its Maximize button, and its agents starting or stopping work,
    /// which its tab's dot shows.
    fn watch_board(&mut self, view: &Entity<BoardView>, cx: &mut Context<Self>) {
        cx.subscribe(view, |this, _, _: &ToggleCanvasMaximize, cx| {
            this.toggle_canvas_maximized(cx)
        })
        .detach();
        cx.subscribe(view, |_, _, _: &crate::board::AgentActivity, cx| {
            cx.notify()
        })
        .detach();
    }

    /// Adds a canvas's tab to its project's group: shown, for the user; behind the tab on show,
    /// for an agent's op.
    fn add_tab(
        &mut self,
        reach: Reach,
        item: PaneItem,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        match reach {
            Reach::Show => self.push_tab(item, group, cx),
            Reach::Quietly => self.push_tab_behind(item, group, cx),
        }
    }

    /// Whether a canvas tab of this window shows the canvas of the canvas file at `path`
    /// ([`Self::board_tab`]): an `.excalidraw` file's is the one bound to the `.sawit` file
    /// beside it. An agent's op on that file goes to this window ([`crate::cli`]).
    pub(crate) fn shows_canvas_file(&self, path: &Path, cx: &App) -> bool {
        self.board_tab(&store::bound_file(path), cx).is_some()
    }

    /// The tab of the canvas bound to the canvas file `file` ([`BoardView::path`]), and the
    /// canvas, whatever opened it: a project's canvas tab, or a canvas file's. Named through a
    /// link or as written, it is one file ([`same_canvas_file`]).
    fn board_tab(&self, file: &Path, cx: &App) -> Option<(usize, Entity<BoardView>)> {
        self.tabs.iter().enumerate().find_map(|(ix, tab)| {
            let PaneItem::Other { view, .. } = &tab.item else {
                return None;
            };
            let board = view.clone().downcast::<BoardView>().ok()?;
            same_canvas_file(board.read(cx).path(), file).then_some((ix, board))
        })
    }

    /// The canvas of the tab at `ix`, if it is a canvas's.
    pub(super) fn board_at(&self, ix: usize) -> Option<Entity<BoardView>> {
        let PaneItem::Other { view, .. } = &self.tabs.get(ix)?.item else {
            return None;
        };
        view.clone().downcast::<BoardView>().ok()
    }

    /// A canvas's tab is showing in the center pane: a project's canvas, or a canvas file's.
    pub(super) fn canvas_active(&self) -> bool {
        matches!(
            self.tabs.get(self.active).map(|t| &t.item),
            Some(PaneItem::Other { view, .. }) if view.clone().downcast::<BoardView>().is_ok()
        )
    }

    /// Tells each canvas whether its tab is on screen ([`BoardView::set_shown`]), as
    /// [`Self::sync_browser`] tells the browser: the active tab, while the tab area shows. Each
    /// render checks, because the layout and the active tab change in many places. A canvas taken
    /// off screen ends what it held of the pointer, which nothing would hear the release of, and
    /// gives the keyboard, if it had it, to the workspace, which a canvas shown again takes it
    /// back from. Those going off screen are told first, so that the one coming on screen finds
    /// the keyboard where they left it, whatever their order.
    pub(super) fn sync_boards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let center = self.welcome.is_none() && self.layout.center;
        let home = self.focus.clone();
        let mut boards: Vec<(Entity<BoardView>, bool)> = (0..self.tabs.len())
            .filter_map(|ix| Some((self.board_at(ix)?, center && ix == self.active)))
            .collect();
        boards.sort_by_key(|(_, shown)| *shown);
        for (board, shown) in boards {
            board.update(cx, |board, cx| board.set_shown(shown, &home, window, cx));
        }
    }
}

/// The project that `file` is the canvas file of (spec D5), among `roots`, the folders of the
/// window's projects: `Some` of its folder as `roots` has it, which is `None` for a project with no
/// folder (its canvas is the app's own, in `app_root`). `None` when it is no project's. Named
/// through a link or as written, it is one file ([`same_canvas_file`]).
pub(super) fn canvas_project(
    file: &Path,
    roots: impl IntoIterator<Item = Option<PathBuf>>,
    app_root: &Path,
) -> Option<Option<PathBuf>> {
    roots
        .into_iter()
        .find(|root| same_canvas_file(&board_file(root.as_deref(), app_root), file))
}

/// Whether `a` and `b` name one canvas file: as written, or through the links on their way, the
/// file there yet or not (CANVAS-UI-4). An agent's `app_open` passes its path as given, where the
/// CLI's paths and the projects' folders are resolved: `/tmp/p/x.sawit` is
/// `/private/tmp/p/x.sawit` on macOS. Two canvases of one file would each save over the
/// other's edits.
fn same_canvas_file(a: &Path, b: &Path) -> bool {
    use sawit_core::files::resolve;
    a == b || resolve(a) == resolve(b)
}
