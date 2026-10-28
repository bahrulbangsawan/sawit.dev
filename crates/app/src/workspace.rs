//! The main window, laid out like Zed: title bar, left panel with tabs
//! (Servers / Files / Database), tabbed center pane, bottom terminal dock and
//! status bar.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::db::{DatabasePanel, DbEvent, DbStore, SqlView, TableView};
use crate::editor::{FileEditor, FileEditorEvent};
use crate::files::{FilesEvent, FilesPanel};
use crate::layout::{Area, Layout};
use crate::servers::{ServerDetail, ServerList, ServerStore, ShowServerTab};
use crate::tabs::{CloseOp, TabFlags, active_after, to_close};
use crate::terminal::TerminalPanel;

actions!(
    workspace,
    [
        CloseActiveTab,
        CloseOtherTabs,
        CloseTabsLeft,
        CloseTabsRight,
        CloseCleanTabs,
        CloseAllTabs,
        TogglePinTab,
        ToggleSidebar,
        ToggleTerminal,
        ToggleCenter
    ]
);

/// Zed's default bindings for the same commands.
pub fn init(cx: &mut App) {
    let ctx = Some("Workspace");
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseActiveTab, ctx),
        KeyBinding::new("alt-cmd-t", CloseOtherTabs, ctx),
        KeyBinding::new("cmd-k e", CloseTabsLeft, ctx),
        KeyBinding::new("cmd-k t", CloseTabsRight, ctx),
        KeyBinding::new("cmd-k u", CloseCleanTabs, ctx),
        KeyBinding::new("cmd-k w", CloseAllTabs, ctx),
        KeyBinding::new("cmd-k shift-enter", TogglePinTab, ctx),
        KeyBinding::new("cmd-b", ToggleSidebar, ctx),
        KeyBinding::new("cmd-j", ToggleTerminal, ctx),
        KeyBinding::new("cmd-shift-j", ToggleCenter, ctx),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Servers,
    Files,
    Database,
}

#[derive(Clone)]
struct Tab {
    item: PaneItem,
    pinned: bool,
}

#[derive(Clone)]
enum PaneItem {
    Server,
    File(Entity<FileEditor>),
    Other {
        view: AnyView,
        /// Identity for "already open?" checks.
        key: SharedString,
        title: SharedString,
    },
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
    tabs: Vec<Tab>,
    active: usize,
    layout: Layout,
    /// A table/SQL tab asked for while its connection was being re-made.
    pending_db: Option<DbEvent>,
    focus: FocusHandle,
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
                cx.observe(&servers, |_, _, cx| cx.notify()),
                cx.subscribe(&servers, |this, _, _: &ShowServerTab, cx| {
                    this.show_server_tab();
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
                cx.observe_in(&db, window, |this, db, window, cx| {
                    let ready = match &this.pending_db {
                        Some(DbEvent::OpenTable { profile, .. } | DbEvent::OpenSql { profile }) => {
                            let store = db.read(cx);
                            if store.connecting.contains(profile) {
                                return;
                            }
                            store.is_connected(profile)
                        }
                        None => return,
                    };
                    // Replay once connected; drop it if reconnecting failed.
                    if let Some(event) = this.pending_db.take()
                        && ready
                    {
                        this.on_db_event(&event, window, cx);
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
            tabs: vec![Tab {
                item: PaneItem::Server,
                pinned: false,
            }],
            active: 0,
            layout: Layout::default(),
            pending_db: None,
            focus: cx.focus_handle(),
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
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::File(e) if e.read(cx).path == path))
        {
            self.active = ix;
        } else {
            let editor = cx.new(|cx| FileEditor::open(path, window, cx));
            cx.subscribe(&editor, |_, _, _: &FileEditorEvent, cx| cx.notify())
                .detach();
            self.push_tab(PaneItem::File(editor));
        }
        cx.notify();
    }

    /// Opens an arbitrary view as a center tab (used by the database tools).
    pub fn open_item(
        &mut self,
        view: AnyView,
        key: SharedString,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.push_tab(PaneItem::Other { view, key, title });
        cx.notify();
    }

    fn push_tab(&mut self, item: PaneItem) {
        self.tabs.push(Tab {
            item,
            pinned: false,
        });
        self.active = self.tabs.len() - 1;
    }

    /// Activates the Server tab, reopening it (after pinned tabs) if closed.
    fn show_server_tab(&mut self) {
        match self
            .tabs
            .iter()
            .position(|t| matches!(t.item, PaneItem::Server))
        {
            Some(ix) => self.active = ix,
            None => {
                let at = self.tabs.iter().take_while(|t| t.pinned).count();
                self.tabs.insert(
                    at,
                    Tab {
                        item: PaneItem::Server,
                        pinned: false,
                    },
                );
                self.active = at;
            }
        }
    }

    /// The profile's (read-only) client, or `None` after starting a
    /// reconnect when the old connection died.
    fn live_client(
        &self,
        profile: &str,
        read_only: bool,
        cx: &mut Context<Self>,
    ) -> Option<std::sync::Arc<tokio_postgres::Client>> {
        let store = self.db.read(cx);
        if !store.is_connected(profile) {
            self.db.update(cx, |s, cx| s.connect(profile, cx));
            return None;
        }
        let clients = if read_only {
            &store.ro_clients
        } else {
            &store.clients
        };
        clients.get(profile).cloned()
    }

    fn on_db_event(&mut self, event: &DbEvent, window: &mut Window, cx: &mut Context<Self>) {
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
                let Some(client) = self.live_client(profile, true, cx) else {
                    self.pending_db = Some(event.clone());
                    return;
                };
                let title: SharedString = format!("{schema}.{table}").into();
                let (schema, table) = (schema.clone(), table.clone());
                let view =
                    cx.new(|cx| TableView::new(title.clone(), client, schema, table, window, cx));
                self.open_item(view.into(), key, title, cx);
            }
            DbEvent::OpenSql { profile } => {
                let Some(client) = self.live_client(profile, false, cx) else {
                    self.pending_db = Some(event.clone());
                    return;
                };
                self.sql_count += 1;
                let title: SharedString = format!("SQL {} · {profile}", self.sql_count).into();
                let view = cx.new(|cx| SqlView::new(title.clone(), client, window, cx));
                self.open_item(view.into(), title.clone(), title, cx);
            }
        }
    }

