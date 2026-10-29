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

impl Workspace {
    /// A project with no folder yet, wired to this workspace.
    pub(super) fn new_project(
        run_store: &Entity<RunStore>,
        terminal: &Entity<TerminalPanel>,
        ports: &Entity<PortsPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Project {
        let files = cx.new(FilesPanel::new);
        let git = cx.new(GitStore::new);
        let git_panel = cx.new(|cx| GitPanel::new(git.clone(), window, cx));
        let run_panel = cx.new(|cx| {
            RunPanel::new(run_store.clone(), terminal.clone(), ports.clone(), window, cx)
        });
        let subscriptions = vec![
            cx.subscribe_in(&files, window, |this, files, event: &FilesEvent, window, cx| {
                this.on_files_event(files, event, window, cx)
            }),
            cx.observe(&git, |_, _, cx| cx.notify()),
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
            FilesEvent::OpenFile(path) => self.open_file(path.clone(), window, cx),
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
            FilesEvent::OpenTerminal(dir) => {
                self.leave_welcome(Layout::default(), cx);
                if !self.layout.terminal {
                    self.layout.toggle(Area::Terminal);
                }
                self.terminal
                    .update(cx, |t, cx| t.open_shell_in(dir.clone(), window, cx));
                cx.notify();
            }
            FilesEvent::FindInFolder(dir) => self.open_folder_search(dir.clone(), window, cx),
            FilesEvent::ViewHistory(path) => self.open_git_history(path.clone(), cx),
        }
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
    /// already, else fills an empty project, else adds one.
    pub fn open_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(Layout::default(), cx);
        self.sidebar = SidebarTab::Files;
        if let Some(ix) = self.projects.iter().position(|p| p.root(cx) == Some(root.clone())) {
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
        self.current().files.update(cx, |f, cx| f.set_root(root, cx));
        cx.notify();
    }

    /// Shows `root` in the active project instead of its folder (`sawit -r`).
    pub fn replace_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.projects.iter().any(|p| p.root(cx) == Some(root.clone()));
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
        self.current().files.update(cx, |f, cx| f.set_root(root, cx));
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
        if let Some(ix) = self.projects.iter().position(|p| p.root(cx) == Some(root.clone())) {
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
        if unsaved == 0 {
            self.remove_project(&root, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Close {} with {unsaved} unsaved file{}?",
                crate::tab_groups::project_name(&root),
                if unsaved == 1 { "" } else { "s" }
            ),
            Some("Your changes will be lost if you don't save them."),
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
