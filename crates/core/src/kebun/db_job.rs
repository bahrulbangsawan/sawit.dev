//! `sawit panen|backup|tanam`: a table or query to CSV or Excel, a `pg_dump` backup, a CSV file
//! into a table.

use std::path::Path;

use crate::db;
use crate::instances::AppPaths;

use super::args::{DbJob, DbJobKind};
use super::find::find;
use super::ink::{Ink, confirm, tilde};
use super::query::stopped_instance;

/// Whether `path` may be written: new, or replacing it was allowed.
fn may_write(path: &Path, yes: bool, ink: &Ink) -> bool {
    if yes || !path.exists() {
        return true;
    }
    match confirm(&format!("{} sudah ada. Timpa?", tilde(path))) {
        Some(answer) => answer,
        None => {
            eprintln!(
                "{} {} sudah ada dan tidak ada saksi (stdin bukan terminal). Tambahkan -y untuk menimpa.",
                ink.red("🔥"),
                tilde(path)
            );
            false
        }
    }
}

/// `sawit panen|backup|tanam …`.
pub async fn db_job(job: DbJob, paths: &AppPaths, cwd: &Path) -> i32 {
    let ink = Ink::stderr();
    if job.kind != DbJobKind::Import && !may_write(&job.path, job.yes, &ink) {
        return 1;
    }
    let kebun = match find(&job.db, paths, cwd).await {
        Ok(Some(kebun)) => kebun,
        Ok(None) => {
            eprintln!(
                "{} Kebun “{}” tidak ada di peta. Lihat `sawit list db`.",
                ink.red("🔥"),
                job.db
            );
            return 1;
        }
        Err(e) => {
            eprintln!("{} {e}", ink.red("🔥"));
            return 1;
        }
    };
    let p = &kebun.profile;
    eprintln!(
        "{}",
        ink.gray(&format!(
            "🌴 {} · {}:{} · {}",
            p.database, p.host, p.port, kebun.via
        ))
    );
    let client = match db::connect(p, kebun.password.as_deref()).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{} Gagal masuk kebun {}: {e}", ink.red("🔥"), p.database);
            if let Some(name) = stopped_instance(p, paths).await {
                eprintln!("   Kebun {name} padam. Nyalakan dulu: `sawit nyalakan {name}`");
            }
            return 1;
        }
    };
    let started = std::time::Instant::now();
    let object = job.object.as_deref();
    let split = |o: &str| match o.split_once('.') {
        Some((schema, table)) => (Some(schema.to_string()), table.to_string()),
        None => (None, o.to_string()),
    };
    let done = match job.kind {
        DbJobKind::Export => {
            let object = object.unwrap_or_default();
            let result = if object.contains(char::is_whitespace) {
                db::export_query(&client, object, &job.path).await
            } else {
                let (schema, table) = split(object);
                db::export_table(&client, schema.as_deref(), &table, &job.path).await
            };
            result.map(|s| {
                let mut text = format!("dipanen {} baris ke {}", s.rows, tilde(&job.path));
                if s.truncated {
                    text.push_str(
                        " · karung Excel penuh (1.048.576 baris), pakai .csv untuk semuanya",
                    );
                }
                text
            })
        }
        DbJobKind::Backup => {
            let scope = match object {
                None => Ok(db::BackupScope::Database),
                Some(o) => match db::load_schema(&client).await {
                    Ok(schemas) if !o.contains('.') && schemas.iter().any(|s| s.name == o) => {
                        Ok(db::BackupScope::Schema(o.into()))
                    }
                    Ok(schemas) => {
                        let (schema, table) = split(o);
                        db::find_relation(&schemas, schema.as_deref(), &table).map(|(s, r)| {
                            db::BackupScope::Table {
                                schema: s.into(),
                                table: r.name.clone(),
                            }
                        })
                    }
                    Err(e) => Err(e),
                },
            };
            match scope {
                Ok(scope) => {
                    let roots =
                        crate::binaries::SearchRoots::detect(paths.managed_bin_root()).await;
                    let binaries = crate::binaries::discover(&roots).await;
                    db::backup(
                        &client,
                        p,
                        kebun.password.as_deref(),
                        &binaries,
                        &scope,
                        &job.path,
                    )
                    .await
                    .map(|()| {
                        let size = std::fs::metadata(&job.path).map(|m| m.len()).unwrap_or(0);
                        format!(
                            "bibit cadangan disimpan di {} ({} KB)",
                            tilde(&job.path),
                            size.div_ceil(1024)
                        )
                    })
                }
                Err(e) => Err(e),
            }
        }
        DbJobKind::Import => {
            let (schema, table) = split(object.unwrap_or_default());
            match db::load_schema(&client).await.and_then(|schemas| {
                db::find_relation(&schemas, schema.as_deref(), &table)
                    .map(|(s, r)| (s.to_string(), r.name.clone()))
            }) {
                Ok((schema, table)) => db::import_csv(&client, &schema, &table, &job.path)
                    .await
                    .map(|n| format!("ditanam {n} baris di {schema}.{table}")),
                Err(e) => Err(e),
            }
        }
    };
    match done {
        Ok(text) => {
            eprintln!(
                "{}",
                ink.gray(&format!("🌾 {text} · {} ms", started.elapsed().as_millis()))
            );
            0
        }
        Err(e) => {
            eprintln!("{} Gagal: {e}", ink.red("🔥"));
            1
        }
    }
}
