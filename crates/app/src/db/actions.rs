//! Jobs the database views start: exports, `pg_dump` backups, CSV imports
//! and statements that drop or empty relations. Each runs on the Tokio
//! runtime and answers through a receiver; `None` means the connection
//! isn't up (and is being reconnected).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::Result;
use sawit_core::binaries::PgBinaries;
use sawit_core::db::{self, BackupScope, ExportFormat, ExportSummary, RowQuery};

use super::store::DbStore;
use crate::rt;

/// Where files are first offered to be saved: the front window's project's
/// `.nyawit/exports/`, else ~/Downloads.
pub fn save_dir(cx: &App) -> PathBuf {
    if let Some(project) = crate::cli::front_project(cx) {
        let exports = sawit_core::nyawit::exports_dir(&project);
        if sawit_core::nyawit::ensure(&project, &exports).is_ok() {
            return exports;
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let downloads = home.join("Downloads");
    if downloads.is_dir() { downloads } else { home }
}

/// The read-write client, or `None` after asking the store to reconnect.
fn client(
    db: &Entity<DbStore>,
    profile: &str,
    cx: &mut App,
) -> Option<Arc<tokio_postgres::Client>> {
    let client = db.read(cx).client(profile, false);
    if client.is_none() {
        let profile = profile.to_string();
        db.update(cx, |s, cx| s.ensure_connecting(&profile, cx));
    }
    client
}

/// Every row of `schema.table` matching `query`, streamed to `path` over a
/// connection of its own (so browsing stays responsive). `rows` counts
/// progress; setting `stop` abandons it.
#[allow(clippy::too_many_arguments)]
pub fn export_relation(
    db: &Entity<DbStore>,
    profile: &str,
    schema: String,
    table: String,
    order: Vec<String>,
    query: RowQuery,
    format: ExportFormat,
    path: PathBuf,
    rows: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    cx: &App,
) -> Option<oneshot::Receiver<Result<ExportSummary>>> {
    let store = db.read(cx);
    let profile = store.profile(profile)?.clone();
    let types = store
        .relation(&profile.name, &schema, &table)
        .map(|r| r.columns.clone())
        .unwrap_or_default();
    Some(rt::spawn(cx, async move {
        let lookup = profile.clone();
        // Keychain: blocking, and may wait on an access prompt.
        let password = tokio::task::spawn_blocking(move || db::resolve_password(&lookup))
            .await
            .unwrap_or_default();
        let client = db::connect_read_only(&profile, password.as_deref()).await?;
        db::export_rows(
            &client, &schema, &table, &order, &query, format, &types, &path, &rows, &stop,
        )
        .await
    }))
}

/// `pg_dump` of `scope` into `path` with the newest suitable binaries.
pub fn backup(
    db: &Entity<DbStore>,
    profile: &str,
    binaries: Vec<PgBinaries>,
    scope: BackupScope,
    path: PathBuf,
    cx: &mut App,
) -> Option<oneshot::Receiver<Result<()>>> {
    let profile = db.read(cx).profile(profile)?.clone();
    let client = client(db, &profile.name, cx)?;
    Some(rt::spawn(cx, async move {
        let lookup = profile.clone();
        let password = tokio::task::spawn_blocking(move || db::resolve_password(&lookup))
            .await
            .unwrap_or_default();
        db::backup(
            &client,
            &profile,
            password.as_deref(),
            &binaries,
            &scope,
            &path,
        )
        .await
    }))
}

/// Loads a CSV file into `schema.table`; answers the rows added.
pub fn import_csv(
    db: &Entity<DbStore>,
    profile: &str,
    schema: String,
    table: String,
    path: PathBuf,
    cx: &mut App,
) -> Option<oneshot::Receiver<Result<u64>>> {
    let client = client(db, profile, cx)?;
    Some(rt::spawn(cx, async move {
        db::import_csv(&client, &schema, &table, &path).await
    }))
}

/// One statement on the read-write connection; answers the rows it changed.
pub fn execute(
    db: &Entity<DbStore>,
    profile: &str,
    sql: String,
    cx: &mut App,
) -> Option<oneshot::Receiver<Result<u64>>> {
    let client = client(db, profile, cx)?;
    Some(rt::spawn(
        cx,
        async move { db::run_bulk(&client, &sql).await },
    ))
}
