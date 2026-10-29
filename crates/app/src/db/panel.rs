//! Database sidebar: connections (from a URL, a project's .env, or a Sawit
//! instance) and their schema → table/view → column, index and constraint
//! tree, with a table-name filter and per-connection settings.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{ConnProfile, RelKind, Relation};

use super::store::DbStore;
use crate::servers::ServerStore;
use crate::states::{Banner, Placeholder};

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
    /// The tree lines, flattened once per render for the virtual list.
    nodes: Rc<Vec<Node>>,
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
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables"));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
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

    /// Adds a profile for every running Sawit instance.
    fn add_instances(&mut self, cx: &mut Context<Self>) {
        let servers = self.servers.read(cx);
        let running: Vec<_> = servers
            .registry
            .instances
            .iter()
            .filter(|i| {
                matches!(
                    servers.status(&i.name),
                    crate::servers::Status::Known(sawit_core::pgctl::PgCtlStatus::Running { .. })
                )
            })
            .map(|i| ConnProfile {
                name: format!("{} (Sawit)", i.name),
                ..ConnProfile::new("localhost", i.port, "postgres", "postgres")
            })
            .collect();
        if running.is_empty() {
            self.error = Some("No Sawit instances are running. Start one, then try again.".into());
        } else {
            self.error = None;
            for p in running {
                self.expanded.insert(p.name.clone());
                self.store.update(cx, |s, cx| s.add_profile(p, cx));
            }
        }
        cx.notify();
    }

    fn open_settings(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.store.read(cx).profile(name).cloned() else {
            return;
        };
        let label = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(profile.name.clone())
                .default_value(profile.label.clone().unwrap_or_default())
        });
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("postgres://user@localhost:5432/db")
                .default_value(profile.connection_string())
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Leave empty to keep the saved one")
                .masked(true)
        });
        let subscriptions = [&label, &url, &password]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.save_settings(cx);
                    }
                })
            })
            .collect();
        label.update(cx, |s, cx| s.focus(window, cx));
        self.show_url = false;
        self.settings = Some(SettingsForm {
            profile: profile.name,
            label,
            url,
            password,
            error: None,
            confirm_remove: false,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.settings else {
            return;
        };
        let (name, label, url, password) = (
            form.profile.clone(),
            form.label.read(cx).value().to_string(),
            form.url.read(cx).value().to_string(),
            form.password.read(cx).value().to_string(),
        );
        let result = self.store.update(cx, |s, cx| {
            s.update_profile(&name, &label, &url, Some(password), cx)
        });
        match result {
            Ok(()) => self.settings = None,
            Err(e) => {
                if let Some(form) = &mut self.settings {
                    form.error = Some(e);
                }
            }
        }
        cx.notify();
    }

    /// The first click asks; the second removes.
    fn remove_from_settings(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.settings else {
            return;
        };
        if !form.confirm_remove {
            form.confirm_remove = true;
            cx.notify();
            return;
        }
        let name = form.profile.clone();
        self.settings = None;
        self.expanded.remove(&name);
        self.store.update(cx, |s, cx| s.remove(&name, cx));
        cx.notify();
    }

    fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        cx.notify();
    }
}

/// The Connection Settings form for one profile.
struct SettingsForm {
    profile: String,
    label: Entity<InputState>,
    url: Entity<InputState>,
    password: Entity<InputState>,
    error: Option<String>,
    /// "Remove" was clicked once; the next click removes.
    confirm_remove: bool,
    _subscriptions: Vec<Subscription>,
}

/// The collapsible groups under a table, after its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Indexes,
    Constraints,
}

impl Facet {
    fn label(self) -> &'static str {
        match self {
            Facet::Indexes => "Indexes",
            Facet::Constraints => "Constraints",
        }
    }
}

/// One visible line of the connection tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Profile {
        /// The profile's key.
        name: String,
        /// What to show: the custom label, else `name`.
        label: String,
        /// The connection string, for the tooltip.
        url: String,
        connected: bool,
        /// Connecting now or waiting to retry.
        pending: bool,
        /// Status when not connected ("idle", "retrying in 4s").
        status: Option<String>,
    },
    Error {
        message: String,
    },
    Schema {
        key: String,
        profile: String,
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
        /// Part of the primary key.
        primary: bool,
    },
    Group {
        key: String,
        facet: Facet,
        count: usize,
        open: bool,
    },
    /// An index or constraint; `definition` is its full SQL.
    Item {
        key: String,
        facet: Facet,
        name: String,
        detail: String,
        definition: String,
    },
}

