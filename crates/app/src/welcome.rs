//! The welcome screen a new, empty window opens on, like Zed's: start
//! actions on the left, recent folders, servers and databases on the right.
//! Picking anything hands over to the workspace, which lays itself out for
//! that choice.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::recent::{RecentProjects, tildify};
use sawit_core::update::Version;

use crate::db::DbStore;
use crate::servers::{ServerStore, status_color};

/// Rows shown per list before "N more".
const MAX_ROWS: usize = 6;

pub enum WelcomeEvent {
    OpenFolder,
    OpenRecent(PathBuf),
    NewInstance,
    OpenServer(String),
    OpenDatabase(String),
    OpenTerminal,
    /// "Skip": the plain workspace.
    Dismiss,
}

impl EventEmitter<WelcomeEvent> for Welcome {}

pub struct Welcome {
    servers: Entity<ServerStore>,
    db: Entity<DbStore>,
    recent: RecentProjects,
    recent_file: PathBuf,
    home: Option<PathBuf>,
    _subscriptions: Vec<Subscription>,
}

impl Welcome {
    pub fn new(servers: Entity<ServerStore>, db: Entity<DbStore>, cx: &mut Context<Self>) -> Self {
        let recent_file = AppPaths::default_macos().recent_file();
        let recent = RecentProjects::load(&recent_file).unwrap_or_default();
        Self {
            _subscriptions: vec![
                cx.observe(&servers, |_, _, cx| cx.notify()),
                cx.observe(&db, |_, _, cx| cx.notify()),
            ],
            servers,
            db,
            recent,
            recent_file,
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    fn forget(&mut self, dir: &Path, cx: &mut Context<Self>) {
        self.recent = RecentProjects::load(&self.recent_file).unwrap_or_default();
        self.recent.remove(dir);
        if let Err(e) = self.recent.save(&self.recent_file) {
            eprintln!("recent: {e}");
        }
        cx.notify();
    }

    fn action(
        &self,
        id: &'static str,
        icon: Lucide,
        label: &'static str,
        hint: SharedString,
        event: impl Fn() -> WelcomeEvent + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        h_flex()
            .id(id)
            .w_full()
            .h(px(32.))
            .px_2()
            .gap_2()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .hover(|el| el.bg(cx.theme().list_hover))
            .child(
                Icon::new(icon)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().flex_1().text_sm().child(label))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(event())))
    }

    fn section(title: &'static str, cx: &App) -> Div {
        v_flex().gap_1().child(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(title),
        )
    }

    fn row(id: SharedString, cx: &App) -> Stateful<Div> {
        h_flex()
            .id(id)
            .w_full()
            .h(px(28.))
            .px_2()
            .gap_2()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .hover(|el| el.bg(cx.theme().list_hover))
    }

    fn more(hidden: usize, cx: &App) -> Option<Div> {
        (hidden > 0).then(|| {
            div()
                .px_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{hidden} more"))
        })
    }

    fn empty(text: &'static str, cx: &App) -> Div {
        div()
            .px_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    }

