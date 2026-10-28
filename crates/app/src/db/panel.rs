//! Database sidebar: connections (from a URL, a project's .env, or a PG Desk
//! instance) and their schema → table/view → column tree.

use std::collections::HashSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::db::{ConnProfile, RelKind};

use super::store::DbStore;
use crate::servers::ServerStore;

#[derive(Clone)]
pub enum DbEvent {
    OpenTable {
        profile: String,
        schema: String,
        table: String,
    },
    OpenSql {
        profile: String,
    },
}

impl EventEmitter<DbEvent> for DatabasePanel {}

pub struct DatabasePanel {
    store: Entity<DbStore>,
    servers: Entity<ServerStore>,
    url_input: Entity<InputState>,
    show_url: bool,
    expanded: HashSet<String>,
    scroll: UniformListScrollHandle,
    error: Option<String>,
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
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
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
            expanded: HashSet::new(),
            scroll: UniformListScrollHandle::new(),
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn add_url(&mut self, url: String, cx: &mut Context<Self>) {
        match self.store.update(cx, |s, cx| s.add_url(&url, cx)) {
            Ok(name) => {
                self.show_url = false;
                self.error = None;
                self.expanded.insert(name);
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn import_env(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose .env File".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match this.store.update(cx, |s, cx| s.import_env(path, cx)) {
                    Ok(name) => {
                        this.error = None;
                        this.expanded.insert(name);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Adds a profile for every running PG Desk instance.
    fn add_instances(&mut self, cx: &mut Context<Self>) {
        let servers = self.servers.read(cx);
        let running: Vec<_> = servers
            .registry
            .instances
            .iter()
            .filter(|i| {
                matches!(
                    servers.status(&i.name),
                    crate::servers::Status::Known(pgdesk_core::pgctl::PgCtlStatus::Running { .. })
                )
            })
            .map(|i| ConnProfile {
                name: format!("{} (PG Desk)", i.name),
                ..ConnProfile::new("localhost", i.port, "postgres", "postgres")
            })
            .collect();
        if running.is_empty() {
            self.error = Some("No PG Desk instances are running.".into());
        } else {
            self.error = None;
            for p in running {
                self.expanded.insert(p.name.clone());
                self.store.update(cx, |s, cx| s.add_profile(p, cx));
            }
        }
        cx.notify();
    }

    fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        cx.notify();
    }
}

/// One visible line of the connection tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Profile {
        name: String,
        connected: bool,
        connecting: bool,
    },
    Error {
        message: String,
    },
    Schema {
        key: String,
        name: String,
        open: bool,
        count: usize,
    },
    Relation {
        key: String,
        profile: String,
        schema: String,
        name: String,
        kind: RelKind,
        open: bool,
        readable: bool,
    },
    Column {
        key: String,
        name: String,
        detail: String,
    },
}

/// Flattens the tree into the lines currently visible. `public` schemas
/// start open; toggling a key flips its default.
pub fn flatten(store: &DbStore, expanded: &HashSet<String>) -> Vec<Node> {
    let mut out = Vec::new();
    for profile in &store.profiles.items {
        let name = profile.name.clone();
        out.push(Node::Profile {
            name: name.clone(),
            connected: store.is_connected(&name),
            connecting: store.connecting.contains(&name),
        });
        if let Some(e) = store.errors.get(&name) {
            out.push(Node::Error { message: e.clone() });
        }
        if !expanded.contains(&name) {
            continue;
        }
        let Some(schemas) = store.schemas.get(&name) else {
            continue;
        };
        for schema in schemas {
            let skey = format!("{name}\u{0}{}", schema.name);
            let sopen = (schema.name == "public") != expanded.contains(&skey);
            out.push(Node::Schema {
                key: skey.clone(),
                name: schema.name.clone(),
                open: sopen,
                count: schema.relations.len(),
            });
            if !sopen {
                continue;
            }
            for rel in &schema.relations {
                let rkey = format!("{skey}\u{0}{}", rel.name);
                // Columns of a relation the role cannot read stay hidden too.
                let ropen = rel.readable && expanded.contains(&rkey);
                out.push(Node::Relation {
                    key: rkey.clone(),
                    profile: name.clone(),
                    schema: schema.name.clone(),
                    name: rel.name.clone(),
                    kind: rel.kind,
                    open: ropen,
                    readable: rel.readable,
                });
                if ropen {
                    for (cix, col) in rel.columns.iter().enumerate() {
                        out.push(Node::Column {
                            key: format!("{rkey}\u{0}{cix}"),
                            name: col.name.clone(),
                            detail: format!(
                                "{}{}",
                                col.data_type,
                                if col.nullable { "" } else { " not null" }
                            ),
                        });
                    }
                }
            }
        }
    }
    out
}

/// Height of every tree line (the list is virtual, so lines are uniform).
const ROW_HEIGHT: f32 = 22.;

fn row(id: impl Into<ElementId>, depth: usize, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .h(px(ROW_HEIGHT))
        .pl(px(8. + depth as f32 * 12.))
        .pr_1()
        .gap_1p5()
        .text_sm()
        .cursor_pointer()
        .hover(|el| el.bg(cx.theme().list_hover))
}

impl DatabasePanel {
    fn render_node(&self, ix: usize, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match node {
            Node::Profile {
                name,
                connected,
                connecting,
            } => {
                let color = if *connected {
                    cx.theme().success
                } else {
                    muted
                };
                let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name.clone());
                row(("db-profile", ix), 0, cx)
                    .child(div().size(px(7.)).rounded_full().bg(color))
                    .child(div().flex_1().truncate().child(name.clone()))
                    .when(*connecting, |el| {
                        el.child(div().text_xs().text_color(muted).child("…"))
                    })
                    .when(*connected, |el| {
                        el.child(
                            Button::new(("db-sql", ix))
                                .ghost()
                                .xsmall()
                                .label("SQL")
                                .tooltip("New SQL Editor")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.emit(DbEvent::OpenSql {
                                        profile: n1.clone(),
                                    });
                                })),
                        )
                        .child(
                            Button::new(("db-refresh", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::RotateCw)
                                .tooltip("Reload Schema")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.store.update(cx, |s, cx| s.refresh(&n2, cx)).detach();
                                })),
                        )
                    })
                    .child(
                        Button::new(("db-remove", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip("Remove Connection (also deletes its password from Keychain)")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.store.update(cx, |s, cx| s.remove(&n3, cx))
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.store.read(cx).is_connected(&n4) {
                            this.toggle(n4.clone(), cx);
                        } else {
                            this.store.update(cx, |s, cx| s.connect(&n4, cx));
                            this.expanded.insert(n4.clone());
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            }
            Node::Error { message } => {
                let full: SharedString = message.clone().into();
                // One line (the list is uniform); the whole text is in the tooltip.
                div()
                    .id(("db-error", ix))
                    .h(px(ROW_HEIGHT))
                    .pl(px(20.))
                    .pr_2()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .truncate()
                    .child(message.lines().next().unwrap_or_default().to_string())
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                    })
                    .into_any_element()
            }
            Node::Schema {
                key,
                name,
                open,
                count,
            } => {
                let k = key.clone();
                row(
                    ElementId::Name(format!("db-schema\u{0}{key}").into()),
                    1,
                    cx,
                )
                .child(
                    Icon::new(if *open {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    })
                    .xsmall(),
                )
                .child(div().truncate().child(name.clone()))
                .child(div().text_xs().text_color(muted).child(count.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(k.clone(), cx)))
                .into_any_element()
            }
            Node::Relation {
                key,
                profile,
                schema,
                name,
                kind,
                open,
                readable,
            } => {
                let id = |part: &str| ElementId::Name(format!("{part}\u{0}{key}").into());
                let icon = match kind {
                    RelKind::Table | RelKind::Foreign => Icon::new(Lucide::Table),
                    RelKind::View | RelKind::MaterializedView => Icon::new(Lucide::Eye),
                };
                if !readable {
                    // Disabled: nothing to open, the tooltip says why.
                    return h_flex()
                        .id(id("db-rel"))
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .pl(px(8. + 2. * 12.))
                        .pr_1()
                        .gap_1p5()
                        .text_sm()
                        .text_color(muted)
                        .opacity(0.6)
                        .cursor_not_allowed()
                        .child(div().w(px(16.)))
                        .child(icon.xsmall())
                        .child(div().flex_1().truncate().child(name.clone()))
                        .child(Icon::new(Lucide::Lock).xsmall())
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "This role has no SELECT permission",
                            )
                            .build(window, cx)
                        })
                        .into_any_element();
                }
                let toggle_key = key.clone();
                let (profile, schema, table) = (profile.clone(), schema.clone(), name.clone());
                row(id("db-rel"), 2, cx)
                    .child(
                        Button::new(id("db-rel-toggle"))
                            .ghost()
                            .xsmall()
                            .icon(if *open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle(toggle_key.clone(), cx);
                            })),
                    )
                    .child(icon.xsmall().text_color(muted))
                    .child(div().truncate().child(name.clone()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(DbEvent::OpenTable {
                            profile: profile.clone(),
                            schema: schema.clone(),
                            table: table.clone(),
                        });
                    }))
                    .into_any_element()
            }
            Node::Column { key, name, detail } => {
                row(ElementId::Name(format!("db-col\u{0}{key}").into()), 4, cx)
                    .child(div().truncate().child(name.clone()))
                    .child(div().text_xs().text_color(muted).child(detail.clone()))
                    .into_any_element()
            }
        }
    }
}

