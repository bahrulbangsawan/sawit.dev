//! Adding connections: a DATABASE_URL, a .env file, the running Sawit instances, a SQLite file
//! or a Workers project's D1 databases, and the "+" menu that offers them.

use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::db::{ConnProfile, DbKind};

use super::DatabasePanel;
use super::tree::D1_SECTION;
use crate::db::logos;

impl DatabasePanel {
    pub(super) fn add_url(&mut self, url: String, cx: &mut Context<Self>) {
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

    pub(super) fn import_env(&mut self, cx: &mut Context<Self>) {
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
    pub(super) fn add_instances(&mut self, cx: &mut Context<Self>) {
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
}

/// The "+" menu: a PostgreSQL URL, a SQLite file, or a Workers project's D1 databases, each
/// with its kind's logo.
impl DatabasePanel {
    pub(super) fn add_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity().downgrade();
        Button::new("db-add")
            .ghost()
            .xsmall()
            .icon(IconName::Plus)
            .tooltip("Add a Database")
            .dropdown_menu(move |menu, _, _| {
                let item = |kind: Option<DbKind>,
                            label: &'static str,
                            f: fn(&mut DatabasePanel, &mut Window, &mut Context<DatabasePanel>)| {
                    let view = view.clone();
                    PopupMenuItem::element(move |_, _| {
                        h_flex()
                            .gap_2()
                            .child(match kind {
                                Some(kind) => logos::logo(kind, 14.),
                                None => logos::cloudflare(14.),
                            })
                            .child(label)
                    })
                    .on_click(move |_, window, cx| {
                        let _ = view.update(cx, |this, cx| f(this, window, cx));
                    })
                };
                menu.item(item(
                    Some(DbKind::Postgres),
                    "PostgreSQL from a DATABASE_URL…",
                    |this, window, cx| {
                        this.settings = None;
                        this.show_url = true;
                        this.url_input.update(cx, |s, cx| s.focus(window, cx));
                        cx.notify();
                    },
                ))
                .item(item(
                    None,
                    "Cloudflare D1 in a Workers Project…",
                    |this, _, cx| this.add_workers_project(cx),
                ))
                .item(item(
                    Some(DbKind::Sqlite),
                    "SQLite File…",
                    |this, _, cx| this.add_sqlite_file(cx),
                ))
            })
    }

    /// Asks for a Workers project folder, and lists its D1 databases.
    fn add_workers_project(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose Workers Project".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if sawit_core::cloudflare::detect_project(&dir).is_empty() {
                    this.error = Some(format!(
                        "{} has no Wrangler config (wrangler.toml, wrangler.json or wrangler.jsonc) or cloudflare.config.ts.",
                        dir.display()
                    ));
                } else {
                    this.error = None;
                    this.expanded.remove(D1_SECTION);
                    this.store.update(cx, |s, cx| s.add_project_dir(&dir, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks for a SQLite file, saves it as a connection and opens it.
    fn add_sqlite_file(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open SQLite File".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(file) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let name = this.store.update(cx, |s, cx| s.add_sqlite_file(&file, cx));
                this.error = None;
                this.expanded.insert(name);
                cx.notify();
            });
        })
        .detach();
    }
}
