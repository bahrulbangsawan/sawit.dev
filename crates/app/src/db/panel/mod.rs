//! Database sidebar: connections (from a URL, a project's .env, or a Sawit
//! instance) and their schema → tables / views / … → column, index and
//! constraint tree, with a table-name filter and per-connection settings.
//!
//! Right-clicking a connection, schema or relation offers what to do with
//! it: open, query, export its rows, back it up with `pg_dump`, import a CSV
//! file, empty or delete it. Those run in the background and report here.
//!
//! Each connection row shows its kind's logo beside the status dot, and says
//! the kind in its tooltip and accessible name ("PostgreSQL 16 · kyzn"). The
//! D1 databases of the window's Workers projects follow the saved
//! connections under "Cloudflare D1" (local first, then remote), and local D1
//! files no binding names under "Unmatched local D1 databases". What only
//! PostgreSQL has (`pg_dump` backups, CSV imports, materialized views,
//! connection strings) isn't offered for SQLite or D1.

mod add;
mod jobs;
mod menus;
mod rows;
mod settings;
mod tree;

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::WriteRoute;

use super::store::DbStore;
use crate::servers::ServerStore;
use crate::states::{Banner, Placeholder};
use settings::SettingsForm;
use tree::{Node, TreeCache};

#[derive(Clone)]
#[allow(clippy::enum_variant_names)] // Each one opens a tab.
pub enum DbEvent {
    OpenTable {
        profile: String,
        schema: String,
        table: String,
    },
    /// A SQL editor tab, optionally starting with `sql`.
    OpenSql {
        profile: String,
        sql: Option<String>,
    },
    /// The schema's tables and foreign keys as a diagram (and DBML).
    OpenDiagram { profile: String, schema: String },
}

impl DbEvent {
    pub fn profile(&self) -> &str {
        match self {
            DbEvent::OpenTable { profile, .. }
            | DbEvent::OpenSql { profile, .. }
            | DbEvent::OpenDiagram { profile, .. } => profile,
        }
    }
}

impl EventEmitter<DbEvent> for DatabasePanel {}

pub struct DatabasePanel {
    store: Entity<DbStore>,
    servers: Entity<ServerStore>,
    url_input: Entity<InputState>,
    show_url: bool,
    /// Filters relations by name.
    search: Entity<InputState>,
    /// The connection whose settings are open.
    settings: Option<SettingsForm>,
    expanded: HashSet<String>,
    /// The tree lines for the virtual list.
    nodes: Rc<Vec<Node>>,
    /// Flattens them again only when what they show changed.
    tree: TreeCache,
    scroll: UniformListScrollHandle,
    error: Option<String>,
    /// Background jobs (backups, exports, imports, drops) still running.
    jobs: Vec<String>,
    /// How the last job ended; `true` for a failure.
    notice: Option<(bool, String)>,
    _subscriptions: Vec<Subscription>,
}

impl DatabasePanel {
    pub fn new(
        store: Entity<DbStore>,
        servers: Entity<ServerStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("postgres://user:pass@localhost:5432/db")
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables"));
        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| {
                this.tree.clear();
                cx.notify();
            }),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&url_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let url = input.read(cx).value().to_string();
                    this.add_url(url, cx);
                }
            }),
        ];
        Self {
            store,
            servers,
            url_input,
            show_url: false,
            search,
            settings: None,
            expanded: HashSet::new(),
            nodes: Rc::default(),
            tree: TreeCache::default(),
            scroll: UniformListScrollHandle::new(),
            error: None,
            jobs: Vec::new(),
            notice: None,
            _subscriptions: subscriptions,
        }
    }

    fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        cx.notify();
    }
}

/// Where SQLite and D1 changes go, as a badge: through the dev server's Local Explorer,
/// straight to the file, blocked, or remote (production) in the warning colour. `None` for
/// PostgreSQL (no route to tell).
pub(super) fn route_badge(
    id: &'static str,
    route: Option<&WriteRoute>,
    cx: &App,
) -> Option<AnyElement> {
    let route = route?;
    let (icon, colour, bg) = match route {
        WriteRoute::Remote { .. } => (
            Lucide::TriangleAlert,
            cx.theme().warning_foreground,
            cx.theme().warning,
        ),
        WriteRoute::Busy { .. } => (
            Lucide::Lock,
            cx.theme().danger_foreground,
            cx.theme().danger,
        ),
        WriteRoute::Explorer { .. } => (
            Lucide::Zap,
            cx.theme().secondary_foreground,
            cx.theme().secondary,
        ),
        WriteRoute::File(_) => (
            Lucide::File,
            cx.theme().secondary_foreground,
            cx.theme().secondary,
        ),
    };
    let tip: SharedString = route.describe().into();
    let label = match route {
        WriteRoute::Remote { .. } => route.label(),
        _ => format!("writes {}", route.label()),
    };
    Some(
        h_flex()
            .id(id)
            .flex_none()
            .gap_1()
            .px_1p5()
            .rounded(cx.theme().radius)
            .text_xs()
            .bg(bg)
            .text_color(colour)
            .child(Icon::new(icon).xsmall())
            .child(label)
            .aria_label(tip.clone())
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .into_any_element(),
    )
}

