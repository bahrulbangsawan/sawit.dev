//! Remote D1 databases, reached only through the user's own Wrangler
//! (`wrangler d1 list --json`, `wrangler d1 execute <name> --remote --json`),
//! with whatever login Wrangler has. Sawit never reads Wrangler's
//! credentials itself.
//!
//! Wrangler runs any SQL it is given, so whether a statement only reads is
//! decided here first: against an in-memory copy of the remote schema (its
//! `CREATE` statements, no rows), where SQLite itself says if a statement
//! writes ([`classify`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use rusqlite::fallible_iterator::FallibleIterator as _;
use serde_json::Value;

use super::explorer::json_text;
use super::query::{QueryOutcome, ResultSet};
use crate::{CoreError, Result};

/// A remote call waits this long (Wrangler signs in, then talks to Cloudflare).
const TIMEOUT: Duration = Duration::from_secs(90);

/// The schema's statements, the copy [`classify`] and the schema tree are built from.
pub const SCHEMA_SQL: &str = "SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_cf_%' ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'view' THEN 1 WHEN 'index' THEN 2 ELSE 3 END";

/// The Wrangler to run for `project`: its own (`node_modules/.bin`), else one on the PATH or
/// in the usual global folders (a Mac app doesn't get the shell's PATH).
pub fn wrangler(project: &Path) -> Option<PathBuf> {
    let local = project.join("node_modules/.bin/wrangler");
    if local.is_file() {
        return Some(local);
    }
    if let Some(found) = crate::probe::find_in_path("wrangler") {
        return Some(found);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    [
        PathBuf::from("/opt/homebrew/bin/wrangler"),
        PathBuf::from("/usr/local/bin/wrangler"),
        home.join(".bun/bin/wrangler"),
        home.join(".npm-global/bin/wrangler"),
        home.join("Library/pnpm/wrangler"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// A D1 database of the account Wrangler is signed in to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteD1 {
    pub name: String,
    pub uuid: String,
}

async fn run(wrangler: &Path, project: &Path, args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new(wrangler);
    command
        .args(args)
        .current_dir(project)
        // No prompts, no colours, no metrics from Sawit's runs.
        .env("WRANGLER_SEND_METRICS", "false")
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(TIMEOUT, command.output())
        .await
        .map_err(|_| CoreError::Sql("Wrangler didn't finish in time.".into()))??;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    // With --json, Wrangler reports a failure as {"error": {"text": …}}.
    let reported = serde_json::from_str::<Value>(&stdout).ok().and_then(|v| {
        v.pointer("/error/text")
            .or_else(|| v.pointer("/error"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(CoreError::Sql(reported.unwrap_or_else(|| {
        if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr
        }
    })))
}

/// `wrangler d1 list --json`.
pub async fn list(wrangler: &Path, project: &Path) -> Result<Vec<RemoteD1>> {
    let out = run(wrangler, project, &["d1", "list", "--json"]).await?;
    let value: Value =
        serde_json::from_str(out.trim()).map_err(|e| CoreError::Parse(e.to_string()))?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            Some(RemoteD1 {
                name: d.get("name")?.as_str()?.to_string(),
                uuid: d.get("uuid")?.as_str().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

/// `wrangler d1 execute <database> --remote --json --command <sql>`: every statement's
/// results (at most `max_rows` rows each) or rows changed.
pub async fn execute(
    wrangler: &Path,
    project: &Path,
    database: &str,
    sql: &str,
    max_rows: usize,
) -> Result<QueryOutcome> {
    let started = Instant::now();
    let out = run(
        wrangler,
        project,
        &[
            "d1",
            "execute",
            database,
            "--remote",
            "--json",
            "--yes",
            "--command",
            sql,
        ],
    )
    .await?;
    let mut outcome = parse_execute(&out, max_rows)?;
    outcome.duration = started.elapsed();
    Ok(outcome)
}

/// Wrangler's `--json` answer: one `{ results: [{column: value}], meta: { changes } }` per
/// statement. Columns come in the order D1 sent them.
fn parse_execute(text: &str, max_rows: usize) -> Result<QueryOutcome> {
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|e| CoreError::Parse(e.to_string()))?;
    let mut out = QueryOutcome::default();
    for statement in value.as_array().into_iter().flatten() {
        let rows: Vec<&serde_json::Map<String, Value>> = statement
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .collect();
        let Some(first) = rows.first() else {
            out.affected.push(
                statement
                    .pointer("/meta/changes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
            continue;
        };
        let columns: Vec<String> = first.keys().cloned().collect();
        out.results.push(ResultSet {
            rows: rows
                .iter()
                .take(max_rows)
                .map(|r| {
                    columns
                        .iter()
                        .map(|c| r.get(c).and_then(json_text))
                        .collect()
                })
                .collect(),
            truncated: rows.len() > max_rows,
            columns,
        });
    }
    Ok(out)
}

/// An in-memory database with the remote schema's tables, views, indexes and triggers (no
/// rows), from the rows of [`SCHEMA_SQL`]. A statement SQLite can't make here (an extension's
/// virtual table) is left out.
pub fn shadow(schema: &[(String, String)]) -> Result<Connection> {
    let conn = Connection::open_in_memory().map_err(super::sqlite::lite_error)?;
    for (_, sql) in schema {
        let _ = conn.execute_batch(sql);
    }
    Ok(conn)
}

/// Whether every statement of `sql` only reads, judged by SQLite against `shadow`. A
/// statement it can't prepare (it names something the copy lacks) counts as a write, so it is
/// asked about rather than run unseen.
pub fn classify(shadow: &Connection, sql: &str) -> std::result::Result<(), String> {
    let mut batch = rusqlite::Batch::new(shadow, sql);
    loop {
        match batch.next() {
            Ok(Some(stmt)) if stmt.readonly() => continue,
            Ok(Some(_)) => return Err("It changes the database.".into()),
            Ok(None) => return Ok(()),
            Err(e) => return Err(format!("Sawit can't tell whether it only reads ({e}).")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_answers_become_results_and_counts() {
        let text = r#"[
          {"results": [{"id": 1, "name": "Kopi", "price": 3.5}, {"id": 2, "name": "Teh", "price": null}], "success": true, "meta": {"changes": 0}},
          {"results": [], "success": true, "meta": {"changes": 3}}
        ]"#;
        let out = parse_execute(text, 1).unwrap();
        assert_eq!(out.results[0].columns, ["id", "name", "price"]);
        assert_eq!(
            out.results[0].rows,
            [[Some("1".into()), Some("Kopi".into()), Some("3.5".into())]]
        );
        assert!(out.results[0].truncated);
        assert_eq!(out.affected, [3]);
    }

    #[test]
    fn writes_are_told_from_reads_on_a_schema_copy() {
        let schema = vec![
            (
                "table".to_string(),
                "CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT)".to_string(),
            ),
            (
                "view".to_string(),
                "CREATE VIEW cheap AS SELECT * FROM products".to_string(),
            ),
        ];
        let copy = shadow(&schema).unwrap();
        assert!(classify(&copy, "SELECT * FROM cheap; SELECT count(*) FROM products").is_ok());
        assert!(classify(&copy, "SELECT 1; DELETE FROM products").is_err());
        assert!(classify(&copy, "DROP TABLE products").is_err());
        // Unknown to the copy: asked about, never run unseen.
        assert!(classify(&copy, "SELECT * FROM missing").is_err());
    }

    /// A fake `wrangler` (a shell script in a temp folder): never the real one, and nothing
    /// remote, from tests.
    #[cfg(unix)]
    #[tokio::test]
    async fn runs_wrangler_with_remote_json_arguments() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("wrangler");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/args\"\necho '[{\"results\":[{\"n\":2}],\"success\":true,\"meta\":{}}]'\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = execute(&fake, dir.path(), "shop", "SELECT 2 AS n", 10)
            .await
            .unwrap();
        assert_eq!(out.results[0].rows, [[Some("2".to_string())]]);
        let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
        assert_eq!(
            args.trim(),
            "d1 execute shop --remote --json --yes --command SELECT 2 AS n"
        );
    }
}
