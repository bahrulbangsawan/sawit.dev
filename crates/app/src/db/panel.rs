//! Database sidebar: connections (from a URL, a project's .env, or a Sawit
//! instance) and their schema → tables / views / … → column, index and
//! constraint tree, with a table-name filter and per-connection settings.
//!
//! Right-clicking a connection, schema or relation offers what to do with
//! it: open, query, export its rows, back it up with `pg_dump`, import a CSV
//! file, empty or delete it. Those run in the background and report here.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{self, BackupScope, ConnProfile, ExportFormat, RelKind, Relation};

use super::actions;
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
            jobs: Vec::new(),
            notice: None,
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

/// The relation a context menu acts on.
#[derive(Debug, Clone)]
struct Target {
    profile: String,
    schema: String,
    name: String,
    kind: RelKind,
}

impl Target {
    fn qualified(&self) -> String {
        format!(
            "{}.{}",
            db::quote_ident(&self.schema),
            db::quote_ident(&self.name)
        )
    }
}

/// Background jobs and what the context menus run.
impl DatabasePanel {
    /// Shows `label` while `rx` runs, then how it ended; reloads the schema
    /// of `refresh` after a success.
    fn track<T: 'static>(
        &mut self,
        label: String,
        rx: Option<futures::channel::oneshot::Receiver<sawit_core::Result<T>>>,
        refresh: Option<String>,
        done: impl FnOnce(T) -> String + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(rx) = rx else {
            self.notice = Some((
                true,
                "Not connected. Reconnecting; try again in a moment.".into(),
            ));
            cx.notify();
            return;
        };
        self.jobs.push(label.clone());
        self.notice = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(ix) = this.jobs.iter().position(|j| *j == label) {
                    this.jobs.remove(ix);
                }
                this.notice = Some(match result {
                    Ok(Ok(value)) => {
                        if let Some(profile) = refresh {
                            this.store
                                .update(cx, |s, cx| s.refresh(&profile, cx))
                                .detach();
                        }
                        (false, done(value))
                    }
                    Ok(Err(e)) => (true, format!("{label} failed. {e}")),
                    Err(_) => (true, format!("{label} stopped unexpectedly. Try again.")),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks where to save `name`, then calls `then` with the path.
    fn save_as(
        &mut self,
        name: String,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, std::path::PathBuf, &mut Context<Self>) + 'static,
    ) {
        let picked = cx.prompt_for_new_path(&actions::save_dir(), Some(&name));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = picked.await {
                let _ = this.update(cx, |this, cx| then(this, path, cx));
            }
        })
        .detach();
    }

    fn backup(
        &mut self,
        profile: String,
        scope: BackupScope,
        custom: bool,
        cx: &mut Context<Self>,
    ) {
        let base = match &scope {
            BackupScope::Database => self
                .store
                .read(cx)
                .profile(&profile)
                .map(|p| p.database.clone())
                .unwrap_or_else(|| "backup".into()),
            BackupScope::Schema(schema) => schema.clone(),
            BackupScope::Table { table, .. } => table.clone(),
        };
        let name = format!("{base}.{}", if custom { "dump" } else { "sql" });
        self.save_as(name, cx, move |this, path, cx| {
            let file = file_name(&path);
            let binaries = this.servers.read(cx).binaries.clone();
            let rx = actions::backup(&this.store, &profile, binaries, scope, path, cx);
            this.track(
                format!("Backing up {base}"),
                rx,
                None,
                move |()| format!("Saved the backup to {file}."),
                cx,
            );
        });
    }

    fn export(&mut self, target: Target, format: ExportFormat, cx: &mut Context<Self>) {
        let name = format!("{}.{}", target.name, format.extension());
        self.save_as(name, cx, move |this, path, cx| {
            let file = file_name(&path);
            let order = this
                .store
                .read(cx)
                .relation(&target.profile, &target.schema, &target.name)
                .map(|r| r.primary_key.clone())
                .unwrap_or_default();
            let rx = actions::export_relation(
                &this.store,
                &target.profile,
                target.schema.clone(),
                target.name.clone(),
                order,
                Default::default(),
                format,
                path,
                Default::default(),
                Default::default(),
                cx,
            );
            this.track(
                format!("Exporting {}", target.name),
                rx,
                None,
                move |summary| {
                    let rows = summary.rows;
                    let rows = format!("{rows} row{}", if rows == 1 { "" } else { "s" });
                    if summary.truncated {
                        format!("Saved the first {rows} to {file}: Excel holds no more. Export as CSV for every row.")
                    } else {
                        format!("Saved {rows} to {file}.")
                    }
                },
                cx,
            );
        });
    }

    fn import_csv(&mut self, target: Target, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(format!("Import into {}", target.name).into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let rx = actions::import_csv(
                    &this.store,
                    &target.profile,
                    target.schema.clone(),
                    target.name.clone(),
                    path,
                    cx,
                );
                let table = target.name.clone();
                this.track(
                    format!("Importing into {table}"),
                    rx,
                    Some(target.profile.clone()),
                    move |n| {
                        format!(
                            "Imported {n} row{} into {table}.",
                            if n == 1 { "" } else { "s" }
                        )
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    /// Asks with `buttons` (the last cancels); answer `i` runs `sql[i]`.
    #[allow(clippy::too_many_arguments)]
    fn confirm_sql(
        &mut self,
        profile: String,
        message: String,
        detail: String,
        buttons: Vec<&'static str>,
        sql: Vec<String>,
        label: String,
        done: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = window.prompt(PromptLevel::Warning, &message, Some(&detail), &buttons, cx);
        cx.spawn(async move |this, cx| {
            let Ok(choice) = answer.await else { return };
            let Some(sql) = sql.into_iter().nth(choice) else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let rx = actions::execute(&this.store, &profile, sql, cx);
                this.track(label, rx, Some(profile.clone()), move |_| done, cx);
            });
        })
        .detach();
    }

    fn delete_relation(&mut self, t: Target, window: &mut Window, cx: &mut Context<Self>) {
        let noun = kind_noun(t.kind);
        self.confirm_sql(
            t.profile.clone(),
            format!("Delete the {noun} {}.{}?", t.schema, t.name),
            "This can't be undone. If views or foreign keys depend on it, \
             “Delete with Dependents” removes those too."
                .into(),
            vec!["Delete", "Delete with Dependents", "Cancel"],
            vec![
                db::drop_sql(&t.schema, &t.name, t.kind, false),
                db::drop_sql(&t.schema, &t.name, t.kind, true),
            ],
            format!("Deleting {}", t.name),
            format!("Deleted the {noun} {}.", t.name),
            window,
            cx,
        );
    }

    fn empty_table(&mut self, t: Target, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_sql(
            t.profile.clone(),
            format!("Delete every row in {}.{}?", t.schema, t.name),
            "The table stays, empty. This can't be undone.".into(),
            vec!["Empty Table", "Cancel"],
            vec![db::truncate_sql(&t.schema, &t.name)],
            format!("Emptying {}", t.name),
            format!("Emptied {}.", t.name),
            window,
            cx,
        );
    }

    fn refresh_matview(&mut self, t: Target, cx: &mut Context<Self>) {
        let rx = actions::execute(
            &self.store,
            &t.profile,
            db::refresh_matview_sql(&t.schema, &t.name),
            cx,
        );
        let name = t.name.clone();
        self.track(
            format!("Refreshing {name}"),
            rx,
            None,
            move |_| format!("Refreshed {name}."),
            cx,
        );
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A menu item that runs `f` on the panel.
fn panel_item(
    label: impl Into<SharedString>,
    icon: Option<Lucide>,
    panel: &WeakEntity<DatabasePanel>,
    f: impl Fn(&mut DatabasePanel, &mut Window, &mut Context<DatabasePanel>) + 'static,
) -> PopupMenuItem {
    let panel = panel.clone();
    PopupMenuItem::new(label)
        .when_some(icon, |item, icon| item.icon(icon))
        .on_click(move |_, window, cx| {
            let _ = panel.update(cx, |this, cx| f(this, window, cx));
        })
}

fn profile_menu(
    menu: PopupMenu,
    panel: WeakEntity<DatabasePanel>,
    name: String,
    connected: bool,
) -> PopupMenu {
    let n = |_: ()| name.clone();
    let menu = if connected {
        let (sql, reload, b1, b2, off) = (n(()), n(()), n(()), n(()), n(()));
        menu.item(panel_item(
            "New SQL Editor",
            Some(Lucide::SquareTerminal),
            &panel,
            move |_, _, cx| {
                cx.emit(DbEvent::OpenSql {
                    profile: sql.clone(),
                    sql: None,
                })
            },
        ))
        .item(panel_item(
            "Reload Schema",
            Some(Lucide::RefreshCw),
            &panel,
            move |this, _, cx| {
                this.store
                    .update(cx, |s, cx| s.refresh(&reload, cx))
                    .detach();
            },
        ))
        .separator()
        .item(panel_item(
            "Back Up Database as SQL…",
            Some(Lucide::Download),
            &panel,
            move |this, _, cx| this.backup(b1.clone(), BackupScope::Database, false, cx),
        ))
        .item(panel_item(
            "Back Up Database for pg_restore…",
            None,
            &panel,
            move |this, _, cx| this.backup(b2.clone(), BackupScope::Database, true, cx),
        ))
        .separator()
        .item(panel_item(
            "Disconnect",
            Some(Lucide::Unplug),
            &panel,
            move |this, _, cx| {
                this.store.update(cx, |s, cx| s.disconnect(&off, cx));
            },
        ))
    } else {
        let on = n(());
        menu.item(panel_item(
            "Connect",
            Some(Lucide::Plug),
            &panel,
            move |this, _, cx| {
                this.store.update(cx, |s, cx| s.connect(&on, cx));
                this.expanded.insert(on.clone());
                cx.notify();
            },
        ))
    };
    let settings = n(());
    menu.item(panel_item(
        "Connection Settings…",
        Some(Lucide::Settings2),
        &panel,
        move |this, window, cx| this.open_settings(&settings, window, cx),
    ))
}

fn schema_menu(
    menu: PopupMenu,
    panel: WeakEntity<DatabasePanel>,
    profile: String,
    schema: String,
) -> PopupMenu {
    let (p1, s1) = (profile.clone(), schema.clone());
    let (p2, s2) = (profile.clone(), schema.clone());
    let (p3, s3) = (profile, schema.clone());
    menu.item(panel_item(
        "Show Diagram",
        Some(Lucide::Waypoints),
        &panel,
        move |_, _, cx| {
            cx.emit(DbEvent::OpenDiagram {
                profile: p1.clone(),
                schema: s1.clone(),
            })
        },
    ))
    .separator()
    .item(panel_item(
        "Back Up Schema as SQL…",
        Some(Lucide::Download),
        &panel,
        move |this, _, cx| this.backup(p2.clone(), BackupScope::Schema(s2.clone()), false, cx),
    ))
    .item(panel_item(
        "Back Up Schema for pg_restore…",
        None,
        &panel,
        move |this, _, cx| this.backup(p3.clone(), BackupScope::Schema(s3.clone()), true, cx),
    ))
    .separator()
    .item(panel_item(
        "Copy Name",
        Some(Lucide::Copy),
        &panel,
        move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(db::quote_ident(&schema))),
    ))
}

fn relation_menu(menu: PopupMenu, panel: WeakEntity<DatabasePanel>, t: Target) -> PopupMenu {
    let noun = kind_noun(t.kind);
    let is_table = t.kind == RelKind::Table;
    let with = |f: fn(&mut DatabasePanel, Target, &mut Window, &mut Context<DatabasePanel>)| {
        let t = t.clone();
        move |this: &mut DatabasePanel, window: &mut Window, cx: &mut Context<DatabasePanel>| {
            f(this, t.clone(), window, cx)
        }
    };
    let mut menu = menu
        .item(panel_item(
            "Open",
            Some(Lucide::Table),
            &panel,
            with(|_, t, _, cx| {
                cx.emit(DbEvent::OpenTable {
                    profile: t.profile,
                    schema: t.schema,
                    table: t.name,
                })
            }),
        ))
        .item(panel_item(
            "Query in SQL Editor",
            Some(Lucide::SquareTerminal),
            &panel,
            with(|_, t, _, cx| {
                let sql = format!("SELECT *\nFROM {}\nLIMIT 100;", t.qualified());
                cx.emit(DbEvent::OpenSql {
                    profile: t.profile,
                    sql: Some(sql),
                })
            }),
        ))
        .separator()
        .item(panel_item(
            "Download as CSV…",
            Some(Lucide::FileText),
            &panel,
            with(|this, t, _, cx| this.export(t, ExportFormat::Csv, cx)),
        ))
        .item(panel_item(
            "Download as Excel…",
            Some(Lucide::FileSpreadsheet),
            &panel,
            with(|this, t, _, cx| this.export(t, ExportFormat::Xlsx, cx)),
        ));
    if is_table {
        menu = menu.item(panel_item(
            "Import CSV…",
            Some(Lucide::Upload),
            &panel,
            with(|this, t, _, cx| this.import_csv(t, cx)),
        ));
    }
    menu = menu
        .separator()
        .item(panel_item(
            "Back Up as SQL…",
            Some(Lucide::Download),
            &panel,
            with(|this, t, _, cx| {
                let scope = BackupScope::Table {
                    schema: t.schema,
                    table: t.name,
                };
                this.backup(t.profile, scope, false, cx)
            }),
        ))
        .item(panel_item(
            "Back Up for pg_restore…",
            None,
            &panel,
            with(|this, t, _, cx| {
                let scope = BackupScope::Table {
                    schema: t.schema,
                    table: t.name,
                };
                this.backup(t.profile, scope, true, cx)
            }),
        ));
    if t.kind == RelKind::MaterializedView {
        menu = menu.item(panel_item(
            "Refresh Materialized View",
            Some(Lucide::RefreshCw),
            &panel,
            with(|this, t, _, cx| this.refresh_matview(t, cx)),
        ));
    }
    let qualified = t.qualified();
    menu = menu
        .separator()
        .item(panel_item(
            "Copy Name",
            Some(Lucide::Copy),
            &panel,
            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(qualified.clone())),
        ))
        .separator();
    if is_table {
        menu = menu.item(panel_item(
            "Empty Table…",
            Some(Lucide::Eraser),
            &panel,
            with(|this, t, window, cx| this.empty_table(t, window, cx)),
        ));
    }
    menu.item(panel_item(
        format!("Delete {}…", capitalize(noun)),
        Some(Lucide::Trash),
        &panel,
        with(|this, t, window, cx| this.delete_relation(t, window, cx)),
    ))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
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

/// The sections a schema's relations are listed in, in this order.
const KIND_ORDER: [RelKind; 4] = [
    RelKind::Table,
    RelKind::View,
    RelKind::MaterializedView,
    RelKind::Foreign,
];

fn kind_label(kind: RelKind) -> &'static str {
    match kind {
        RelKind::Table => "Tables",
        RelKind::View => "Views",
        RelKind::MaterializedView => "Materialized Views",
        RelKind::Foreign => "Foreign Tables",
    }
}

/// "table", "view", … for menus and prompts.
fn kind_noun(kind: RelKind) -> &'static str {
    match kind {
        RelKind::Table => "table",
        RelKind::View => "view",
        RelKind::MaterializedView => "materialized view",
        RelKind::Foreign => "foreign table",
    }
}

