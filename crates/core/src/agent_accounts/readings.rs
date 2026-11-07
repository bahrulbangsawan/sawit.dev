//! The pool's history: every account's limit readings in `accounts/readings.sqlite`, kept 120
//! days (the status line's own JSONL history keeps 8, in Claude Pool's format), so Timeline and
//! Analytics have 30 and 90 days to show and the history survives a restart. A reading is stored
//! when it changes, or every 5 minutes while it doesn't. Also the pool's notifications, with
//! read and unread.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _, params};

use super::pool::{Limit, LimitKind, Provider, Reading, Status};
use super::usage::HistoryEntry;
use crate::{CoreError, Result};

/// How long readings are kept.
pub const KEEP_DAYS: f64 = 120.0;
/// An unchanged reading is stored again after this long.
const EVERY: f64 = 300.0;

fn err(e: rusqlite::Error) -> CoreError {
    CoreError::Config(format!("The readings store: {e}"))
}

pub fn path(root: &Path) -> PathBuf {
    super::store::dir(root).join("readings.sqlite")
}

pub fn open(root: &Path) -> Result<Connection> {
    super::store::private_dir(&super::store::dir(root))?;
    let p = path(root);
    let db = Connection::open(&p).map_err(err)?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    init(&db)?;
    Ok(db)
}

/// The tables, in a database that may be in memory (tests).
pub fn init(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS readings (
           account TEXT NOT NULL, provider TEXT NOT NULL, kind TEXT NOT NULL,
           t REAL NOT NULL, used REAL NOT NULL, resets REAL NOT NULL,
           PRIMARY KEY (account, kind, t));
         CREATE INDEX IF NOT EXISTS readings_t ON readings (t);
         CREATE TABLE IF NOT EXISTS notices (
           id INTEGER PRIMARY KEY AUTOINCREMENT, t REAL NOT NULL, account TEXT NOT NULL,
           status TEXT NOT NULL, text TEXT NOT NULL, read INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS last_status (account TEXT PRIMARY KEY, status TEXT NOT NULL);",
    )
    .map_err(err)
}