/// The lines under an open relation: columns, then its indexes and
/// constraints as groups (tables and materialized views only).
fn relation_children(rkey: &str, rel: &Relation, expanded: &HashSet<String>, out: &mut Vec<Node>) {
    for (cix, col) in rel.columns.iter().enumerate() {
        out.push(Node::Column {
            key: format!("{rkey}\u{0}{cix}"),
            name: col.name.clone(),
            detail: format!(
                "{}{}",
                col.data_type,
                if col.nullable { "" } else { " not null" }
            ),
            primary: rel.primary_key.contains(&col.name),
        });
    }
    let facets: &[Facet] = match rel.kind {
        RelKind::Table => &[Facet::Indexes, Facet::Constraints],
        RelKind::MaterializedView => &[Facet::Indexes],
        RelKind::View | RelKind::Foreign => &[],
    };
    for &facet in facets {
        let gkey = format!("{rkey}\u{0}{}", facet.label());
        let count = match facet {
            Facet::Indexes => rel.indexes.len(),
            Facet::Constraints => rel.constraints.len(),
        };
        let open = count > 0 && expanded.contains(&gkey);
        out.push(Node::Group {
            key: gkey.clone(),
            facet,
            count,
            open,
        });
        if !open {
            continue;
        }
        match facet {
            Facet::Indexes => out.extend(rel.indexes.iter().map(|ix| Node::Item {
                key: format!("{gkey}\u{0}{}", ix.name),
                facet,
                name: ix.name.clone(),
                detail: match (ix.primary, ix.unique) {
                    (true, _) => format!("primary {}", ix.method),
                    (false, true) => format!("unique {}", ix.method),
                    _ => ix.method.clone(),
                },
                definition: ix.definition.clone(),
            })),
            Facet::Constraints => out.extend(rel.constraints.iter().map(|c| Node::Item {
                key: format!("{gkey}\u{0}{}", c.name),
                facet,
                name: c.name.clone(),
                detail: c.kind.label().into(),
                definition: c.definition.clone(),
            })),
        }
    }
}