impl Render for DatabasePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let node_count = flatten(store, &self.expanded).len();
        let empty = store.profiles.items.is_empty();

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
                    .child(
                        Button::new("db-add-url")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("Add from DATABASE_URL")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_url = !this.show_url;
                                if this.show_url {
                                    this.url_input.update(cx, |s, cx| s.focus(window, cx));
                                }
                                cx.notify();
                            })),
                    )
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
                            .tooltip("Connect Running PG Desk Instances")
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
            .when_some(self.error.clone(), |el, e| {
                el.child(div().px_2().py_1().text_xs().text_color(cx.theme().danger).whitespace_normal().child(e))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .when(!empty, |el| {
                        // Virtual: only visible lines are built, so schemas
                        // with thousands of tables stay cheap.
                        el.child(
                            uniform_list(
                                "db-tree",
                                node_count,
                                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    let nodes = flatten(this.store.read(cx), &this.expanded);
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
                    .when(empty, |el| {
                        el.child(
                            div()
                                .p_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No connections yet. Add one from a DATABASE_URL, a .env file, or a running PG Desk instance."),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
    use super::{Node, flatten};
    use crate::db::store::DbStore;
    use pgdesk_core::db::{ColumnInfo, ConnProfile, Profiles, RelKind, Relation, SchemaInfo};
    use std::collections::HashSet;

    fn rel(name: &str, readable: bool) -> Relation {
        Relation {
            name: name.into(),
            kind: RelKind::Table,
            columns: vec![ColumnInfo {
                name: "id".into(),
                data_type: "int".into(),
                nullable: false,
            }],
            readable,
        }
    }

    fn store() -> (DbStore, String) {
        let p = ConnProfile::new("h", 5432, "app", "u");
        let name = p.name.clone();
        let mut store = DbStore::detached(Profiles { items: vec![p] });
        store.schemas.insert(
            name.clone(),
            vec![
                SchemaInfo {
                    name: "audit".into(),
                    relations: vec![rel("log", true)],
                },
                SchemaInfo {
                    name: "public".into(),
                    relations: vec![rel("users", true), rel("secret", false)],
                },
            ],
        );
        (store, name)
    }

    fn names(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Profile { name, .. } => format!("p:{name}"),
                Node::Error { .. } => "error".into(),
                Node::Schema { name, .. } => format!("s:{name}"),
                Node::Relation { name, .. } => format!("r:{name}"),
                Node::Column { name, .. } => format!("c:{name}"),
            })
            .collect()
    }

    #[test]
    fn collapsed_profile_shows_only_itself() {
        let (store, name) = store();
        assert_eq!(
            names(&flatten(&store, &HashSet::new())),
            vec![format!("p:{name}")]
        );
    }

    #[test]
    fn public_starts_open_other_schemas_closed() {
        let (store, name) = store();
        let expanded = HashSet::from([name.clone()]);
        assert_eq!(
            names(&flatten(&store, &expanded)),
            vec![
                format!("p:{name}"),
                "s:audit".into(),
                "s:public".into(),
                "r:users".into(),
                "r:secret".into(),
            ]
        );
    }

    #[test]
    fn unreadable_relations_are_flagged_and_never_expand() {
        let (store, name) = store();
        let secret = format!("{name}\u{0}public\u{0}secret");
        let users = format!("{name}\u{0}public\u{0}users");
        let expanded = HashSet::from([name.clone(), secret, users]);
        let nodes = flatten(&store, &expanded);
        assert!(nodes.iter().any(|n| matches!(
            n,
            Node::Relation { name, readable: false, open: false, .. } if name == "secret"
        )));
        // Only `users` contributes a column line.
        assert_eq!(
            nodes
                .iter()
                .filter(|n| matches!(n, Node::Column { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn errors_get_their_own_line() {
        let (mut store, name) = store();
        store.errors.insert(name.clone(), "FATAL: no\nmore".into());
        assert_eq!(names(&flatten(&store, &HashSet::new()))[1], "error");
    }
}
