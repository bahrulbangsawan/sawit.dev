//! Background jobs and what the context menus run: backups, exports, CSV imports and the
//! statements that empty, delete or refresh a relation, each reported in the panel.

use gpui_kit::*;
use sawit_core::db::{self, BackupScope, DbKind, ExportFormat, RelKind, WriteRoute};

use super::DatabasePanel;
use super::tree::kind_noun;
use crate::db::actions;

/// The relation a context menu acts on.
#[derive(Debug, Clone)]
pub(super) struct Target {
    pub(super) profile: String,
    pub(super) schema: String,
    pub(super) name: String,
    pub(super) kind: RelKind,
    /// The database's kind, for its dialect and what it offers.
    pub(super) db: DbKind,
}

impl Target {
    pub(super) fn qualified(&self) -> String {
        db::relation_sql_for(self.db, &self.schema, &self.name)
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
        let picked = cx.prompt_for_new_path(&actions::save_dir(cx), Some(&name));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = picked.await {
                let _ = this.update(cx, |this, cx| then(this, path, cx));
            }
        })
        .detach();
    }

    pub(super) fn backup(
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

    pub(super) fn export(&mut self, target: Target, format: ExportFormat, cx: &mut Context<Self>) {
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

    pub(super) fn import_csv(&mut self, target: Target, cx: &mut Context<Self>) {
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
        let (level, detail) = match self.store.read(cx).route(&profile) {
            Some(route @ WriteRoute::Remote { .. }) => (
                PromptLevel::Critical,
                format!("{detail}\n\n{}", route.describe()),
            ),
            Some(route) => (
                PromptLevel::Warning,
                format!("{detail}\n\n{}", route.describe()),
            ),
            None => (PromptLevel::Warning, detail),
        };
        let answer = window.prompt(level, &message, Some(&detail), &buttons, cx);
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

    pub(super) fn delete_relation(
        &mut self,
        t: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let noun = kind_noun(t.kind);
        // SQLite has no CASCADE: one way to delete.
        let (detail, buttons, sql) = if t.db.is_postgres() {
            (
                "This can't be undone. If views or foreign keys depend on it, \
                 “Delete with Dependents” removes those too.",
                vec!["Delete", "Delete with Dependents", "Cancel"],
                vec![
                    db::drop_sql(&t.schema, &t.name, t.kind, false),
                    db::drop_sql(&t.schema, &t.name, t.kind, true),
                ],
            )
        } else {
            (
                "This can't be undone.",
                vec!["Delete", "Cancel"],
                vec![db::drop_sql_for(t.db, &t.schema, &t.name, t.kind, false)],
            )
        };
        self.confirm_sql(
            t.profile.clone(),
            format!("Delete the {noun} {}.{}?", t.schema, t.name),
            detail.into(),
            buttons,
            sql,
            format!("Deleting {}", t.name),
            format!("Deleted the {noun} {}.", t.name),
            window,
            cx,
        );
    }

    pub(super) fn empty_table(&mut self, t: Target, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_sql(
            t.profile.clone(),
            format!("Delete every row in {}.{}?", t.schema, t.name),
            "The table stays, empty. This can't be undone.".into(),
            vec!["Empty Table", "Cancel"],
            vec![db::empty_sql_for(t.db, &t.schema, &t.name)],
            format!("Emptying {}", t.name),
            format!("Emptied {}.", t.name),
            window,
            cx,
        );
    }

    pub(super) fn refresh_matview(&mut self, t: Target, cx: &mut Context<Self>) {
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