fn kind_icon(kind: RelKind) -> Lucide {
    match kind {
        RelKind::Table | RelKind::Foreign => Lucide::Table,
        RelKind::View => Lucide::Eye,
        RelKind::MaterializedView => Lucide::Layers,
    }
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
    /// "Tables", "Views", … under a schema; open unless collapsed.
    Kind {
        key: String,
        kind: RelKind,
        count: usize,
        open: bool,
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
            for kind in KIND_ORDER {
                let of_kind: Vec<&Relation> = matching
                    .iter()
                    .copied()
                    .filter(|r| r.kind == kind)
                    .collect();
                if of_kind.is_empty() {
                    continue;
                }
                // Sections start open; the key marks one collapsed.
                let kkey = format!("{skey}\u{0}\u{1}{}", kind_label(kind));
                let kopen = !query.is_empty() || !expanded.contains(&kkey);
                out.push(Node::Kind {
                    key: kkey,
                    kind,
                    count: of_kind.len(),
                    open: kopen,
                });
                if !kopen {
                    continue;
                }
                for rel in of_kind {
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
                    .context_menu({
                        let (weak, name, connected) =
                            (cx.entity().downgrade(), name.clone(), *connected);
                        move |menu, _, _| profile_menu(menu, weak.clone(), name.clone(), connected)
                    })
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
                let node_profile = profile.clone();
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
                .context_menu({
                    let weak = cx.entity().downgrade();
                    let (profile, schema) = (node_profile.clone(), name.clone());
                    move |menu, _, _| {
                        schema_menu(menu, weak.clone(), profile.clone(), schema.clone())
                    }
                })
                .into_any_element()
            }
            Node::Kind {
                key,
                kind,
                count,
                open,
            } => {
                let k = key.clone();
                row(ElementId::Name(format!("db-kind\u{0}{key}").into()), 2, cx)
                    .text_color(muted)
                    .child(
                        Icon::new(if *open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall(),
                    )
                    .child(div().text_xs().font_semibold().child(kind_label(*kind)))
                    .child(div().text_xs().child(count.to_string()))
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
                let icon = Icon::new(kind_icon(*kind));
                if !readable {
                    // Disabled: nothing to open, the tooltip says why.
                    return h_flex()
                        .id(id("db-rel"))
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .pl(px(8. + 3. * 12.))
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
                let target = Target {
                    profile: profile.clone(),
                    schema: schema.clone(),
                    name: name.clone(),
                    kind: *kind,
                };
                let (profile, schema, table) = (profile.clone(), schema.clone(), name.clone());
                row(id("db-rel"), 3, cx)
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
                    .context_menu({
                        let weak = cx.entity().downgrade();
                        move |menu, _, _| relation_menu(menu, weak.clone(), target.clone())
                    })
                    .into_any_element()
            }
            Node::Column {
                key,
                name,
                detail,
                primary,
            } => row(ElementId::Name(format!("db-col\u{0}{key}").into()), 5, cx)
                .cursor_default()
                // The name keeps its room; the type truncates first.
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(relative(0.7))
                        .truncate()
                        .child(name.clone()),
                )
                .when(*primary, |el| {
                    el.child(Icon::new(Lucide::KeyRound).xsmall().text_color(muted))
                })
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .child(detail.clone()),
                )
                .into_any_element(),
            Node::Group {
                key,
                facet,
                count,
                open,
            } => {
                let k = key.clone();
                let empty = *count == 0;
                row(ElementId::Name(format!("db-group\u{0}{key}").into()), 5, cx)
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
                row(ElementId::Name(format!("db-item\u{0}{key}").into()), 7, cx)
                    .cursor_default()
                    .child(div().truncate().child(name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .flex_shrink_0()
                            .child(detail.clone()),
                    )
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
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.remove_from_settings(cx)),
                                ),
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
                Node::Kind { kind, .. } => format!("k:{}", super::kind_label(*kind)),
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
                "k:Tables".into(),
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
            vec![
                format!("p:{name}"),
                "s:audit".into(),
                "k:Tables".into(),
                "r:log".into()
            ]
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
            lines[4..8],
            ["r:users", "c:id", "g:Indexes:1", "g:Constraints:1"].map(String::from)
        );
        expanded.insert(format!("{rkey}\u{0}Indexes"));
        let nodes = flatten(&store, &expanded, "");
        assert!(nodes.iter().any(|n| matches!(
            n,
            Node::Item { facet: Facet::Indexes, detail, .. } if detail == "primary btree"
        )));
        assert!(
            nodes
                .iter()
                .any(|n| matches!(n, Node::Column { primary: true, .. }))
        );
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

    #[test]
    fn relations_are_listed_by_kind_in_collapsible_sections() {
        let (mut store, name) = store();
        let public = &mut store.schemas.get_mut(&name).unwrap()[1];
        public.relations[1].kind = RelKind::View;
        public.relations.push(Relation {
            kind: RelKind::MaterializedView,
            ..rel("totals", true)
        });
        public.relations.push(rel("accounts", true));
        let mut expanded = HashSet::from([name.clone()]);
        let lines = names(&flatten(&store, &expanded, ""));
        assert_eq!(
            lines[3..],
            [
                "k:Tables",
                "r:users",
                "r:accounts",
                "k:Views",
                "r:secret",
                "k:Materialized Views",
                "r:totals"
            ]
            .map(String::from)
        );
        // Collapsing a section hides its relations only.
        expanded.insert(format!("{name}\u{0}public\u{0}\u{1}Tables"));
        let lines = names(&flatten(&store, &expanded, ""));
        assert_eq!(
            lines[3..6],
            ["k:Tables", "k:Views", "r:secret"].map(String::from)
        );
        // A filter opens every section with a match.
        let lines = names(&flatten(&store, &expanded, "acc"));
        assert_eq!(
            lines[1..],
            ["s:public", "k:Tables", "r:accounts"].map(String::from)
        );
    }
}