    fn flags(&self, cx: &App) -> Vec<TabFlags> {
        self.tabs
            .iter()
            .map(|t| TabFlags {
                pinned: t.pinned,
                dirty: matches!(&t.item, PaneItem::File(e) if e.read(cx).dirty),
            })
            .collect()
    }

    /// Closes tabs per `op` relative to `target`. Closing a single tab with
    /// unsaved changes asks first; bulk closes leave such tabs open.
    fn close(&mut self, op: CloseOp, target: usize, window: &mut Window, cx: &mut Context<Self>) {
        let closing = to_close(op, target, &self.flags(cx));
        if let [ix] = closing[..]
            && let Some(PaneItem::File(editor)) = self.tabs.get(ix).map(|t| t.item.clone())
            && editor.read(cx).dirty
        {
            self.confirm_close_dirty(editor, window, cx);
            return;
        }
        self.remove_tabs(&closing, cx);
    }

    fn remove_tabs(&mut self, closing: &[usize], cx: &mut Context<Self>) {
        if closing.is_empty() {
            return;
        }
        let len = self.tabs.len();
        self.active = active_after(self.active, closing, len).unwrap_or(0);
        let mut ix = 0;
        self.tabs.retain(|_| {
            let keep = !closing.contains(&ix);
            ix += 1;
            keep
        });
        cx.notify();
    }