/// Flattens the tree into the lines currently visible. `public` schemas
/// start open; toggling a key flips its default. A non-empty `query` keeps
/// only relations whose name contains it (any case), with their schemas
/// open.
pub fn flatten(store: &DbStore, expanded: &HashSet<String>, query: &str) -> Vec<Node> {
    let query = query.trim().to_lowercase();
    let mut out = Vec::new();
    for profile in &store.profiles.items {
        let name = profile.name.clone();
        let state = store.state(&name);
        out.push(Node::Profile {
            name: name.clone(),
            label: profile.display_name().to_string(),
            url: profile.connection_string(),
            connected: store.is_connected(&name),
            pending: state.is_pending(),
            status: state.label(std::time::Instant::now()),
        });
        if let Some(e) = state
            .error()
            .or(store.notices.get(&name).map(String::as_str))
        {
            out.push(Node::Error {
                message: e.to_string(),
            });
        }
        if !expanded.contains(&name) {
            continue;
        }
        let Some(schemas) = store.schemas.get(&name) else {
            continue;
        };
        for schema in schemas {
            let skey = format!("{name}\u{0}{}", schema.name);
            let matching: Vec<&Relation> = schema
                .relations
                .iter()
                .filter(|r| query.is_empty() || r.name.to_lowercase().contains(&query))
                .collect();
            if !query.is_empty() && matching.is_empty() {
                continue;
            }
            let sopen = !query.is_empty() || (schema.name == "public") != expanded.contains(&skey);
            out.push(Node::Schema {
                key: skey.clone(),
                profile: name.clone(),
                name: schema.name.clone(),
                open: sopen,
                count: matching.len(),
            });
            if !sopen {
                continue;
            }
            for rel in matching {
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
                    relation_children(&rkey, rel, expanded, &mut out);
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
                label,
                url,
                connected,
                pending,
                status,
            } => {
                let color = if *connected {
                    cx.theme().success
                } else if *pending {
                    cx.theme().warning
                } else {
                    muted
                };
                let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name.clone());
                let n5 = name.clone();
                // The full name and where it points, since both truncate.
                let tip: SharedString = if label == url {
                    label.clone().into()
                } else {
                    format!("{label}\n{url}").into()
                };
                row(("db-profile", ix), 0, cx)
                    .child(div().size(px(7.)).rounded_full().bg(color))
                    .child(
                        div()
                            .id(("db-profile-name", ix))
                            .flex_1()
                            .truncate()
                            .child(label.clone())
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                    .build(window, cx)
                            }),
                    )
                    .when_some(status.clone(), |el, status| {
                        el.child(div().text_xs().text_color(muted).child(status))
                    })
                    .child(if *connected || *pending {
                        Button::new(("db-disconnect", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Unplug)
                            .tooltip(if *pending {
                                "Stop Connecting"
                            } else {
                                "Disconnect"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.store.update(cx, |s, cx| s.disconnect(&n5, cx));
                            }))
                    } else {
                        Button::new(("db-connect", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Plug)
                            .tooltip("Connect")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.store.update(cx, |s, cx| s.connect(&n5, cx));
                                this.expanded.insert(n5.clone());
                                cx.notify();
                            }))
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
                                        sql: None,
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
                        Button::new(("db-settings", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Settings2)
                            .tooltip("Connection Settings")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_settings(&n3, window, cx);
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
                profile,
                name,
                open,
                count,
            } => {
                let k = key.clone();
                let (profile, schema) = (profile.clone(), name.clone());
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
                .child(div().flex_1())
                .child(
                    Button::new(ElementId::Name(format!("db-diagram\u{0}{key}").into()))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Waypoints)
                        .tooltip("Show Schema Diagram")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(DbEvent::OpenDiagram {
                                profile: profile.clone(),
                                schema: schema.clone(),
                            });
                        })),
                )
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
                    RelKind::View => Icon::new(Lucide::Eye),
                    RelKind::MaterializedView => Icon::new(Lucide::Layers),
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
                                "No SELECT permission for this role",
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
            Node::Column {
                key,
                name,
                detail,
                primary,
            } => row(ElementId::Name(format!("db-col\u{0}{key}").into()), 4, cx)
                .cursor_default()
                // The name keeps its room; the type truncates first.
                .child(div().flex_shrink_0().max_w(relative(0.7)).truncate().child(name.clone()))
                .when(*primary, |el| {
                    el.child(Icon::new(Lucide::KeyRound).xsmall().text_color(muted))
                })
                .child(div().min_w_0().text_xs().text_color(muted).truncate().child(detail.clone()))
                .into_any_element(),
            Node::Group {
                key,
                facet,
                count,
                open,
            } => {
                let k = key.clone();
                let empty = *count == 0;
                row(ElementId::Name(format!("db-group\u{0}{key}").into()), 4, cx)
                    .text_color(muted)
                    .child(
                        Icon::new(if *open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .when(empty, |i| i.invisible()),
                    )
                    .child(
                        Icon::new(match facet {
                            Facet::Indexes => Lucide::ListTree,
                            Facet::Constraints => Lucide::ShieldCheck,
                        })
                        .xsmall(),
                    )
                    .child(facet.label())
                    .child(div().text_xs().child(count.to_string()))
                    .when(empty, |el| el.cursor_default())
                    .when(!empty, |el| {
                        el.on_click(cx.listener(move |this, _, _, cx| this.toggle(k.clone(), cx)))
                    })
                    .into_any_element()
            }
            Node::Item {
                key,
                name,
                detail,
                definition,
                ..
            } => {
                let full: SharedString = definition.clone().into();
                row(ElementId::Name(format!("db-item\u{0}{key}").into()), 6, cx)
                    .cursor_default()
                    .child(div().truncate().child(name.clone()))
                    .child(div().text_xs().text_color(muted).flex_shrink_0().child(detail.clone()))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                    })
                    .into_any_element()
            }
        }
    }
}