/// Stores the limits read at `t` for `account`: each one that changed since its last reading,
/// or that has gone [`EVERY`] without one. How many rows were added.
pub fn record(
    db: &Connection,
    account: &str,
    provider: Provider,
    limits: &[Limit],
    t: f64,
) -> Result<usize> {
    let mut added = 0;
    for l in limits {
        let last: Option<(f64, f64, f64)> = db
            .query_row(
                "SELECT t, used, resets FROM readings WHERE account = ?1 AND kind = ?2 ORDER BY t DESC LIMIT 1",
                params![account, l.kind.id()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(err)?;
        let same = last.is_some_and(|(lt, used, resets)| {
            (used - l.used_pct).abs() < 0.05
                && (resets - l.resets_at).abs() < 60.0
                && t - lt < EVERY
        });
        if same || last.is_some_and(|(lt, ..)| t <= lt) {
            continue;
        }
        added += db
            .execute(
                "INSERT OR IGNORE INTO readings (account, provider, kind, t, used, resets) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![account, provider.id(), l.kind.id(), t, l.used_pct, l.resets_at],
            )
            .map_err(err)?;
    }
    Ok(added)
}

/// Brings in the status line's JSONL history of a Claude Code account (once, or again: rows
/// already there are skipped).
pub fn import_history(db: &Connection, account: &str, history: &[HistoryEntry]) -> Result<usize> {
    let mut added = 0;
    for e in history {
        for (kind, used, resets) in [
            (LimitKind::FiveHour, e.fh, e.fhr),
            (LimitKind::Weekly, e.sd, e.sdr),
        ] {
            if let (Some(u), Some(r)) = (used, resets) {
                added += db
                    .execute(
                        "INSERT OR IGNORE INTO readings (account, provider, kind, t, used, resets) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![account, Provider::ClaudeCode.id(), kind.id(), e.t, u, r],
                    )
                    .map_err(err)?;
            }
        }
    }
    Ok(added)
}

/// The account's readings since `from`, oldest first.
pub fn readings(db: &Connection, account: &str, from: f64) -> Result<Vec<Reading>> {
    let mut q = db
        .prepare(
            "SELECT t, kind, used, resets FROM readings WHERE account = ?1 AND t >= ?2 ORDER BY t",
        )
        .map_err(err)?;
    let rows = q
        .query_map(params![account, from], |r| {
            Ok((
                r.get::<_, f64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })
        .map_err(err)?;
    let mut out = Vec::new();
    for row in rows {
        let (t, kind, used, resets) = row.map_err(err)?;
        if let Some(kind) = LimitKind::from_id(&kind) {
            out.push(Reading {
                t,
                kind,
                used_pct: used,
                resets_at: resets,
            });
        }
    }
    Ok(out)
}

/// Drops readings older than [`KEEP_DAYS`], and notices older than 30 days.
pub fn prune(db: &Connection, now: f64) -> Result<()> {
    db.execute(
        "DELETE FROM readings WHERE t < ?1",
        params![now - KEEP_DAYS * 86400.0],
    )
    .map_err(err)?;
    db.execute(
        "DELETE FROM notices WHERE t < ?1",
        params![now - 30.0 * 86400.0],
    )
    .map_err(err)?;
    Ok(())
}

/// A notification of the pool: an account that ran low, ran out, or has room again.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Notice {
    pub id: i64,
    pub t: f64,
    pub account: String,
    pub status: String,
    pub text: String,
    pub read: bool,
}

/// Notes `status` for the account: when it moves into running low, almost out or limit
/// reached, or back to plenty left from one of those, a notice says so. Returns the notice.
pub fn note_status(
    db: &Connection,
    account: &str,
    name: &str,
    status: Status,
    t: f64,
) -> Result<Option<Notice>> {
    let before: Option<String> = db
        .query_row(
            "SELECT status FROM last_status WHERE account = ?1",
            params![account],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    // Not reported, out of date and the like say nothing about room: they don't count.
    let counts = matches!(
        status,
        Status::PlentyLeft | Status::RunningLow | Status::AlmostOut | Status::LimitReached
    );
    if !counts || before.as_deref() == Some(status.id()) {
        return Ok(None);
    }
    db.execute(
        "INSERT INTO last_status (account, status) VALUES (?1, ?2) ON CONFLICT(account) DO UPDATE SET status = ?2",
        params![account, status.id()],
    )
    .map_err(err)?;
    let text = match (before.as_deref(), status) {
        (None, _) | (Some("plenty-left"), Status::PlentyLeft) => return Ok(None),
        (_, Status::PlentyLeft) => format!("{name} has room again"),
        (_, Status::RunningLow) => format!("{name} is running low"),
        (_, Status::AlmostOut) => format!("{name} is almost out"),
        (_, Status::LimitReached) => format!("{name} reached its limit"),
        _ => return Ok(None),
    };
    db.execute(
        "INSERT INTO notices (t, account, status, text) VALUES (?1, ?2, ?3, ?4)",
        params![t, account, status.id(), text],
    )
    .map_err(err)?;
    Ok(Some(Notice {
        id: db.last_insert_rowid(),
        t,
        account: account.into(),
        status: status.id().into(),
        text,
        read: false,
    }))
}

/// The newest notices, at most `n`.
pub fn notices(db: &Connection, n: usize) -> Result<Vec<Notice>> {
    let mut q = db
        .prepare("SELECT id, t, account, status, text, read FROM notices ORDER BY t DESC, id DESC LIMIT ?1")
        .map_err(err)?;
    let rows = q
        .query_map(params![n as i64], |r| {
            Ok(Notice {
                id: r.get(0)?,
                t: r.get(1)?,
                account: r.get(2)?,
                status: r.get(3)?,
                text: r.get(4)?,
                read: r.get::<_, i64>(5)? != 0,
            })
        })
        .map_err(err)?;
    rows.collect::<std::result::Result<_, _>>().map_err(err)
}

/// Marks every notice read.
pub fn mark_all_read(db: &Connection) -> Result<usize> {
    db.execute("UPDATE notices SET read = 1 WHERE read = 0", [])
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        db
    }

    fn five(used: f64) -> Limit {
        Limit {
            kind: LimitKind::FiveHour,
            used_pct: used,
            resets_at: 20_000.0,
        }
    }

    #[test]
    fn readings_are_kept_when_they_change_or_every_five_minutes() {
        let db = db();
        assert_eq!(
            record(&db, "a", Provider::ClaudeCode, &[five(10.0)], 100.0).unwrap(),
            1
        );
        assert_eq!(
            record(&db, "a", Provider::ClaudeCode, &[five(10.0)], 160.0).unwrap(),
            0
        );
        assert_eq!(
            record(&db, "a", Provider::ClaudeCode, &[five(12.0)], 200.0).unwrap(),
            1
        );
        assert_eq!(
            record(&db, "a", Provider::ClaudeCode, &[five(12.0)], 600.0).unwrap(),
            1
        );
        let rows = readings(&db, "a", 0.0).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].used_pct, 12.0);
        prune(&db, 600.0 + KEEP_DAYS * 86400.0 - 300.0).unwrap();
        assert_eq!(readings(&db, "a", 0.0).unwrap().len(), 1);
    }

    #[test]
    fn the_jsonl_history_imports_once() {
        let db = db();
        let h = vec![HistoryEntry {
            t: 50.0,
            fh: Some(5.0),
            fhr: Some(18_000.0),
            sd: Some(2.0),
            sdr: Some(600_000.0),
        }];
        assert_eq!(import_history(&db, "a", &h).unwrap(), 2);
        assert_eq!(import_history(&db, "a", &h).unwrap(), 0);
    }

    #[test]
    fn status_changes_make_notices_that_can_be_read() {
        let db = db();
        assert!(
            note_status(&db, "a", "work", Status::PlentyLeft, 1.0)
                .unwrap()
                .is_none()
        );
        let n = note_status(&db, "a", "work", Status::RunningLow, 2.0)
            .unwrap()
            .unwrap();
        assert_eq!(n.text, "work is running low");
        assert!(
            note_status(&db, "a", "work", Status::RunningLow, 3.0)
                .unwrap()
                .is_none()
        );
        assert!(
            note_status(&db, "a", "work", Status::OutOfDate, 4.0)
                .unwrap()
                .is_none()
        );
        note_status(&db, "a", "work", Status::PlentyLeft, 5.0)
            .unwrap()
            .unwrap();
        let all = notices(&db, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].text, "work has room again");
        assert!(!all[0].read);
        assert_eq!(mark_all_read(&db).unwrap(), 2);
        assert!(notices(&db, 10).unwrap().iter().all(|n| n.read));
    }
}