impl Render for DatabasePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().trim().to_string();
        let store = self.store.read(cx);
        self.nodes = self.tree.lines(store, &self.expanded, &query);
        let node_count = self.nodes.len();
        let empty = store.profiles.items.is_empty() && store.detected.is_empty();
        // Something is loaded to search, and nothing in it matches.
        let no_match = !query.is_empty()
            && !store.schemas.is_empty()
            && !self
                .nodes
                .iter()
                .any(|n| matches!(n, Node::Relation { .. }));
        let settings = self.render_settings(cx);

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().flex_1().text_xs().font_semibold().text_color(cx.theme().muted_foreground).child("DATABASE"))
                    .child(self.add_menu(cx))
                    .child(
                        Button::new("db-add-env")
                            .ghost()
                            .xsmall()
                            .label(".env")
                            .tooltip("Import DATABASE_URL from a .env File")
                            .on_click(cx.listener(|this, _, _, cx| this.import_env(cx))),
                    )
                    .child(
                        Button::new("db-add-instances")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Server)
                            .tooltip("Connect Running Sawit Instances")
                            .on_click(cx.listener(|this, _, _, cx| this.add_instances(cx))),
                    ),
            )
            .when(self.show_url, |el| {
                el.child(
                    v_flex()
                        .p_2()
                        .gap_1()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(Input::new(&self.url_input).small())
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Press Enter to add. Passwords are stored in Keychain."),
                        ),
                )
            })
            .children(settings)
            .when(!empty, |el| {
                el.child(
                    div()
                        .px_2()
                        .py_1p5()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            Input::new(&self.search)
                                .small()
                                .prefix(Icon::new(IconName::Search).xsmall())
                                .cleanable(true),
                        ),
                )
            })
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("db-error", e).on_dismiss(cx.listener(|this, _, _, cx| {
                    this.error = None;
                    cx.notify();
                })))
            })
            .children(self.jobs.iter().enumerate().map(|(ix, job)| {
                Banner::info(("db-job", ix), format!("{job}…"))
            }))
            .when_some(self.notice.clone(), |el, (failed, message)| {
                let banner = if failed {
                    Banner::error("db-notice", message)
                } else {
                    Banner::info("db-notice", message)
                };
                el.child(banner.on_dismiss(cx.listener(|this, _, _, cx| {
                    this.notice = None;
                    cx.notify();
                })))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .when(no_match, |el| {
                        el.child(
                            Placeholder::new(IconName::Search, "No matching tables")
                                .compact()
                                .detail(format!(
                                    "No table or view in an open connection matches “{query}”."
                                ))
                                .action(
                                    Button::new("db-clear-search")
                                        .small()
                                        .label("Clear Filter")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.search.update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                ),
                        )
                    })
                    .when(!empty && !no_match, |el| {
                        // Virtual: only visible lines are built, so schemas
                        // with thousands of tables stay cheap.
                        el.child(
                            uniform_list(
                                "db-tree",
                                node_count,
                                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    let nodes = this.nodes.clone();
                                    range
                                        .filter_map(|ix| {
                                            nodes.get(ix).map(|n| this.render_node(ix, n, cx))
                                        })
                                        .collect()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    })
                    .when(empty && !self.show_url, |el| {
                        el.child(
                            Placeholder::new(Lucide::Database, "No connections yet")
                                .compact()
                                .detail("Connect with a DATABASE_URL, a .env file, a running Sawit instance, a SQLite file, or a Workers project's D1 databases.")
                                .action(
                                    Button::new("empty-db-url")
                                        .small()
                                        .primary()
                                        .label("Add URL")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.show_url = true;
                                            this.url_input.update(cx, |s, cx| s.focus(window, cx));
                                            cx.notify();
                                        })),
                                )
                                .action(
                                    Button::new("empty-db-env")
                                        .small()
                                        .label("Import .env")
                                        .on_click(cx.listener(|this, _, _, cx| this.import_env(cx))),
                                )
                                .action(
                                    Button::new("empty-db-instances")
                                        .small()
                                        .label("Connect Instances")
                                        .on_click(cx.listener(|this, _, _, cx| this.add_instances(cx))),
                                ),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests;
