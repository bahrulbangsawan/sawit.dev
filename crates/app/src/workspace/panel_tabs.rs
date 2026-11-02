//! What the sidebar's panels open in tabs: a database's tables, SQL and diagrams, a prompt, an SSH
//! server's settings, and Git's history, commit graph, commits, diffs and GitHub page.

use super::*;

impl Workspace {
    /// The profile's (read-only) client, or `None` after starting a
    /// reconnect when the old connection died.
    fn live_client(
        &self,
        profile: &str,
        read_only: bool,
        cx: &mut Context<Self>,
    ) -> Option<sawit_core::db::Conn> {
        let client = self.db.read(cx).client(profile, read_only);
        if client.is_none() {
            self.db.update(cx, |s, cx| s.ensure_connecting(profile, cx));
        }
        client
    }

    pub(super) fn on_db_event(
        &mut self,
        event: &DbEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DbEvent::OpenTable {
                profile,
                schema,
                table,
            } => {
                // One tab per table per connection.
                let key: SharedString =
                    format!("table\u{0}{profile}\u{0}{schema}\u{0}{table}").into();
                if let Some(ix) = self
                    .tabs
                    .iter()
                    .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
                {
                    self.active = ix;
                    cx.notify();
                    return;
                }
                if self.live_client(profile, true, cx).is_none() {
                    self.pending_db = Some(event.clone());
                    return;
                }
                let title: SharedString = format!("{schema}.{table}").into();
                let (db, profile, schema, table) = (
                    self.db.clone(),
                    profile.clone(),
                    schema.clone(),
                    table.clone(),
                );
                let view = cx.new(|cx| {
                    TableView::new(title.clone(), db, profile, schema, table, window, cx)
                });
                // Its "Query" button opens a SQL editor.
                cx.subscribe_in(&view, window, |this, _, event: &DbEvent, window, cx| {
                    this.on_db_event(event, window, cx)
                })
                .detach();
                self.open_item(view.into(), key, title, Lucide::Table, cx);
            }
            DbEvent::OpenSql { profile, sql } => {
                if self.live_client(profile, false, cx).is_none() {
                    self.pending_db = Some(event.clone());
                    return;
                }
                self.sql_count += 1;
                let shown = self
                    .db
                    .read(cx)
                    .profile(profile)
                    .map_or(profile.as_str(), |p| p.display_name())
                    .to_string();
                let title: SharedString = format!("SQL {} · {shown}", self.sql_count).into();
                let (db, profile, sql) = (self.db.clone(), profile.clone(), sql.clone());
                let view = cx.new(|cx| SqlView::new(title.clone(), db, profile, sql, window, cx));
                if std::mem::take(&mut self.run_next_sql) {
                    view.update(cx, |v, cx| v.run_now(window, cx));
                }
                self.open_item(
                    view.into(),
                    title.clone(),
                    title,
                    Lucide::SquareTerminal,
                    cx,
                );
            }
            DbEvent::OpenDiagram { profile, schema } => {
                let key: SharedString = format!("diagram\u{0}{profile}\u{0}{schema}").into();
                if let Some(ix) = self
                    .tabs
                    .iter()
                    .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
                {
                    self.active = ix;
                    cx.notify();
                    return;
                }
                if self.live_client(profile, true, cx).is_none() {
                    self.pending_db = Some(event.clone());
                    return;
                }
                let title: SharedString = format!("{schema} · diagram").into();
                let (db, profile, schema) = (self.db.clone(), profile.clone(), schema.clone());
                let view =
                    cx.new(|cx| DiagramView::new(title.clone(), db, profile, schema, window, cx));
                self.open_item(view.into(), key, title, Lucide::Waypoints, cx);
            }
        }
    }

    /// Opens a saved prompt in its own tab (one per prompt).
    pub(super) fn open_prompt(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let key: SharedString = format!("prompt\u{0}{id}").into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let store = self.prompt_store.clone();
            let view = cx.new(|cx| PromptView::new(id, store, window, cx));
            let title = view.read(cx).title(cx).into();
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title,
                    icon: Lucide::MessageSquareText,
                    file: None,
                },
                None,
                cx,
            );
        }
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        cx.notify();
    }

    /// Opens a saved server's settings in its own tab (one per server).
    pub(super) fn open_ssh_host(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        let key: SharedString = format!("ssh-host\u{0}{id}").into();
        if !self.activate_key(&key) {
            let store = self.ssh_store.clone();
            let view = cx.new(|cx| SshHostView::new(id, store, window, cx));
            cx.subscribe_in(
                &view,
                window,
                |this, _, SshHostEvent::Connect(key), window, cx| {
                    let _ = this.connect_ssh(key.clone(), false, Origin::User, window, cx);
                },
            )
            .detach();
            let title = view.read(cx).title(cx).into();
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title,
                    icon: Lucide::Server,
                    file: None,
                },
                None,
                cx,
            );
        }
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        cx.notify();
    }

    /// The commits touching one file or folder (one tab each).
    pub(super) fn open_git_history(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.show_center(cx);
        let key = format!("git-history\u{0}{}", path.display());
        if !self.activate_key(&key) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| GraphView::history(git, path, cx));
            cx.subscribe(&view, |this, _, GraphEvent::OpenCommit(commit), cx| {
                this.open_commit(commit.clone(), cx)
            })
            .detach();
            self.open_item_in(
                view.into(),
                key.into(),
                format!("History: {name}").into(),
                Lucide::GitCommitHorizontal,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// The commit graph (one tab).
    pub(super) fn open_git_graph(&mut self, cx: &mut Context<Self>) {
        self.show_center(cx);
        let key = self.project_key("git-graph", cx);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| GraphView::new(git, cx));
            cx.subscribe(&view, |this, _, GraphEvent::OpenCommit(commit), cx| {
                this.open_commit(commit.clone(), cx)
            })
            .detach();
            self.open_item_in(
                view.into(),
                key.into(),
                "Commit Graph".into(),
                Lucide::GitGraph,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// One commit, one tab per commit.
    fn open_commit(&mut self, commit: sawit_core::git::Commit, cx: &mut Context<Self>) {
        let key = CommitView::key(&commit.hash);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| CommitView::new(git, commit, cx));
            let title = view.read(cx).title();
            self.open_item_in(
                view.into(),
                key,
                title,
                Lucide::GitCommitHorizontal,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// Starts the one GitHub sign-in (unless one is under way) and shows the GitHub page, whose
    /// card shows the code to enter: from the title bar, the Welcome screen, or a "Sign in"
    /// beside a GitHub control. Only ever from the user's click: nothing signs in by itself.
    pub(super) fn sign_in_to_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        self.github.update(cx, |a, cx| a.sign_in(cx));
        self.open_github(window, cx);
    }

    /// The GitHub tab (one tab).
    pub(super) fn open_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_center(cx);
        let key = self.project_key("github", cx);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| GitHubView::new(git, window, cx));
            self.open_item_in(
                view.into(),
                key.into(),
                "GitHub".into(),
                Lucide::Github,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// A changed file's diff, one tab per file and side.
    pub(super) fn open_git_diff(
        &mut self,
        file: sawit_core::git::FileChange,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_center(cx);
        let key = self.project_key(&DiffView::key(&file, staged), cx);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| DiffView::new(git, file, staged, cx));
            cx.subscribe_in(
                &view,
                window,
                |this, _, e: &DiffViewEvent, window, cx| match e {
                    DiffViewEvent::OpenFile(path) => this.open_file(path.clone(), window, cx),
                },
            )
            .detach();
            let title = view.read(cx).title();
            self.open_item_in(
                view.into(),
                key.into(),
                title,
                Lucide::GitCommitHorizontal,
                group,
                cx,
            );
        }
        cx.notify();
    }
}