    fn render_recent(&self, cx: &mut Context<Self>) -> Div {
        let folders = self.recent.existing();
        let hidden = folders.len().saturating_sub(MAX_ROWS);
        let rows: Vec<_> = folders
            .into_iter()
            .take(MAX_ROWS)
            .enumerate()
            .map(|(ix, dir)| {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string());
                let parent = dir
                    .parent()
                    .map(|p| tildify(p, self.home.as_deref()))
                    .unwrap_or_default();
                let (open, forget) = (dir.clone(), dir);
                Self::row(SharedString::from(format!("recent-{ix}")), cx)
                    .group("recent-row")
                    .child(
                        Icon::new(Lucide::Folder)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().flex_none().text_sm().child(name))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(parent),
                    )
                    .child(
                        div()
                            .invisible()
                            .group_hover("recent-row", |el| el.visible())
                            .child(
                                Button::new(("forget-recent", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::X)
                                    .tooltip("Remove from Recent")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.forget(&forget, cx);
                                    })),
                            ),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(WelcomeEvent::OpenRecent(open.clone()))
                    }))
            })
            .collect();
        let empty = rows.is_empty();
        Self::section("RECENT FOLDERS", cx)
            .children(rows)
            .children(Self::more(hidden, cx))
            .when(empty, |el| {
                el.child(Self::empty("Folders you open will show up here.", cx))
            })
    }

    fn render_servers(&self, cx: &mut Context<Self>) -> Div {
        let store = self.servers.read(cx);
        let instances = &store.registry.instances;
        let hidden = instances.len().saturating_sub(MAX_ROWS);
        let rows: Vec<_> = instances
            .iter()
            .take(MAX_ROWS)
            .map(|inst| {
                let name = inst.name.clone();
                Self::row(
                    SharedString::from(format!("welcome-inst-{}", inst.name)),
                    cx,
                )
                .child(
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(status_color(store.status(&inst.name), cx)),
                )
                .child(div().flex_1().truncate().text_sm().child(inst.name.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(":{}", inst.port)),
                )
                .on_click(
                    cx.listener(move |_, _, _, cx| cx.emit(WelcomeEvent::OpenServer(name.clone()))),
                )
            })
            .collect();
        let empty = rows.is_empty();
        Self::section("POSTGRESQL INSTANCES", cx)
            .children(rows)
            .children(Self::more(hidden, cx))
            .when(empty, |el| el.child(Self::empty("No instances yet.", cx)))
    }

    fn render_databases(&self, cx: &mut Context<Self>) -> Option<Div> {
        let profiles = &self.db.read(cx).profiles.items;
        if profiles.is_empty() {
            return None;
        }
        let hidden = profiles.len().saturating_sub(MAX_ROWS);
        let rows: Vec<_> = profiles
            .iter()
            .take(MAX_ROWS)
            .map(|p| {
                let name = p.name.clone();
                Self::row(SharedString::from(format!("welcome-db-{}", p.name)), cx)
                    .child(
                        Icon::new(Lucide::Database)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_sm()
                            .child(p.display_name().to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}:{}", p.host, p.port)),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(WelcomeEvent::OpenDatabase(name.clone()))
                    }))
            })
            .collect();
        Some(
            Self::section("DATABASE CONNECTIONS", cx)
                .children(rows)
                .children(Self::more(hidden, cx)),
        )
    }
}

impl Render for Welcome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let keys = |action: &dyn Action| crate::keymap::keys_for(action, "Workspace", window);
        let start = Self::section("START", cx)
            .child(self.action(
                "welcome-open-folder",
                Lucide::FolderOpen,
                "Open Folder…",
                keys(&crate::cli::OpenFolder),
                || WelcomeEvent::OpenFolder,
                cx,
            ))
            .child(self.action(
                "welcome-new-instance",
                Lucide::Plus,
                "New PostgreSQL Instance…",
                SharedString::default(),
                || WelcomeEvent::NewInstance,
                cx,
            ))
            .child(self.action(
                "welcome-terminal",
                Lucide::SquareTerminal,
                "Open Terminal",
                keys(&crate::workspace::ToggleTerminal),
                || WelcomeEvent::OpenTerminal,
                cx,
            ));
        let right = v_flex()
            .flex_1()
            .min_w(px(280.))
            .gap_6()
            .child(self.render_recent(cx))
            .child(self.render_servers(cx))
            .children(self.render_databases(cx));

        div()
            .id("welcome")
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .w_full()
                    .min_h_full()
                    .items_center()
                    .justify_center()
                    .py_12()
                    .px_6()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(720.))
                            .gap_10()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(img(crate::assets::LOGO_MARK).size(px(40.)))
                                    .child(
                                        v_flex()
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_baseline()
                                                    .child(
                                                        div()
                                                            .text_xl()
                                                            .font_semibold()
                                                            .child("Sawit"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(format!(
                                                                "v{}",
                                                                Version::current()
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("Your local dev workspace"),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_start()
                                    .flex_wrap()
                                    .gap_10()
                                    .child(v_flex().w(px(260.)).flex_none().child(start))
                                    .child(right),
                            )
                            .child(
                                h_flex().child(
                                    Button::new("welcome-skip")
                                        .ghost()
                                        .small()
                                        .label("Continue Without a Project")
                                        .icon(Lucide::ArrowRight)
                                        .on_click(cx.listener(|_, _, _, cx| {
                                            cx.emit(WelcomeEvent::Dismiss)
                                        })),
                                ),
                            ),
                    ),
            )
    }
}
