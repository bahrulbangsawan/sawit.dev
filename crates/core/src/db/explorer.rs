//! D1 through a running dev server's Local Explorer. While `wrangler dev`
//! (or Vite with the Cloudflare plugin, or `cf dev`) runs, workerd owns its
//! D1 files, so Sawit sends writes to the explorer's D1 route, where workerd
//! itself runs them. Finding explorers and calling their API on loopback is
//! [`crate::cloudflare::explorer`]'s; this module picks the one that runs a
//! project's Worker with its D1 binding, and speaks the D1 route.
//!
//! The route is the one the explorer's OpenAPI document ("Local Explorer API"
//! 0.0.1) gives, [`D1_RAW`]: a batch of statements in, `{ success, errors,
//! result: [{ results: { columns, rows }, meta: { changes } }] }` out
//! (Miniflare's packages/miniflare/src/workers/local-explorer/resources/d1.ts
//! in workers-sdk). `GET /d1/database` lists the databases by the id Miniflare
//! keeps them under (`uuid`).

use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;

use serde_json::Value;

use super::query::{QueryOutcome, ResultSet};
use crate::cloudflare::explorer::{self as cf, Method};
use crate::{CoreError, Result};

/// The D1 SQL operation, as the explorer's OpenAPI document names it.
pub const D1_RAW: &str = "POST /d1/database/{database_id}/raw";

