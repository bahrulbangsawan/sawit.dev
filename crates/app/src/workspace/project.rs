//! The projects open in one window. Each has its own Files, Git and Run
//! panels; the sidebar shows the active one's. Tabs and terminals are
//! grouped by the project they belong to.

use super::*;

pub(super) struct Project {
    pub files: Entity<FilesPanel>,
    pub git: Entity<GitStore>,
    pub git_panel: Entity<GitPanel>,
    pub run_panel: Entity<RunPanel>,
    _subscriptions: Vec<Subscription>,
}

impl Project {
    pub fn root(&self, cx: &App) -> Option<PathBuf> {
        self.files.read(cx).root.clone()
    }
}

/// The folder a project opened at `dir` is: `dir` as it is on disk, through the links on its way
/// and in the letter case there ([`sawit_core::files::resolve`]). The CLI and `app_open` name
/// folders so, and so do agents, by their working folder; the Open dialog keeps a link picked
/// there as the link, and Recent Projects reopens folders as they were remembered. Opened as
/// named, one folder could be two projects of a window, and what is keyed by the project's
/// folder (saved commands, the browser's cookies, which project a file or a dev server is in)
/// didn't match what agents and the CLI name (review-mcp M1).
fn project_folder(dir: &Path) -> PathBuf {
    sawit_core::files::resolve(dir)
}

