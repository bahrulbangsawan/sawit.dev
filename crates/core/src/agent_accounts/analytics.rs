//! Usage analytics: the tokens Claude Code recorded on this Mac (input, output, cache write,
//! cache read, requests) per account and model over the last 24 hours, 7 or 30 days, against the
//! period before. An index in `accounts/usage-index.sqlite` (0600) is kept up to date from each
//! account's conversation files, read on from where the last scan stopped.
//!
//! Only assistant lines with `usage` are read, through a struct that declares nothing but the
//! time, ids, model and the four token counts: no message content is ever decoded or stored.
//! Claude Code writes one line per content block (and sometimes an early copy with a partial
//! output count), so each response, by message id and request id, keeps its largest counts.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde::Deserialize;

use super::Account;
use crate::{CoreError, Result};

/// How long the index keeps a response.
const KEEP_DAYS: i64 = 62;

fn err(e: rusqlite::Error) -> CoreError {
    CoreError::Config(format!("usage index: {e}"))
}

pub fn index_path(root: &Path) -> PathBuf {
    super::store::dir(root).join("usage-index.sqlite")
}

/// The index, made on first use.
pub fn open(root: &Path) -> Result<Connection> {
    super::store::private_dir(&super::store::dir(root))?;
    let path = index_path(root);
    let db = Connection::open(&path).map_err(err)?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (path TEXT PRIMARY KEY, offset INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS responses (
           key TEXT PRIMARY KEY, account TEXT NOT NULL, model TEXT NOT NULL, t INTEGER NOT NULL,
           input INTEGER NOT NULL, output INTEGER NOT NULL, cache_write INTEGER NOT NULL, cache_read INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS responses_t ON responses (t);",
    )
    .map_err(err)?;
    Ok(db)
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    uuid: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Tokens>,
}

#[derive(Deserialize, Default)]
struct Tokens {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
}

/// One response's counts from a conversation line, or `None` for anything else.
pub fn parse_line(line: &str) -> Option<(String, String, i64, [i64; 4])> {
    if !line.contains("\"usage\"") || !line.contains("assistant") {
        return None;
    }
    let l: Line = serde_json::from_str(line).ok()?;
    if l.kind.as_deref() != Some("assistant") {
        return None;
    }
    let m = l.message?;
    let u = m.usage?;
    let t = crate::github::parse_time(&l.timestamp?)?;
    let key = match (m.id, l.request_id) {
        (Some(id), Some(r)) => format!("{id}:{r}"),
        (Some(id), None) => id,
        (None, _) => l.uuid?,
    };
    Some((
        key,
        m.model.unwrap_or_else(|| "unknown".into()),
        t,
        [
            u.input_tokens,
            u.output_tokens,
            u.cache_creation_input_tokens,
            u.cache_read_input_tokens,
        ],
    ))
}

fn jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => jsonl_files(&p, out),
            Ok(t) if t.is_file() && p.extension().is_some_and(|x| x == "jsonl") => out.push(p),
            _ => {}
        }
    }
}

/// Reads what is new in every account's conversation files into the index.
pub fn scan(db: &mut Connection, accounts: &[&Account], now: i64) -> Result<usize> {
    let mut added = 0;
    let tx = db.transaction().map_err(err)?;
    for a in accounts {
        let mut files = Vec::new();
        jsonl_files(&a.dir.join("projects"), &mut files);
        for f in files {
            let key = f.to_string_lossy().to_string();
            let offset: i64 = tx
                .query_row(
                    "SELECT offset FROM files WHERE path = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let Ok(bytes) = std::fs::read(&f) else {
                continue;
            };
            let start = if offset as usize > bytes.len() {
                0
            } else {
                offset as usize
            };
            let Some(end) = bytes[start..]
                .iter()
                .rposition(|b| *b == b'\n')
                .map(|i| start + i + 1)
            else {
                continue;
            };
            for line in String::from_utf8_lossy(&bytes[start..end]).lines() {
                let Some((k, model, t, c)) = parse_line(line) else {
                    continue;
                };
                tx.execute(
                    "INSERT INTO responses (key, account, model, t, input, output, cache_write, cache_read)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(key) DO UPDATE SET t = MIN(t, excluded.t),
                       input = MAX(input, excluded.input), output = MAX(output, excluded.output),
                       cache_write = MAX(cache_write, excluded.cache_write), cache_read = MAX(cache_read, excluded.cache_read)",
                    params![k, a.id, model, t, c[0], c[1], c[2], c[3]],
                )
                .map_err(err)?;
                added += 1;
            }
            tx.execute(
                "INSERT INTO files (path, offset) VALUES (?1, ?2) ON CONFLICT(path) DO UPDATE SET offset = excluded.offset",
                params![key, end as i64],
            )
            .map_err(err)?;
        }
    }
    tx.execute(
        "DELETE FROM responses WHERE t < ?1",
        params![now - KEEP_DAYS * 86400],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(added)
}

/// Totals over a period.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub input: i64,
    pub output: i64,
    pub cache_write: i64,
    pub cache_read: i64,
    pub requests: i64,
}

impl Totals {
    pub fn total(&self) -> i64 {
        self.input + self.output + self.cache_write + self.cache_read
    }
}

/// Per account and model, for `[from, to)`.
pub fn totals(db: &Connection, from: i64, to: i64) -> Result<Vec<(String, String, Totals)>> {
    let mut q = db
        .prepare(
            "SELECT account, model, SUM(input), SUM(output), SUM(cache_write), SUM(cache_read), COUNT(*)
             FROM responses WHERE t >= ?1 AND t < ?2 GROUP BY account, model ORDER BY account, model",
        )
        .map_err(err)?;
    let rows = q
        .query_map(params![from, to], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                Totals {
                    input: r.get(2)?,
                    output: r.get(3)?,
                    cache_write: r.get(4)?,
                    cache_read: r.get(5)?,
                    requests: r.get(6)?,
                },
            ))
        })
        .map_err(err)?;
    rows.collect::<std::result::Result<_, _>>().map_err(err)
}