/// The error an API answer carries, in Miniflare's (Cloudflare API) shape.
fn api_error(status: u16, body: &[u8]) -> CoreError {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("errors")?
                .as_array()?
                .iter()
                .filter_map(|e| e.get("message")?.as_str())
                .map(str::to_string)
                .reduce(|a, b| format!("{a}\n{b}"))
        })
        .unwrap_or_else(|| format!("The Local Explorer answered {status}."));
    CoreError::Sql(message)
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The ids of the D1 databases the explorer at `addr` reaches (its own first, then its
/// peers').
pub async fn d1_ids(addr: SocketAddr) -> Result<Vec<String>> {
    let r = cf::request(addr, Method::Get, "d1/database", None)
        .await
        .map_err(CoreError::Network)?;
    if r.status != 200 {
        return Err(api_error(r.status, &r.body));
    }
    let v: Value = serde_json::from_slice(&r.body).map_err(|e| CoreError::Parse(e.to_string()))?;
    Ok(v.get("result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|d| d.get("uuid")?.as_str().map(str::to_string))
        .collect())
}

/// Runs `statements` on D1 database `id`, one after another, in workerd.
pub async fn raw(addr: SocketAddr, id: &str, statements: &[String]) -> Result<QueryOutcome> {
    let started = Instant::now();
    let batch: Vec<Value> = statements
        .iter()
        .map(|sql| serde_json::json!({ "sql": sql }))
        .collect();
    let body = serde_json::json!({ "batch": batch }).to_string();
    let path = format!("d1/database/{}/raw", percent_encode(id));
    let r = cf::request(
        addr,
        Method::Post,
        &path,
        Some((body.as_bytes(), "application/json")),
    )
    .await
    .map_err(CoreError::Network)?;
    let value: Value = serde_json::from_slice(&r.body).map_err(|_| api_error(r.status, &r.body))?;
    if !(200..300).contains(&r.status) || value.get("success") == Some(&Value::Bool(false)) {
        return Err(api_error(r.status, &r.body));
    }
    let mut out = QueryOutcome::default();
    for result in value
        .get("result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let columns: Vec<String> = result
            .pointer("/results/columns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().map(str::to_string))
            .collect();
        if columns.is_empty() {
            let changes = result
                .pointer("/meta/changes")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            out.affected.push(changes);
            continue;
        }
        let rows = result
            .pointer("/results/rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|row| {
                row.as_array()
                    .into_iter()
                    .flatten()
                    .map(json_text)
                    .collect()
            })
            .collect();
        out.results.push(ResultSet {
            columns,
            rows,
            truncated: false,
        });
    }
    out.duration = started.elapsed();
    Ok(out)
}

/// A JSON cell as text: NULL is `None`, strings as they are, anything else as JSON.
pub(crate) fn json_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Whether the explorer at `addr` serves D1 database `id` of Worker `worker`'s binding
/// `binding`: its API has [`D1_RAW`], it runs that Worker itself (any, when the config names
/// none) with that D1 binding, and lists the database.
pub async fn serves(
    addr: SocketAddr,
    spec: &cf::Spec,
    worker: Option<&str>,
    binding: &str,
    id: &str,
) -> bool {
    if !spec.has(D1_RAW) {
        return false;
    }
    let workers = cf::local_workers(addr).await;
    // An older explorer without `/local/workers`: the database list decides alone.
    let runs_it = workers.is_empty()
        || workers.iter().any(|w| {
            w.is_self
                && worker.is_none_or(|name| w.name == name)
                && w.bindings
                    .iter()
                    .any(|(kind, b)| kind == "d1" && b == binding)
        });
    runs_it
        && d1_ids(addr)
            .await
            .is_ok_and(|ids| ids.iter().any(|d| d == id))
}

/// The explorer at one of `addrs` that serves the database ([`serves`]). For tests and for
/// callers that already know where the dev servers are.
pub async fn find_among(
    addrs: &[SocketAddr],
    worker: Option<&str>,
    binding: &str,
    id: &str,
) -> Option<SocketAddr> {
    for &addr in addrs {
        let Ok(Some(spec)) = cf::probe(addr).await else {
            continue;
        };
        if serves(addr, &spec, worker, binding, id).await {
            return Some(addr);
        }
    }
    None
}

/// The explorer of a dev server running in `project` that serves the database: the dev
/// servers on this Mac ([`crate::probe::listening_jobs`]), the explorers among them
/// ([`cf::find`], cached per port and process), those running in the project
/// ([`cf::running_in`]).
pub async fn find_for(
    project: &Path,
    worker: Option<&str>,
    binding: &str,
    id: &str,
) -> Option<SocketAddr> {
    let jobs = crate::probe::listening_jobs().await.ok()?;
    let explorers = cf::find(&jobs).await;
    for explorer in cf::running_in(&explorers, project) {
        if serves(explorer.addr, &explorer.spec, worker, binding, id).await {
            return Some(explorer.addr);
        }
    }
    None
}

/// Splits SQL into its statements, as SQLite reads them (a trigger's `BEGIN … END` stays
/// whole); blank ones and a trailing comment are dropped.
pub fn split_statements(sql: &str) -> Vec<String> {
    let complete = |s: &str| {
        let Ok(c) = std::ffi::CString::new(s) else {
            return false;
        };
        // SAFETY: a NUL-terminated string that lives across the call.
        unsafe { rusqlite::ffi::sqlite3_complete(c.as_ptr()) != 0 }
    };
    let mut out = Vec::new();
    let mut start = 0;
    for (ix, _) in sql.match_indices(';') {
        let piece = &sql[start..=ix];
        if complete(piece) {
            let trimmed = piece.trim().trim_end_matches(';').trim();
            if has_statement(trimmed) {
                out.push(trimmed.to_string());
            }
            start = ix + 1;
        }
    }
    let rest = sql[start..].trim();
    if has_statement(rest) {
        out.push(rest.to_string());
    }
    out
}

/// Text that is more than whitespace and comments.
fn has_statement(text: &str) -> bool {
    let mut rest = text;
    loop {
        let t = rest.trim_start();
        if let Some(after) = t.strip_prefix("--") {
            rest = after.split_once('\n').map(|(_, r)| r).unwrap_or("");
        } else if let Some(after) = t.strip_prefix("/*") {
            rest = after.split_once("*/").map(|(_, r)| r).unwrap_or("");
        } else {
            return !t.is_empty();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cloudflare::explorer::API_PATH;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A stand-in Local Explorer on 127.0.0.1:0 that runs Worker `worker` with D1 binding `DB`
    /// (database `id`), records each request line and body, and answers as Miniflare does:
    /// its OpenAPI document at the API's root, then `local/workers`, `d1/database` and `raw`.
    pub(crate) async fn fake_explorer(
        worker: &'static str,
        id: &'static str,
    ) -> (SocketAddr, Arc<Mutex<Vec<(String, String)>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // The head, then as much body as Content-Length says.
                loop {
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let len = head
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if body.len() >= len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                let line = head.lines().next().unwrap_or_default().to_string();
                log.lock().unwrap().push((line.clone(), body.to_string()));
                let path = line.split_whitespace().nth(1).unwrap_or_default();
                let answer = if path == API_PATH {
                    r#"{"openapi":"3.0.3","info":{"title":"Local Explorer API","version":"0.0.1"},"paths":{"/d1/database":{"get":{}},"/d1/database/{database_id}/raw":{"post":{}},"/local/workers":{"get":{}}}}"#.to_string()
                } else if path.ends_with("/local/workers") {
                    format!(
                        r#"{{"success":true,"result":[{{"name":"{worker}","isSelf":true,"bindings":{{"d1":[{{"id":"{id}","bindingName":"DB"}}]}}}}]}}"#
                    )
                } else if path.ends_with("/d1/database") {
                    format!(r#"{{"success":true,"result":[{{"uuid":"{id}","name":"DB"}}]}}"#)
                } else if path.ends_with(&format!("/d1/database/{id}/raw")) {
                    r#"{"success":true,"errors":[],"result":[{"results":{"columns":[],"rows":[]},"meta":{"changes":2},"success":true},{"results":{"columns":["n"],"rows":[[2]]},"meta":{"changes":0},"success":true}]}"#.to_string()
                } else {
                    r#"{"success":false,"errors":[{"code":7404,"message":"The database x could not be found"}]}"#.to_string()
                };
                let status = if answer.contains("\"success\":false") {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                // Chunked, as workerd answers.
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{answer}\r\n0\r\n\r\n",
                    answer.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (addr, seen)
    }

    #[tokio::test]
    async fn finds_the_explorer_running_the_worker_and_writes_through_it() {
        let (addr, seen) = fake_explorer("api", "5d0c").await;
        // A closed port first: skipped.
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead = closed.local_addr().unwrap();
        drop(closed);
        assert_eq!(
            find_among(&[dead, addr], Some("web"), "DB", "5d0c").await,
            None
        );
        assert_eq!(
            find_among(&[dead, addr], Some("api"), "CACHE", "5d0c").await,
            None
        );
        assert_eq!(
            find_among(&[dead, addr], Some("api"), "DB", "other").await,
            None
        );
        let found = find_among(&[dead, addr], Some("api"), "DB", "5d0c").await;
        assert_eq!(found, Some(addr));
        let out = raw(
            addr,
            "5d0c",
            &[
                "DELETE FROM t WHERE x = 'a;b'".into(),
                "SELECT 2 AS n".into(),
            ],
        )
        .await
        .unwrap();
        assert_eq!(out.affected, [2]);
        assert_eq!(out.results[0].rows, [[Some("2".to_string())]]);
        // The fake server runs on this thread: the log's lock is let go before it answers again.
        let (line, body) = seen.lock().unwrap().last().cloned().unwrap();
        assert!(
            line.starts_with(&format!("POST {API_PATH}/d1/database/5d0c/raw")),
            "{line}"
        );
        let sent: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(sent["batch"][0]["sql"], "DELETE FROM t WHERE x = 'a;b'");
        let e = raw(addr, "gone", &["SELECT 1".into()]).await.unwrap_err();
        assert!(e.to_string().contains("could not be found"), "{e}");
    }

    #[test]
    fn statements_split_as_sqlite_reads_them() {
        assert_eq!(
            split_statements("INSERT INTO t VALUES ('a;b'); -- note\nSELECT 1;\n\n  "),
            ["INSERT INTO t VALUES ('a;b')", "-- note\nSELECT 1"]
        );
        assert_eq!(
            split_statements(
                "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = n + 1; END; SELECT 2"
            ),
            [
                "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = n + 1; END",
                "SELECT 2"
            ]
        );
        assert!(split_statements("  -- only a comment").is_empty());
    }
}
