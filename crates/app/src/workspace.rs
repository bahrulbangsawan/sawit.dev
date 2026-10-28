//! The main window, laid out like Zed: title bar, left panel with tabs
//! (Servers / Files / Database), tabbed center pane, bottom terminal dock and
//! status bar.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::db::{DatabasePanel, DbEvent, DbStore, SqlView, TableView};
use crate::editor::{FileEditor, FileEditorEvent};
use crate::files::{FilesEvent, FilesPanel};
use crate::servers::{ServerDetail, ServerList, ServerStore};
use crate::terminal::TerminalPanel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Servers,
    Files,
    Database,
}

#[derive(Clone)]
enum PaneItem {
    Server,
    File(Entity<FileEditor>),
    Other(AnyView, SharedString),
}

pub struct Workspace {
    servers: Entity<ServerStore>,
    server_list: Entity<ServerList>,
    server_detail: Entity<ServerDetail>,
    files: Entity<FilesPanel>,
    db: Entity<DbStore>,
    db_panel: Entity<DatabasePanel>,
    sql_count: usize,
    terminal: Entity<TerminalPanel>,
    sidebar: SidebarTab,
    items: Vec<PaneItem>,
    active: usize,
    last_selected: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let servers = cx.new(ServerStore::new);
        let files = cx.new(FilesPanel::new);
        let terminal = cx.new(TerminalPanel::new);
        let db = cx.new(DbStore::new);
        let db_panel = cx.new(|cx| DatabasePanel::new(db.clone(), servers.clone(), window, cx));
        let subscriptions =
            vec![
                cx.observe(&servers, |this, servers, cx| {
                    let s = servers.read(cx);
                    let selected = (!s.creating).then(|| s.selected.clone()).flatten();
                    if s.creating || selected != this.last_selected {
                        this.last_selected = selected;
                        this.active = 0;
                    }
                    cx.notify();
                }),
                cx.subscribe_in(&files, window, |this, _, event: &FilesEvent, window, cx| {
                    match event {
                        FilesEvent::OpenFile(path) => this.open_file(path.clone(), window, cx),
                        FilesEvent::RootChanged(root) => {
                            let root = root.clone();
                            this.terminal.update(cx, |t, _| t.set_cwd(Some(root)));
                            cx.notify();
                        }
                    }
                }),
                cx.subscribe_in(&db_panel, window, |this, _, event: &DbEvent, window, cx| {
                    this.on_db_event(event, window, cx)
                }),
            ];
        Self {
            server_list: cx.new(|cx| ServerList::new(servers.clone(), cx)),
            server_detail: cx.new(|cx| ServerDetail::new(servers.clone(), window, cx)),
            servers,
            files,
            db,
            db_panel,
            sql_count: 0,
            terminal,
            sidebar: SidebarTab::Servers,
            items: vec![PaneItem::Server],
            active: 0,
            last_selected: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn open_project(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.files.update(cx, |f, cx| f.set_root(root, cx));
        self.sidebar = SidebarTab::Files;
        cx.notify();
    }

    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .items
            .iter()
            .position(|i| matches!(i, PaneItem::File(e) if e.read(cx).path == path))
        {
            self.active = ix;
        } else {
            let editor = cx.new(|cx| FileEditor::open(path, window, cx));
            cx.subscribe(&editor, |_, _, _: &FileEditorEvent, cx| cx.notify())
                .detach();
            self.items.push(PaneItem::File(editor));
            self.active = self.items.len() - 1;
        }
        cx.notify();
    }

    /// Opens an arbitrary view as a center tab (used by the database tools).
    pub fn open_item(&mut self, view: AnyView, title: SharedString, cx: &mut Context<Self>) {
        self.items.push(PaneItem::Other(view, title));
        self.active = self.items.len() - 1;
        cx.notify();
    }