impl Workspace {
    /// A project with no folder yet, wired to this workspace.
    pub(super) fn new_project(
        run_store: &Entity<RunStore>,
        terminal: &Entity<TerminalPanel>,
        ports: &Entity<PortsPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Project {
        let files = cx.new(|cx| FilesPanel::new(window, cx));
        let git = cx.new(GitStore::new);
        let git_panel = cx.new(|cx| GitPanel::new(git.clone(), window, cx));
        let run_panel = cx.new(|cx| {
            RunPanel::new(
                run_store.clone(),
                terminal.clone(),
                ports.clone(),
                window,
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &files,
                window,
                |this, files, event: &FilesEvent, window, cx| {
                    this.on_files_event(files, event, window, cx)
                },
            ),
            // The Files tab's Git items follow the repository the store finds, or loses.
            cx.observe(&git, {
                let files = files.clone();
                move |_, git, cx| {
                    let repo = git.read(cx).repo.clone();
                    files.update(cx, |files, cx| files.set_repo(repo, cx));
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &git_panel,
                window,
                |this, _, event: &GitPanelEvent, window, cx| match event {
                    GitPanelEvent::Diff { file, staged } => {
                        this.open_git_diff(file.clone(), *staged, window, cx)
                    }
                    GitPanelEvent::Graph => this.open_git_graph(cx),
                    GitPanelEvent::GitHub => this.open_github(window, cx),
                },
            ),
            cx.subscribe_in(
                &run_panel,
                window,
                |this, _, event: &RunEvent, window, cx| this.on_run_event(event, window, cx),
            ),
        ];
        Project {
            files,
            git,
            git_panel,
            run_panel,
            _subscriptions: subscriptions,
        }
    }

    fn on_files_event(
        &mut self,
        files: &Entity<FilesPanel>,
        event: &FilesEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FilesEvent::OpenFile(path) => {
                self.open_file(path.clone(), window, cx);
                let shown = self.tabs.get(self.active).map(|tab| tab.item.clone());
                give_keyboard(shown.as_ref(), &self.focus, window, cx);
            }
            FilesEvent::RootChanged(root) => {
                remember_recent(root.clone());
                if let Some(p) = self.projects.iter().find(|p| p.files == *files) {
                    let root = Some(root.clone());
                    p.git.update(cx, |g, cx| g.set_dir(root.clone(), cx));
                    p.run_panel.update(cx, |r, cx| r.set_root(root, cx));
                }
                // `files` hasn't stored the new root yet: sync once it has.
                cx.defer_in(window, |this, _, cx| this.sync_projects(cx));
            }
            FilesEvent::Moved { from, to } => self.follow_move(from, to, cx),
            FilesEvent::OpenTerminal(dir) => self.open_terminal_in(dir.clone(), window, cx),
            FilesEvent::FindInFolder(dir) => self.open_folder_search(dir.clone(), window, cx),
            FilesEvent::ViewHistory(path) => self.open_git_history(path.clone(), cx),
            FilesEvent::OpenFolder(dir) => {
                let (dir, handle) = (dir.clone(), window.window_handle());
                let workspace = cx.entity().downgrade();
                // Once this update is over: it looks at every window's projects.
                cx.defer(move |cx| crate::cli::open_folder_from_files(dir, handle, workspace, cx));
            }
        }
    }

    /// Shows the terminal with a new shell in `dir`.
    pub(super) fn open_terminal_in(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(Layout::default(), cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        self.terminal
            .update(cx, |t, cx| t.open_shell_in(dir, window, cx));
        cx.notify();
    }

    pub(super) fn current(&self) -> &Project {
        &self.projects[self.project]
    }

    /// Every open project's folder, in switcher order.
    pub fn roots(&self, cx: &App) -> Vec<PathBuf> {
        self.projects.iter().filter_map(|p| p.root(cx)).collect()
    }

    /// The project folder holding `path`, if any.
    pub(super) fn group_of(&self, path: &Path, cx: &App) -> Option<PathBuf> {
        crate::tabs::project_of(&self.roots(cx), path)
    }

    /// The colour of the group for `root`.
    pub(super) fn group_color(&self, root: &Path, cx: &App) -> Hsla {
        let ix = self.roots(cx).iter().position(|r| r == root).unwrap_or(0);
        crate::tab_groups::group_color(ix, cx)
    }

    /// Opens `root` as a project of this window: shows it if it is open
    /// already, else fills an empty project, else adds one. The project is
    /// the folder as it is on disk ([`project_folder`]).
    pub fn open_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let root = project_folder(&root);
        self.leave_welcome(Layout::default(), cx);
        self.sidebar = SidebarTab::Files;
        if let Some(ix) = self
            .projects
            .iter()
            .position(|p| p.root(cx) == Some(root.clone()))
        {
            self.activate_project(ix, cx);
            return;
        }
        if self.current().root(cx).is_some() {
            let project = Self::new_project(
                &self.run_store,
                &self.terminal,
                &self.ports_panel,
                window,
                cx,
            );
            self.projects.push(project);
            self.project = self.projects.len() - 1;
        }
        self.current()
            .files
            .update(cx, |f, cx| f.set_root(root, cx));
        cx.notify();
    }

    /// Shows `root` in the active project instead of its folder (`sawit -r`), as
    /// it is on disk ([`project_folder`]).
    pub fn replace_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let root = project_folder(&root);
        let open = self
            .projects
            .iter()
            .any(|p| p.root(cx) == Some(root.clone()));
        if open || self.root(cx).is_none() {
            return self.open_project(root, window, cx);
        }
        self.leave_welcome(Layout::default(), cx);
        self.sidebar = SidebarTab::Files;
        let old = self.root(cx);
        // Its tabs keep their files, in no group now.
        for tab in &mut self.tabs {
            if tab.group.is_some() && tab.group == old {
                tab.group = None;
            }
        }
        let active = self.active;
        let mut placed = None;
        for (ix, tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            let at = insert_index(&self.slots(), tab.pinned, tab.group.as_deref());
            self.tabs.insert(at, tab);
            placed = match placed {
                _ if ix == active => Some(at),
                Some(a) if at <= a => Some(a + 1),
                other => other,
            };
        }
        self.active = placed.unwrap_or(0);
        self.current()
            .files
            .update(cx, |f, cx| f.set_root(root, cx));
        cx.notify();
    }

    pub(super) fn activate_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.projects.len() && ix != self.project {
            self.project = ix;
            self.sync_projects(cx);
        }
    }

    /// Makes the project holding `path` the active one.
    pub fn activate_project_of(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(root) = self.group_of(path, cx) else {
            return;
        };
        if let Some(ix) = self
            .projects
            .iter()
            .position(|p| p.root(cx) == Some(root.clone()))
        {
            self.activate_project(ix, cx);
        }
    }

    /// Tells the terminal dock which projects are open and which is active.
    pub(super) fn sync_projects(&mut self, cx: &mut Context<Self>) {
        let roots = self.roots(cx);
        let active = self.root(cx);
        self.collapsed.retain(|r| roots.contains(r));
        self.terminal
            .update(cx, |t, cx| t.set_projects(roots, active, cx));
        cx.notify();
    }

    /// Closes the project at `ix` with its tabs, asking first if any has
    /// unsaved changes. Its terminals keep running, ungrouped.
    pub(super) fn close_project(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.projects.get(ix).and_then(|p| p.root(cx)) else {
            return;
        };
        let unsaved = self
            .tabs
            .iter()
            .filter(|t| t.group.as_ref() == Some(&root))
            .filter(|t| matches!(&t.item, PaneItem::File(e) if e.read(cx).dirty))
            .count();
        // Its browser's annotation notes not yet copied or sent go with it too.
        let notes: usize = self
            .tabs
            .iter()
            .filter(|t| t.group.as_ref() == Some(&root))
            .filter_map(|t| match &t.item {
                PaneItem::Other { view, .. } => view.clone().downcast::<BrowserView>().ok(),
                _ => None,
            })
            .map(|b| b.read(cx).unsent_notes(cx))
            .sum();
        if unsaved == 0 && notes == 0 {
            self.remove_project(&root, window, cx);
            return;
        }
        let name = crate::tab_groups::project_name(&root);
        let (message, detail) = if unsaved > 0 {
            (
                format!(
                    "Close {name} with {unsaved} unsaved file{}?",
                    if unsaved == 1 { "" } else { "s" }
                ),
                "Your changes will be lost if you don't save them.".to_string(),
            )
        } else {
            (
                format!("Close {name} with its browser's annotations?"),
                crate::browser::notes_lost(notes),
            )
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&detail),
            &["Close Anyway", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(0) = answer.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.remove_project(&root, window, cx)
                });
            }
        })
        .detach();
    }

    fn remove_project(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self
            .projects
            .iter()
            .position(|p| p.root(cx).as_deref() == Some(root))
        else {
            return;
        };
        let closing: Vec<usize> = (0..self.tabs.len())
            .filter(|&i| self.tabs[i].group.as_deref() == Some(root))
            .collect();
        self.remove_tabs(&closing, cx);
        self.projects.remove(ix);
        if self.projects.is_empty() {
            // Keep one, empty, for the Files panel's "Open Folder".
            let empty = Self::new_project(
                &self.run_store,
                &self.terminal,
                &self.ports_panel,
                window,
                cx,
            );
            self.projects.push(empty);
        }
        if ix < self.project {
            self.project -= 1;
        }
        self.project = self.project.min(self.projects.len() - 1);
        self.sync_projects(cx);
    }
}