    fn confirm_close_dirty(
        &mut self,
        editor: Entity<FileEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = editor.read(cx).title();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Simpan perubahan pada {name}?"),
            Some("Perubahan akan hilang jika tidak disimpan."),
            &["Simpan", "Jangan simpan", "Batal"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            let close = match answer.await {
                Ok(0) => editor.update(cx, |e, cx| e.save_task(cx)).await,
                Ok(1) => true,
                _ => false,
            };
            if close {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this
                        .tabs
                        .iter()
                        .position(|t| matches!(&t.item, PaneItem::File(e) if *e == editor))
                    {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }

    fn toggle_pin(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return;
        };
        let active_is_target = self.active == ix;
        let active_item = self.active;
        self.tabs.remove(ix);
        let pinned_count = self.tabs.iter().take_while(|t| t.pinned).count();
        // Pinned tabs sit together at the start, like in Zed; a pinned tab
        // goes to the end of that group, an unpinned one just after it.
        let at = pinned_count;
        self.tabs.insert(
            at,
            Tab {
                pinned: !tab.pinned,
                ..tab
            },
        );
        if active_is_target {
            self.active = at;
        } else {
            // Shift the active index the same way the move shifted it.
            let mut a = active_item;
            if ix < a {
                a -= 1;
            }
            if at <= a {
                a += 1;
            }
            self.active = a;
        }
        cx.notify();
    }

    fn toggle_read_only(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(PaneItem::File(editor)) = self.tabs.get(ix).map(|t| t.item.clone()) {
            editor.update(cx, |e, cx| {
                let ro = !e.read_only;
                e.set_read_only(ro, cx)
            });
        }
    }

    fn tab_menu(
        this: WeakEntity<Self>,
        ix: usize,
        is_file: bool,
        read_only: bool,
        pinned: bool,
        menu: PopupMenu,
        cx: &App,
    ) -> PopupMenu {
        let muted = cx.theme().muted_foreground;
        let item = |label: &'static str, keys: &'static str| {
            PopupMenuItem::element(move |_, _| {
                h_flex()
                    .w_full()
                    .gap_6()
                    .justify_between()
                    .child(label)
                    .child(div().text_color(muted).child(keys))
            })
        };
        let close = |op: CloseOp| {
            let this = this.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |ws, cx| ws.close(op, ix, window, cx));
            }
        };
        let this_pin = this.clone();
        let this_ro = this.clone();
        menu.item(item("Close", "⌘W").on_click(close(CloseOp::This)))
            .item(item("Close Others", "⌥⌘T").on_click(close(CloseOp::Others)))
            .separator()
            .item(item("Close Left", "⌘K E").on_click(close(CloseOp::Left)))
            .item(item("Close Right", "⌘K T").on_click(close(CloseOp::Right)))
            .separator()
            .item(item("Close Clean", "⌘K U").on_click(close(CloseOp::Clean)))
            .item(item("Close All", "⌘K W").on_click(close(CloseOp::All)))
            .when(is_file, |menu| {
                menu.separator().item(
                    PopupMenuItem::new(if read_only {
                        "Make Tab Editable"
                    } else {
                        "Make Tab Read-Only"
                    })
                    .on_click(move |_, _, cx| {
                        let _ = this_ro.update(cx, |ws, cx| ws.toggle_read_only(ix, cx));
                    }),
                )
            })
            .separator()
            .item(
                item(if pinned { "Unpin Tab" } else { "Pin Tab" }, "⌘K ⇧↩").on_click(
                    move |_, _, cx| {
                        let _ = this_pin.update(cx, |ws, cx| ws.toggle_pin(ix, cx));
                    },
                ),
            )
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
        let weak = cx.entity().downgrade();
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let active = ix == self.active;
                let pinned = tab.pinned;
                let (icon, title, dirty, read_only) = match &tab.item {
                    PaneItem::Server => {
                        (Icon::new(Lucide::Server), "Server".to_string(), false, None)
                    }
                    PaneItem::File(e) => {
                        let e = e.read(cx);
                        (
                            Icon::new(Lucide::FileCode),
                            e.title(),
                            e.dirty,
                            Some(e.read_only),
                        )
                    }
                    PaneItem::Other { title, .. } => {
                        (Icon::new(Lucide::Table), title.to_string(), false, None)
                    }
                };
                let weak = weak.clone();
                h_flex()
                    .id(("pane-tab", ix))
                    .h_full()
                    .pl_3()
                    .pr_1()
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
                    .when(read_only == Some(true), |el| {
                        el.child(
                            Icon::new(Lucide::Lock)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .when(dirty, |el| {
                        el.child(div().size(px(6.)).rounded_full().bg(cx.theme().foreground))
                    })
                    .child(if pinned {
                        Button::new(("pin-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Pin)
                            .tooltip("Lepas pin")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_pin(ix, cx);
                            }))
                    } else {
                        Button::new(("close-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip("Tutup (⌘W)")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close(CloseOp::This, ix, window, cx);
                            }))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.active = ix;
                        cx.notify();
                    }))
                    .context_menu(move |menu, _, cx| {
                        Self::tab_menu(
                            weak.clone(),
                            ix,
                            read_only.is_some(),
                            read_only == Some(true),
                            pinned,
                            menu,
                            cx,
                        )
                    })
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

    fn render_active_item(&self, cx: &App) -> AnyElement {
        match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::File(editor)) => editor.clone().into_any_element(),
            Some(PaneItem::Other { view, .. }) => view.clone().into_any_element(),
            Some(PaneItem::Server) => self.server_detail.clone().into_any_element(),
            None => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Tidak ada tab yang terbuka.")
                .child("Pilih server, file, atau tabel di panel kiri.")
                .into_any_element(),
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

impl Workspace {
    fn toggle_area(&mut self, area: Area, cx: &mut Context<Self>) {
        self.layout.toggle(area);
        cx.notify();
    }

    fn render_layout_toggles(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let l = self.layout;
        let toggle = |id: &'static str, icon: Lucide, tooltip: &'static str, on: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tooltip)
                .selected(on)
        };
        h_flex()
            .gap_0p5()
            .child(
                toggle(
                    "toggle-sidebar",
                    Lucide::PanelLeft,
                    "Sidebar (⌘B)",
                    l.sidebar,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_area(Area::Sidebar, cx))),
            )
            .child(
                toggle(
                    "toggle-center",
                    Lucide::PanelTop,
                    "Tab & tabel (⇧⌘J)",
                    l.center,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_area(Area::Center, cx))),
            )
            .child(
                toggle(
                    "toggle-terminal",
                    Lucide::PanelBottom,
                    "Terminal / herdr (⌘J)",
                    l.terminal,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_area(Area::Terminal, cx))),
            )
    }

    fn render_center(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .size_full()
            .child(self.render_tab_bar(cx))
            .child(div().flex_1().min_h_0().child(self.render_active_item(cx)))
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let l = self.layout;
        // The terminal stays alive while hidden; only its view is dropped.
        let main = match (l.center, l.terminal) {
            (true, true) => v_resizable("workspace-v")
                .child(resizable_panel().child(self.render_center(cx)))
                .child(
                    resizable_panel()
                        .size(px(240.))
                        .size_range(px(80.)..Pixels::MAX)
                        .child(self.terminal.clone()),
                )
                .into_any_element(),
            (true, false) => self.render_center(cx).into_any_element(),
            _ => self.terminal.clone().into_any_element(),
        };
        if !l.sidebar {
            return main;
        }
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
                            .child(div().flex_1().min_h_0().child(self.render_sidebar())),
                    ),
            )
            .child(resizable_panel().child(main))
            .into_any_element()
    }

    fn on_close_action(&mut self, op: CloseOp, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.active;
        self.close(op, target, window, cx);
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if window.focused(cx).is_none() {
            // Keep workspace shortcuts working when nothing inside has focus.
            self.focus.focus(window, cx);
        }
        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &CloseActiveTab, w, cx| {
                this.on_close_action(CloseOp::This, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseOtherTabs, w, cx| {
                this.on_close_action(CloseOp::Others, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseTabsLeft, w, cx| {
                this.on_close_action(CloseOp::Left, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseTabsRight, w, cx| {
                this.on_close_action(CloseOp::Right, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseCleanTabs, w, cx| {
                this.on_close_action(CloseOp::Clean, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseAllTabs, w, cx| {
                this.on_close_action(CloseOp::All, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_area(Area::Sidebar, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, _, cx| this.toggle_area(Area::Terminal, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleCenter, _, cx| this.toggle_area(Area::Center, cx)),
            )
            .on_action(cx.listener(|this, _: &TogglePinTab, _, cx| {
                let ix = this.active;
                this.toggle_pin(ix, cx)
            }))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .size_full()
                        .pr_2()
                        .gap_2()
                        .text_sm()
                        .child(img(crate::assets::LOGO_MARK).size(px(16.)))
                        .child(div().font_semibold().child("PG Desk"))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("Postgres · Editor · Terminal"),
                        )
                        .child(div().flex_1())
                        .child(self.render_layout_toggles(cx)),
                ),
            )
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
            .child(self.render_status_bar(cx))
    }
}