    fn on_db_event(&mut self, event: &DbEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            DbEvent::OpenTable {
                profile,
                schema,
                table,
                columns,
            } => {
                let title: SharedString = format!("{schema}.{table}").into();
                if let Some(ix) = self
                    .items
                    .iter()
                    .position(|i| matches!(i, PaneItem::Other(_, t) if *t == title))
                {
                    self.active = ix;
                    cx.notify();
                    return;
                }
                let Some(client) = self.db.read(cx).clients.get(profile).cloned() else {
                    return;
                };
                let (schema, table, columns) = (schema.clone(), table.clone(), columns.clone());
                let view = cx.new(|cx| TableView::new(client, schema, table, columns, window, cx));
                self.open_item(view.into(), title, cx);
            }
            DbEvent::OpenSql { profile } => {
                let Some(client) = self.db.read(cx).clients.get(profile).cloned() else {
                    return;
                };
                self.sql_count += 1;
                let title: SharedString = format!("SQL {} · {profile}", self.sql_count).into();
                let view = cx.new(|cx| SqlView::new(title.clone(), client, window, cx));
                self.open_item(view.into(), title, cx);
            }
        }
    }

    fn close_item(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix == 0 || ix >= self.items.len() {
            return;
        }
        self.items.remove(ix);
        if self.active >= ix {
            self.active = self.active.saturating_sub(1);
        }
        cx.notify();
    }

    fn render_sidebar_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tab = |id: &'static str,
                   icon: Icon,
                   tooltip: &'static str,
                   which: SidebarTab,
                   current: SidebarTab| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tooltip)
                .selected(current == which)
        };
        let current = self.sidebar;
        h_flex()
            .h(px(30.))
            .px_1()
            .gap_0p5()
            .bg(cx.theme().sidebar)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                tab(
                    "side-servers",
                    Icon::new(Lucide::Server),
                    "Servers",
                    SidebarTab::Servers,
                    current,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar = SidebarTab::Servers;
                    cx.notify();
                })),
            )
            .child(
                tab(
                    "side-files",
                    Icon::new(IconName::Folder),
                    "Files",
                    SidebarTab::Files,
                    current,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar = SidebarTab::Files;
                    cx.notify();
                })),
            )
            .child(
                tab(
                    "side-db",
                    Icon::new(Lucide::Database),
                    "Database",
                    SidebarTab::Database,
                    current,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar = SidebarTab::Database;
                    cx.notify();
                })),
            )
    }

    fn render_sidebar(&self) -> AnyElement {
        match self.sidebar {
            SidebarTab::Servers => self.server_list.clone().into_any_element(),
            SidebarTab::Files => self.files.clone().into_any_element(),
            SidebarTab::Database => self.db_panel.clone().into_any_element(),
        }
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tabs: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .map(|(ix, item)| {
                let active = ix == self.active;
                let (icon, title, dirty) = match item {
                    PaneItem::Server => (Icon::new(Lucide::Server), "Server".to_string(), false),
                    PaneItem::File(e) => {
                        let e = e.read(cx);
                        (Icon::new(Lucide::FileCode), e.title(), e.dirty)
                    }
                    PaneItem::Other(_, title) => {
                        (Icon::new(Lucide::Table), title.to_string(), false)
                    }
                };
                h_flex()
                    .id(("pane-tab", ix))
                    .h_full()
                    .px_3()
                    .gap_1p5()
                    .text_sm()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .when(active, |el| {
                        el.bg(cx.theme().tab_active)
                            .text_color(cx.theme().tab_active_foreground)
                    })
                    .when(!active, |el| {
                        el.bg(cx.theme().tab).text_color(cx.theme().tab_foreground)
                    })
                    .child(icon.xsmall())
                    .child(title)
                    .when(dirty, |el| {
                        el.child(div().size(px(6.)).rounded_full().bg(cx.theme().foreground))
                    })
                    .when(ix > 0, |el| {
                        el.child(
                            Button::new(("close-tab", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::X)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.close_item(ix, cx)),
                                ),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.active = ix;
                        cx.notify();
                    }))
            })
            .collect();
        h_flex()
            .h(px(32.))
            .bg(cx.theme().tab_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .overflow_x_hidden()
            .children(tabs)
    }

    fn render_active_item(&self) -> AnyElement {
        match self.items.get(self.active) {
            Some(PaneItem::File(editor)) => editor.clone().into_any_element(),
            Some(PaneItem::Other(view, _)) => view.clone().into_any_element(),
            _ => self.server_detail.clone().into_any_element(),
        }
    }

    fn render_status_bar(&self, cx: &App) -> impl IntoElement + use<> {
        let store = self.servers.read(cx);
        let running = store.running_count();
        let total = store.registry.instances.len();
        let project = self
            .files
            .read(cx)
            .root
            .as_ref()
            .map(|r| r.display().to_string())
            .unwrap_or_default();
        h_flex()
            .h(px(24.))
            .px_2()
            .gap_3()
            .justify_between()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .bg(cx.theme().status_bar)
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .child(
                h_flex()
                    .gap_1()
                    .child(div().size(px(6.)).rounded_full().bg(if running > 0 {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    }))
                    .child(format!("{running}/{total} instance berjalan")),
            )
            .child(div().truncate().child(project))
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .size_full()
                        .gap_2()
                        .text_sm()
                        .child(div().font_semibold().child("PG Desk"))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("Postgres · Editor · Terminal"),
                        ),
                ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("workspace-h")
                        .child(
                            resizable_panel()
                                .size(px(240.))
                                .size_range(px(160.)..px(480.))
                                .child(
                                    v_flex()
                                        .size_full()
                                        .border_r_1()
                                        .border_color(cx.theme().border)
                                        .child(self.render_sidebar_tabs(cx))
                                        .child(
                                            div().flex_1().min_h_0().child(self.render_sidebar()),
                                        ),
                                ),
                        )
                        .child(
                            resizable_panel().child(
                                v_resizable("workspace-v")
                                    .child(
                                        resizable_panel().child(
                                            v_flex()
                                                .size_full()
                                                .child(self.render_tab_bar(cx))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_h_0()
                                                        .child(self.render_active_item()),
                                                ),
                                        ),
                                    )
                                    .child(
                                        resizable_panel()
                                            .size(px(240.))
                                            .size_range(px(80.)..Pixels::MAX)
                                            .child(self.terminal.clone()),
                                    ),
                            ),
                        ),
                ),
            )
            .child(self.render_status_bar(cx))
    }
}
