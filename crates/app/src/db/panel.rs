//! Database sidebar: connections (from a URL, a project's .env, or a PG Desk
//! instance) and their schema → table/view → column tree.

use std::collections::HashSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::db::{ColumnInfo, ConnProfile, RelKind};

use super::store::DbStore;
use crate::servers::ServerStore;

pub enum DbEvent {
    OpenTable {
        profile: String,
        schema: String,
        table: String,
        columns: Vec<ColumnInfo>,
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
            prompt: Some("Pilih file .env".into()),
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
            self.error = Some("Tidak ada instance PG Desk yang berjalan.".into());
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

fn row(id: impl Into<ElementId>, depth: usize, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(22.))
        .pl(px(8. + depth as f32 * 12.))
        .pr_1()
        .gap_1p5()
        .text_sm()
        .cursor_pointer()
        .hover(|el| el.bg(cx.theme().list_hover))
}

impl Render for DatabasePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        for (pix, profile) in store.profiles.items.iter().enumerate() {
            let name = profile.name.clone();
            let open = self.expanded.contains(&name);
            let connected = store.clients.contains_key(&name);
            let connecting = store.connecting.contains(&name);
            let color = if connected {
                cx.theme().success
            } else {
                cx.theme().muted_foreground
            };
            let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name.clone());
            rows.push(
                row(("db-profile", pix), 0, cx)
                    .child(div().size(px(7.)).rounded_full().bg(color))
                    .child(div().flex_1().truncate().child(name.clone()))
                    .when(connecting, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("…"),
                        )
                    })
                    .when(connected, |el| {
                        el.child(
                            Button::new(("db-sql", pix))
                                .ghost()
                                .xsmall()
                                .label("SQL")
                                .tooltip("Editor SQL baru")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.emit(DbEvent::OpenSql {
                                        profile: n1.clone(),
                                    });
                                })),
                        )
                        .child(
                            Button::new(("db-refresh", pix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::RotateCw)
                                .tooltip("Muat ulang skema")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.store.update(cx, |s, cx| s.refresh(&n2, cx)).detach();
                                })),
                        )
                    })
                    .child(
                        Button::new(("db-remove", pix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip("Hapus koneksi (password di Keychain ikut dihapus)")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.store.update(cx, |s, cx| s.remove(&n3, cx))
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let connected = this.store.read(cx).clients.contains_key(&n4);
                        if !connected {
                            this.store.update(cx, |s, cx| s.connect(&n4, cx));
                            this.expanded.insert(n4.clone());
                            cx.notify();
                        } else {
                            this.toggle(n4.clone(), cx);
                        }
                    }))
                    .into_any_element(),
            );
            if let Some(e) = store.errors.get(&name) {
                rows.push(
                    div()
                        .pl(px(20.))
                        .pr_2()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .whitespace_normal()
                        .child(e.clone())
                        .into_any_element(),
                );
            }
            if !open {
                continue;
            }
            let Some(schemas) = store.schemas.get(&name) else {
                continue;
            };
            for (six, schema) in schemas.iter().enumerate() {
                let skey = format!("{name}\u{0}{}", schema.name);
                // `public` starts open; a click flips whatever the default was.
                let sopen = (schema.name == "public") != self.expanded.contains(&skey);
                let k = skey.clone();
                rows.push(
                    row(("db-schema", pix * 1000 + six), 1, cx)
                        .child(
                            Icon::new(if sopen {
                                IconName::FolderOpen
                            } else {
                                IconName::Folder
                            })
                            .xsmall(),
                        )
                        .child(div().truncate().child(schema.name.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(schema.relations.len().to_string()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(k.clone(), cx)))
                        .into_any_element(),
                );
                if !sopen {
                    continue;
                }
                for (rix, rel) in schema.relations.iter().enumerate() {
                    let rkey = format!("{skey}\u{0}{}", rel.name);
                    let ropen = self.expanded.contains(&rkey);
                    let icon = match rel.kind {
                        RelKind::Table | RelKind::Foreign => Icon::new(Lucide::Table),
                        RelKind::View | RelKind::MaterializedView => Icon::new(Lucide::Eye),
                    };
                    let (profile_name, schema_name, table_name, columns) = (
                        name.clone(),
                        schema.name.clone(),
                        rel.name.clone(),
                        rel.columns.clone(),
                    );
                    let toggle_key = rkey.clone();
                    let id = pix * 1_000_000 + six * 1000 + rix;
                    rows.push(
                        row(("db-rel", id), 2, cx)
                            .child(
                                Button::new(("db-rel-toggle", id))
                                    .ghost()
                                    .xsmall()
                                    .icon(if ropen {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.toggle(toggle_key.clone(), cx);
                                    })),
                            )
                            .child(icon.xsmall().text_color(cx.theme().muted_foreground))
                            .child(div().truncate().child(rel.name.clone()))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(DbEvent::OpenTable {
                                    profile: profile_name.clone(),
                                    schema: schema_name.clone(),
                                    table: table_name.clone(),
                                    columns: columns.clone(),
                                });
                            }))
                            .into_any_element(),
                    );
                    if ropen {
                        for (cix, col) in rel.columns.iter().enumerate() {
                            rows.push(
                                row(("db-col", id * 1000 + cix), 4, cx)
                                    .child(div().truncate().child(col.name.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{}{}",
                                                col.data_type,
                                                if col.nullable { "" } else { " not null" }
                                            )),
                                    )
                                    .into_any_element(),
                            );
                        }
                    }
                }
            }
        }
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
                            .tooltip("Tambah dari DATABASE_URL")
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
                            .tooltip("Impor DATABASE_URL dari file .env")
                            .on_click(cx.listener(|this, _, _, cx| this.import_env(cx))),
                    )
                    .child(
                        Button::new("db-add-instances")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Server)
                            .tooltip("Hubungkan instance PG Desk yang berjalan")
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
                                .child("Enter untuk menambah. Password disimpan di Keychain."),
                        ),
                )
            })
            .when_some(self.error.clone(), |el, e| {
                el.child(div().px_2().py_1().text_xs().text_color(cx.theme().danger).whitespace_normal().child(e))
            })
            .child(
                v_flex()
                    .id("db-rows")
                    .flex_1()
                    .py_1()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |el| {
                        el.child(
                            div()
                                .p_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Belum ada koneksi. Tambah dari DATABASE_URL, file .env, atau instance PG Desk yang berjalan."),
                        )
                    }),
            )
    }
}