impl DatabasePanel {
    fn render_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.settings.as_ref()?;
        let muted = cx.theme().muted_foreground;
        let title = self
            .store
            .read(cx)
            .profile(&form.profile)
            .map(|p| p.display_name().to_string())
            .unwrap_or_default();
        let field = |label: &'static str, input: &Entity<InputState>, hint: &'static str| {
            v_flex()
                .gap_1()
                .child(div().text_xs().font_medium().child(label))
                .child(Input::new(input).small())
                .child(div().text_xs().text_color(muted).child(hint))
        };
        let confirm = form.confirm_remove;
        Some(
            v_flex()
                .p_2()
                .gap_3()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex()
                        .child(div().text_sm().font_semibold().child("Connection Settings"))
                        .child(div().text_xs().text_color(muted).truncate().child(title)),
                )
                .child(field(
                    "Name",
                    &form.label,
                    "Shown in the sidebar. Leave empty to use the connection details.",
                ))
                .child(field(
                    "Connection string",
                    &form.url,
                    "Change the host, database or SSL settings here. Saving reconnects.",
                ))
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().font_medium().child("Password"))
                        .child(Input::new(&form.password).small().mask_toggle())
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("Stored in Keychain, never shown."),
                        ),
                )
                .when_some(form.error.clone(), |el, e| {
                    el.child(div().text_xs().text_color(cx.theme().danger).child(e))
                })
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("db-settings-save")
                                .small()
                                .primary()
                                .label("Save")
                                .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
                        )
                        .child(
                            Button::new("db-settings-cancel")
                                .small()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings = None;
                                    cx.notify();
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("db-settings-remove")
                                .small()
                                .danger()
                                .when(!confirm, |b| b.ghost())
                                .icon(Lucide::Trash)
                                .label(if confirm { "Confirm Remove" } else { "Remove" })
                                .tooltip("Removes the connection and its Keychain password")
                                .on_click(cx.listener(|this, _, _, cx| this.remove_from_settings(cx))),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Render for DatabasePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().trim().to_string();
        let store = self.store.read(cx);
        self.nodes = Rc::new(flatten(store, &self.expanded, &query));
        let node_count = self.nodes.len();
        let empty = store.profiles.items.is_empty();
        // Something is loaded to search, and nothing in it matches.
        let no_match = !query.is_empty()
            && !store.schemas.is_empty()
            && !self.nodes.iter().any(|n| matches!(n, Node::Relation { .. }));
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
                    .child(
                        Button::new("db-add-url")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("Add from DATABASE_URL")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.settings = None;
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
                                .detail("Connect with a DATABASE_URL, a .env file, or a running Sawit instance.")
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
mod tests {
    // Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
    use super::{Facet, Node, flatten};
    use crate::db::lifecycle::ConnState;
    use crate::db::store::DbStore;
    use sawit_core::db::FailureKind;
    use sawit_core::db::{
        ColumnInfo, ConnProfile, Constraint, ConstraintKind, IndexInfo, Profiles, RelKind,
        Relation, SchemaInfo,
    };
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

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
            ..Relation::default()
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
                Node::Group { facet, count, .. } => format!("g:{}:{count}", facet.label()),
                Node::Item { name, .. } => format!("i:{name}"),
            })
            .collect()
    }

    #[test]
    fn collapsed_profile_shows_only_itself() {
        let (store, name) = store();
        assert_eq!(
            names(&flatten(&store, &HashSet::new(), "")),
            vec![format!("p:{name}")]
        );
    }

    #[test]
    fn public_starts_open_other_schemas_closed() {
        let (store, name) = store();
        let expanded = HashSet::from([name.clone()]);
        assert_eq!(
            names(&flatten(&store, &expanded, "")),
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
        let nodes = flatten(&store, &expanded, "");
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
        store.set_state(
            &name,
            ConnState::Failed {
                error: "Check the user name and password.\nFATAL: no".into(),
                kind: FailureKind::Auth,
            },
        );
        assert_eq!(names(&flatten(&store, &HashSet::new(), ""))[1], "error");
        // Schema reload problems show the same way.
        store.set_state(&name, ConnState::Idle);
        store
            .notices
            .insert(name.clone(), "Unable to reload".into());
        assert_eq!(names(&flatten(&store, &HashSet::new(), ""))[1], "error");
    }

    #[test]
    fn each_connection_state_shows_its_status() {
        let (mut store, name) = store();
        let now = Instant::now();
        let profile_line = |store: &DbStore| match &flatten(store, &HashSet::new(), "")[0] {
            Node::Profile {
                connected,
                pending,
                status,
                ..
            } => (*connected, *pending, status.clone()),
            other => panic!("{other:?}"),
        };
        // Opening the app: every connection idle, nothing connecting.
        assert_eq!(profile_line(&store), (false, false, Some("idle".into())));
        store.set_state(&name, ConnState::Connecting { attempt: 0 });
        assert_eq!(
            profile_line(&store),
            (false, true, Some("connecting…".into()))
        );
        store.set_state(&name, ConnState::Connecting { attempt: 3 });
        assert_eq!(profile_line(&store).2.as_deref(), Some("reconnecting 3/8…"));
        store.set_state(
            &name,
            ConnState::Retrying {
                attempt: 2,
                retry_at: now + Duration::from_secs(4),
                error: "Connection lost".into(),
            },
        );
        let nodes = flatten(&store, &HashSet::new(), "");
        assert!(matches!(&nodes[0], Node::Profile { pending: true, .. }));
        assert!(matches!(&nodes[1], Node::Error { message } if message == "Connection lost"));
        store.set_state(&name, ConnState::Inactive);
        assert_eq!(
            profile_line(&store),
            (false, false, Some("inactive".into()))
        );
        store.set_state(&name, ConnState::Disconnected);
        assert_eq!(
            profile_line(&store),
            (false, false, Some("disconnected".into()))
        );
        // "Connected" with no live client (it just dropped) isn't shown as connected.
        store.set_state(&name, ConnState::Connected);
        assert_eq!(profile_line(&store), (false, false, None));
    }

    #[test]
    fn filter_keeps_matching_relations_and_opens_their_schemas() {
        let (store, name) = store();
        let expanded = HashSet::from([name.clone()]);
        // `audit` starts closed but opens for a match; `public` has none.
        assert_eq!(
            names(&flatten(&store, &expanded, " LO ")),
            vec![format!("p:{name}"), "s:audit".into(), "r:log".into()]
        );
        assert_eq!(
            names(&flatten(&store, &expanded, "zzz")),
            vec![format!("p:{name}")]
        );
    }

    #[test]
    fn tables_list_indexes_and_constraints_after_columns() {
        let (mut store, name) = store();
        let users = &mut store.schemas.get_mut(&name).unwrap()[1].relations[0];
        users.primary_key = vec!["id".into()];
        users.indexes = vec![IndexInfo {
            name: "users_pkey".into(),
            method: "btree".into(),
            unique: true,
            primary: true,
            definition: "CREATE UNIQUE INDEX users_pkey ON public.users USING btree (id)".into(),
        }];
        users.constraints = vec![Constraint {
            name: "users_pkey".into(),
            kind: ConstraintKind::PrimaryKey,
            definition: "PRIMARY KEY (id)".into(),
        }];
        let rkey = format!("{name}\u{0}public\u{0}users");
        let mut expanded = HashSet::from([name.clone(), rkey.clone()]);
        let lines = names(&flatten(&store, &expanded, ""));
        assert_eq!(
            lines[3..7],
            ["r:users", "c:id", "g:Indexes:1", "g:Constraints:1"].map(String::from)
        );
        expanded.insert(format!("{rkey}\u{0}Indexes"));
        let nodes = flatten(&store, &expanded, "");
        assert!(nodes.iter().any(|n| matches!(
            n,
            Node::Item { facet: Facet::Indexes, detail, .. } if detail == "primary btree"
        )));
        assert!(nodes.iter().any(|n| matches!(n, Node::Column { primary: true, .. })));
    }

    #[test]
    fn views_have_no_index_or_constraint_groups() {
        let (mut store, name) = store();
        store.schemas.get_mut(&name).unwrap()[1].relations[0].kind = RelKind::View;
        let rkey = format!("{name}\u{0}public\u{0}users");
        let expanded = HashSet::from([name.clone(), rkey]);
        assert!(
            !flatten(&store, &expanded, "")
                .iter()
                .any(|n| matches!(n, Node::Group { .. }))
        );
    }
}