/// Gives the keyboard to the tab of the file the Files tab just opened or showed, `item`: its
/// editor (its text once read), or a video's player. An image or a PDF takes no keys, so the
/// workspace (`home`) has them then; a canvas took them as it opened. Left in the tree, where the
/// click on the file put them, the keys meant for the file went to the tree (FILES-1): ⌘Z undid
/// a file change, ⌘D duplicated the file, ⌘V pasted one. A file opened the first time already
/// had them once read; one whose tab was open, or that couldn't be read as text, didn't.
fn give_keyboard(item: Option<&PaneItem>, home: &FocusHandle, window: &mut Window, cx: &mut App) {
    match item {
        Some(PaneItem::File(editor)) => editor.update(cx, |e, cx| e.take_keyboard(window, cx)),
        Some(PaneItem::Other { view, .. }) => {
            if view.clone().downcast::<BoardView>().is_ok() {
                return;
            }
            match view.clone().downcast::<VideoView>() {
                Ok(video) => {
                    let player = video.read(cx).focus_handle(cx);
                    window.focus(&player, cx);
                }
                Err(_) => window.focus(home, cx),
            }
        }
        Some(PaneItem::Server) | None => {}
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
        IntoElement, ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
        prelude::InteractiveElement as _,
    };

    use super::{FileEditor, FolderSearch, ImageView, PaneItem, follow_tab, give_keyboard};
    use crate::files::Scratch;

    /// A window with the Files tree's keyboard, the workspace's, and a text file's and an
    /// image's tab.
    struct Window2 {
        tree: FocusHandle,
        home: FocusHandle,
        editor: Entity<FileEditor>,
        image: Entity<ImageView>,
    }

    impl Render for Window2 {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(&self.home)
                .child(div().track_focus(&self.tree))
                .child(self.editor.clone())
                .child(self.image.clone())
        }
    }

    /// Whether `handle`, picked from the window's view, has the keyboard.
    fn has(
        window: AnyWindowHandle,
        view: &Entity<Window2>,
        cx: &mut TestAppContext,
        handle: impl Fn(&Window2, &gpui_kit::App) -> FocusHandle,
    ) -> bool {
        cx.update_window(window, |_, window, cx| {
            handle(view.read(cx), cx).is_focused(window)
        })
        .unwrap()
    }

    /// Gives the keyboard to the tree, then hands it on as the Files tab's OpenFile does.
    fn open_from_tree(
        window: AnyWindowHandle,
        view: &Entity<Window2>,
        cx: &mut TestAppContext,
        item: impl Fn(&Window2) -> PaneItem,
    ) {
        cx.update_window(window, |_, window, cx| {
            let (tree, home, item) = {
                let v = view.read(cx);
                (v.tree.clone(), v.home.clone(), item(v))
            };
            window.focus(&tree, cx);
            give_keyboard(Some(&item), &home, window, cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// The file the Files tab opens or shows takes the keyboard from the tree (FILES-1): a text
    /// file's editor, its text once read; an image, which takes no keys, leaves them with the
    /// workspace. Reopening a file whose tab was open left them in the tree, unseen, where ⌫,
    /// ⌘Z, ⌘D and ⌘V meant for the file acted on files.
    #[gpui_kit::test]
    fn a_file_shown_from_the_files_tab_takes_the_keyboard(cx: &mut TestAppContext) {
        // The files are read on the runtime's real threads.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
        });
        let dir = Scratch::new("give-keyboard");
        let (notes, picture) = (dir.0.join("notes.md"), dir.0.join("picture.png"));
        std::fs::write(&notes, "notes\n").unwrap();
        std::fs::write(&picture, b"not a picture").unwrap();
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                let editor = cx.new(|cx| FileEditor::open(notes, None, window, cx));
                let image = cx.new(|cx| ImageView::new(picture, cx));
                cx.new(|cx| Window2 {
                    tree: cx.focus_handle(),
                    home: cx.focus_handle(),
                    editor,
                    image,
                })
            })
            .unwrap()
        });
        let editor = |v: &Window2| PaneItem::File(v.editor.clone());
        let text = |v: &Window2, cx: &gpui_kit::App| {
            let editor = v.editor.read(cx);
            editor
                .text_focus(cx)
                .unwrap_or_else(|| v.editor.focus_handle(cx))
        };

        // Before the file is read, the editor has the keyboard; its text takes it once read.
        open_from_tree(window, &view, cx, editor);
        assert!(has(window, &view, cx, text), "the tree kept the keyboard");
        let deadline = Instant::now() + Duration::from_secs(20);
        while cx.update(|cx| view.read(cx).editor.read(cx).text_focus(cx).is_none()) {
            assert!(Instant::now() < deadline, "the file was never read");
            std::thread::sleep(Duration::from_millis(10));
            cx.run_until_parked();
        }
        // Shown again from the tree, as a file whose tab is open is.
        open_from_tree(window, &view, cx, editor);
        assert!(
            has(window, &view, cx, text),
            "the text doesn't have the keyboard"
        );

        let image = |v: &Window2| PaneItem::Other {
            view: v.image.clone().into(),
            key: "file\u{0}picture.png".into(),
            title: "picture.png".into(),
            icon: gpui_kit::assets::IconName::Image,
            file: None,
        };
        open_from_tree(window, &view, cx, image);
        assert!(has(window, &view, cx, |v, _| v.home.clone()));
        assert!(!has(window, &view, cx, |v, _| v.tree.clone()));
    }

    /// The tabs of a renamed or moved file follow it (FILES-14): an image's, a PDF's or a
    /// video's, as a text file's and a canvas file's did, and a Find in Folder tab its folder
    /// and the files it found. They kept the old path: the file renamed opened in a second tab,
    /// `sawit --wait` waited on a path that was gone, and a search ran where its folder was.
    #[gpui_kit::test]
    fn viewer_and_search_tabs_follow_a_move(cx: &mut TestAppContext) {
        // The image is looked at on the runtime's real threads.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
        });
        let dir = Scratch::new("tabs-follow-a-move");
        let (a, b) = (dir.0.join("a.png"), dir.0.join("b.png"));
        let (docs, notes) = (dir.0.join("docs"), dir.0.join("notes"));
        std::fs::write(&a, b"not a picture").unwrap();
        std::fs::create_dir(&docs).unwrap();
        let (image, search) = (a.clone(), docs.clone());
        let (image, search) = cx.update(|cx| {
            let (_, pair) = gpui_kit::open_window(Default::default(), cx, |window, cx| {
                let image = cx.new(|cx| ImageView::new(image, cx));
                let search = cx.new(|cx| FolderSearch::new(search, window, cx));
                cx.new(|_| Pair(image, search))
            })
            .unwrap();
            let pair = pair.read(cx);
            (pair.0.clone(), pair.1.clone())
        });
        let mut image_tab = PaneItem::Other {
            view: image.clone().into(),
            key: format!("file\u{0}{}", a.display()).into(),
            title: "a.png".into(),
            icon: gpui_kit::assets::IconName::Image,
            file: Some(a.clone()),
        };
        let mut search_tab = PaneItem::Other {
            view: search.clone().into(),
            key: FolderSearch::key(&docs).into(),
            title: "Search: docs".into(),
            icon: gpui_kit::assets::IconName::Search,
            file: None,
        };

        cx.update(|cx| {
            follow_tab(&mut image_tab, &a, &b, cx);
            follow_tab(&mut search_tab, &docs, &notes, cx);
            let PaneItem::Other {
                key, title, file, ..
            } = &image_tab
            else {
                unreachable!()
            };
            assert_eq!(key.as_ref(), format!("file\u{0}{}", b.display()));
            assert_eq!((title.as_ref(), file.as_ref()), ("b.png", Some(&b)));
            assert_eq!(image.read(cx).path, b);
            let PaneItem::Other { key, title, .. } = &search_tab else {
                unreachable!()
            };
            assert_eq!(key.as_ref(), FolderSearch::key(&notes));
            assert_eq!(title.as_ref(), "Search: notes");
            assert_eq!(search.read(cx).dir, notes);
        });
    }

    /// A window's view holding an image's and a search's tab.
    struct Pair(Entity<ImageView>, Entity<FolderSearch>);

    impl Render for Pair {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(self.0.clone())
                .child(self.1.clone())
        }
    }
}
