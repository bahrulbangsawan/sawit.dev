//! The context menus of a connection, a schema and a relation in the tree.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{self, BackupScope, DbKind, ExportFormat, RelKind};

use super::jobs::Target;
use super::tree::kind_noun;
use super::{DatabasePanel, DbEvent};

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

pub(super) fn profile_menu(
    menu: PopupMenu,
    panel: WeakEntity<DatabasePanel>,
    name: String,
    connected: bool,
    kind: DbKind,
    saved: bool,
) -> PopupMenu {
    let n = |_: ()| name.clone();
    let menu = if connected {
        let (sql, reload, b1, b2, off) = (n(()), n(()), n(()), n(()), n(()));
        let menu = menu
            .item(panel_item(
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
            ));
        // pg_dump backs up PostgreSQL only.
        let menu = if kind.is_postgres() {
            menu.separator()
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
        } else {
            menu
        };
        menu.separator().item(panel_item(
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
    if !saved {
        // Found in a Workers project: nothing to set or remove.
        return menu;
    }
    let settings = n(());
    menu.item(panel_item(
        "Connection Settings…",
        Some(Lucide::Settings2),
        &panel,
        move |this, window, cx| this.open_settings(&settings, window, cx),
    ))
}

pub(super) fn schema_menu(
    menu: PopupMenu,
    panel: WeakEntity<DatabasePanel>,
    profile: String,
    schema: String,
    kind: DbKind,
) -> PopupMenu {
    let (p1, s1) = (profile.clone(), schema.clone());
    let (p2, s2) = (profile.clone(), schema.clone());
    let (p3, s3) = (profile, schema.clone());
    let menu = menu.item(panel_item(
        "Show Diagram",
        Some(Lucide::Waypoints),
        &panel,
        move |_, _, cx| {
            cx.emit(DbEvent::OpenDiagram {
                profile: p1.clone(),
                schema: s1.clone(),
            })
        },
    ));
    let menu = if kind.is_postgres() {
        menu.separator()
            .item(panel_item(
                "Back Up Schema as SQL…",
                Some(Lucide::Download),
                &panel,
                move |this, _, cx| {
                    this.backup(p2.clone(), BackupScope::Schema(s2.clone()), false, cx)
                },
            ))
            .item(panel_item(
                "Back Up Schema for pg_restore…",
                None,
                &panel,
                move |this, _, cx| {
                    this.backup(p3.clone(), BackupScope::Schema(s3.clone()), true, cx)
                },
            ))
    } else {
        menu
    };
    menu.separator().item(panel_item(
        "Copy Name",
        Some(Lucide::Copy),
        &panel,
        move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(db::quote_ident(&schema))),
    ))
}

pub(super) fn relation_menu(
    menu: PopupMenu,
    panel: WeakEntity<DatabasePanel>,
    t: Target,
) -> PopupMenu {
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
    let postgres = t.db.is_postgres();
    if is_table && postgres {
        menu = menu.item(panel_item(
            "Import CSV…",
            Some(Lucide::Upload),
            &panel,
            with(|this, t, _, cx| this.import_csv(t, cx)),
        ));
    }
    if postgres {
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
    }
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
