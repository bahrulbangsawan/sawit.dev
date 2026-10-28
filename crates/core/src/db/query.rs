//! Talking to a database: connect, schema tree, paged reads, ad-hoc SQL.

use std::time::{Duration, Instant};

use futures::{StreamExt, pin_mut};
use tokio_postgres::config::SslMode as PgSslMode;
use tokio_postgres::{Client, SimpleQueryMessage};

use super::profile::{ConnProfile, SslMode};
use super::quote_ident;
use crate::{CoreError, Result};

/// Formats server errors with their detail, hint and position.
pub fn sql_error(e: tokio_postgres::Error) -> CoreError {
    if e.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) {
        return CoreError::Sql("Query canceled.".into());
    }
    match e.as_db_error() {
        Some(db) => {
            let mut msg = format!("{}: {}", db.severity(), db.message());
            if let Some(d) = db.detail() {
                msg.push_str(&format!("\nDETAIL: {d}"));
            }
            if let Some(h) = db.hint() {
                msg.push_str(&format!("\nHINT: {h}"));
            }
            if let Some(tokio_postgres::error::ErrorPosition::Original(pos)) = db.position() {
                msg.push_str(&format!("\n(position {pos})"));
            }
            CoreError::Sql(msg)
        }
        None => CoreError::Db(e),
    }
}

/// Session settings every connection gets, as (name, value).
const BASE_SETTINGS: &[(&str, &str)] = &[
    // Paging without a primary key uses OFFSET/LIMIT in physical order;
    // synchronized seqscans (tables > shared_buffers/4) would start each
    // scan mid-table.
    ("synchronize_seqscans", "off"),
];

/// Connection poolers (PgBouncer, Supavisor, Neon) refuse the `options`
/// startup parameter; settings then have to be applied with SET.
pub(crate) fn rejects_startup_options(message: &str) -> bool {
    message.contains("unsupported startup parameter")
}

/// Login role for agent (MCP) access to PG Desk instances: reads everything,
/// cannot write, cannot run server programs or write server files.
pub const READ_ONLY_ROLE: &str = "pgdesk_ro";

fn config(profile: &ConnProfile, password: Option<&str>) -> tokio_postgres::Config {
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&profile.host)
        .port(profile.port)
        .user(&profile.user)
        .dbname(&profile.database)
        .application_name("pgdesk")
        .connect_timeout(Duration::from_secs(10))
        // Notice dead links (NAT idle drops on cloud databases) in about a
        // minute instead of the OS default of two hours.
        .keepalives(true)
        .keepalives_idle(Duration::from_secs(30))
        .keepalives_interval(Duration::from_secs(10))
        .keepalives_retries(3)
        .ssl_mode(match profile.ssl {
            SslMode::Disable => PgSslMode::Disable,
            SslMode::Prefer => PgSslMode::Prefer,
            SslMode::Require | SslMode::VerifyFull => PgSslMode::Require,
        });
    if let Some(pw) = password {
        cfg.password(pw);
    }
    cfg
}

async fn connect_with(
    profile: &ConnProfile,
    password: Option<&str>,
    settings: &[(&str, &str)],
) -> Result<Client> {
    let mut cfg = config(profile, password);
    let options: Vec<String> = settings
        .iter()
        .map(|(k, v)| format!("-c {k}={v}"))
        .collect();
    cfg.options(options.join(" "));
    match open(cfg, profile.ssl).await {
        Err(e) if rejects_startup_options(&e.to_string()) => {
            let client = open(config(profile, password), profile.ssl).await?;
            for (k, v) in settings {
                client
                    .batch_execute(&format!("SET {k} = {v}"))
                    .await
                    .map_err(sql_error)?;
            }
            Ok(client)
        }
        other => other,
    }
}

async fn open(cfg: tokio_postgres::Config, ssl: SslMode) -> Result<Client> {
    let (client, connection) = cfg
        .connect(super::tls::connector(ssl))
        .await
        .map_err(sql_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Asks the server to stop the query currently running on the connection
/// `token` came from (the query then fails with "Query canceled.").
pub async fn cancel(token: &tokio_postgres::CancelToken, ssl: SslMode) -> Result<()> {
    token
        .cancel_query(super::tls::connector(ssl))
        .await
        .map_err(sql_error)
}

/// Connects (must run on the Tokio runtime; the connection task is spawned there).
pub async fn connect(profile: &ConnProfile, password: Option<&str>) -> Result<Client> {
    connect_with(profile, password, BASE_SETTINGS).await
}

/// A connection whose transactions are read-only by default.
pub async fn connect_read_only(profile: &ConnProfile, password: Option<&str>) -> Result<Client> {
    let mut settings = BASE_SETTINGS.to_vec();
    settings.push(("default_transaction_read_only", "on"));
    connect_with(profile, password, &settings).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelKind {
    Table,
    View,
    MaterializedView,
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub name: String,
    pub kind: RelKind,
    pub columns: Vec<ColumnInfo>,
    /// The connected role may SELECT from it.
    pub readable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    pub name: String,
    pub relations: Vec<Relation>,
}

pub async fn list_databases(client: &Client) -> Result<Vec<String>> {
    let rows = client
        .query(
            "SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY datname",
            &[],
        )
        .await
        .map_err(sql_error)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// User schemas with their tables/views and columns (system schemas hidden).
pub async fn load_schema(client: &Client) -> Result<Vec<SchemaInfo>> {
    const SCHEMAS: &str = "SELECT nspname FROM pg_namespace
        WHERE nspname NOT IN ('pg_catalog', 'information_schema')
          AND nspname NOT LIKE 'pg_toast%' AND nspname NOT LIKE 'pg_temp%'
        ORDER BY nspname";
    const RELATIONS: &str = "SELECT n.nspname, c.relname, c.relkind::text, a.attname,
               format_type(a.atttypid, a.atttypmod), NOT a.attnotnull,
               has_table_privilege(c.oid, 'SELECT')
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        LEFT JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
        WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
          AND n.nspname NOT IN ('pg_catalog', 'information_schema')
          AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%'
        ORDER BY n.nspname, c.relname, a.attnum";
    let mut schemas: Vec<SchemaInfo> = client
        .query(SCHEMAS, &[])
        .await
        .map_err(sql_error)?
        .iter()
        .map(|r| SchemaInfo {
            name: r.get(0),
            relations: vec![],
        })
        .collect();
    for row in client.query(RELATIONS, &[]).await.map_err(sql_error)? {
        let (schema, name, kind): (String, String, String) = (row.get(0), row.get(1), row.get(2));
        let Some(s) = schemas.iter_mut().find(|s| s.name == schema) else {
            continue;
        };
        if s.relations.last().is_none_or(|r| r.name != name) {
            let kind = match kind.as_str() {
                "v" => RelKind::View,
                "m" => RelKind::MaterializedView,
                "f" => RelKind::Foreign,
                _ => RelKind::Table,
            };
            s.relations.push(Relation {
                name: name.clone(),
                kind,
                columns: vec![],
                readable: row.get(6),
            });
        }
        if let Some(col) = row.get::<_, Option<String>>(3) {
            let rel = s.relations.last_mut().expect("pushed above");
            rel.columns.push(ColumnInfo {
                name: col,
                data_type: row.get(4),
                nullable: row.get(5),
            });
        }
    }
    Ok(schemas)
}

/// Rows in a table or view; `exact` is false when only the planner's
/// estimate was available in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowCount {
    pub rows: u64,
    pub exact: bool,
}

/// Longest an exact `count(*)` may run before falling back to the estimate
/// (a slow count on a remote database would otherwise hold up the connection).
const COUNT_TIMEOUT: &str = "3s";

/// Exact count for relations estimated below ~1M rows (bounded by
/// `COUNT_TIMEOUT`), else the statistics estimate.
pub async fn row_count(client: &Client, schema: &str, table: &str) -> Result<RowCount> {
    row_count_within(client, schema, table, COUNT_TIMEOUT).await
}

/// [`row_count`] with an explicit Postgres interval for the exact count.
pub async fn row_count_within(
    client: &Client,
    schema: &str,
    table: &str,
    timeout: &str,
) -> Result<RowCount> {
    let estimate: i64 = client
        .query_opt(
            "SELECT c.reltuples::bigint FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2",
            &[&schema, &table],
        )
        .await
        .map_err(sql_error)?
        .map(|r| r.get(0))
        .unwrap_or(-1);
    let fallback = RowCount {
        rows: estimate.max(0) as u64,
        exact: false,
    };
    if estimate >= 1_000_000 {
        return Ok(fallback);
    }
    // One implicit transaction, so SET LOCAL only bounds this count.
    let sql = format!(
        "SET LOCAL statement_timeout = '{}'; SELECT count(*) FROM {}.{}",
        timeout.replace('\'', "''"),
        quote_ident(schema),
        quote_ident(table)
    );
    match client.simple_query(&sql).await {
        Ok(messages) => {
            let rows = messages
                .iter()
                .find_map(|m| match m {
                    SimpleQueryMessage::Row(r) => r.get(0).and_then(|v| v.parse().ok()),
                    _ => None,
                })
                .unwrap_or(0);
            Ok(RowCount { rows, exact: true })
        }
        Err(e) if e.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) => {
            Ok(fallback)
        }
        Err(e) => Err(sql_error(e)),
    }
}

/// Primary-key columns of a table, in key order (empty for views and
/// tables without one).
pub async fn primary_key(client: &Client, schema: &str, table: &str) -> Result<Vec<String>> {
    let rows = client
        .query(
            "SELECT a.attname::text
             FROM pg_index i
             JOIN pg_class c ON c.oid = i.indrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord)
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
             WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2
             ORDER BY k.ord",
            &[&schema, &table],
        )
        .await
        .map_err(sql_error)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultSet {
    pub columns: Vec<String>,
    /// Text values; `None` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// More rows existed than `max_rows`.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct QueryOutcome {
    pub results: Vec<ResultSet>,
    /// Rows affected by statements that returned no result set.
    pub affected: Vec<u64>,
    pub duration: Duration,
}

/// Runs one or more statements (simple protocol, text values), keeping at
/// most `max_rows` rows per result set without buffering the rest.
pub async fn run_sql(client: &Client, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    let started = Instant::now();
    let stream = client.simple_query_raw(sql).await.map_err(sql_error)?;
    pin_mut!(stream);
    let mut out = QueryOutcome::default();
    let mut current: Option<ResultSet> = None;
    while let Some(msg) = stream.next().await {
        match msg.map_err(sql_error)? {
            SimpleQueryMessage::RowDescription(cols) => {
                current = Some(ResultSet {
                    columns: cols.iter().map(|c| c.name().to_string()).collect(),
                    ..ResultSet::default()
                });
            }
            SimpleQueryMessage::Row(row) => {
                let set = current.get_or_insert_with(|| ResultSet {
                    columns: row.columns().iter().map(|c| c.name().to_string()).collect(),
                    ..ResultSet::default()
                });
                if set.rows.len() < max_rows {
                    set.rows.push(
                        (0..row.len())
                            .map(|i| row.get(i).map(str::to_string))
                            .collect(),
                    );
                } else {
                    set.truncated = true;
                }
            }
            SimpleQueryMessage::CommandComplete(n) => match current.take() {
                Some(set) => out.results.push(set),
                None => out.affected.push(n),
            },
            _ => {}
        }
    }
    if let Some(set) = current.take() {
        out.results.push(set);
    }
    out.duration = started.elapsed();
    Ok(out)
}

/// One page of a table or view. Ordered by `order_by` (normally the primary
/// key) so pages are stable while rows change; without it, physical order
/// (stable only for an unchanged table).
pub async fn fetch_page(
    client: &Client,
    schema: &str,
    table: &str,
    order_by: &[String],
    offset: u64,
    limit: u64,
) -> Result<ResultSet> {
    let order = if order_by.is_empty() {
        String::new()
    } else {
        let cols: Vec<_> = order_by.iter().map(|c| quote_ident(c)).collect();
        format!(" ORDER BY {}", cols.join(", "))
    };
    let sql = format!(
        "SELECT * FROM {}.{}{order} OFFSET {offset} LIMIT {limit}",
        quote_ident(schema),
        quote_ident(table)
    );
    let mut out = run_sql(client, &sql, limit as usize).await?;
    Ok(out.results.pop().unwrap_or_default())
}

/// Rejects SQL that the read-only mode must never run: COPY (server files /
/// programs) and attempts to switch read-only mode off.
pub fn screen_read_only_sql(sql: &str) -> Result<()> {
    let lowered = sql.to_lowercase();
    if lowered.contains("transaction_read_only") || lowered.contains("characteristics") {
        return Err(CoreError::Sql("Read-only mode can't be changed.".into()));
    }
    // First keyword after whitespace, comments and opening parentheses.
    let mut rest = lowered.as_str();
    loop {
        let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '(');
        if let Some(after) = trimmed.strip_prefix("--") {
            rest = after.split_once('\n').map(|(_, r)| r).unwrap_or("");
        } else if let Some(after) = trimmed.strip_prefix("/*") {
            rest = after.split_once("*/").map(|(_, r)| r).unwrap_or("");
        } else {
            rest = trimmed;
            break;
        }
    }
    if rest.starts_with("copy") {
        return Err(CoreError::Sql(
            "COPY is not allowed in read-only mode.".into(),
        ));
    }
    Ok(())
}

/// Creates (idempotently) the non-superuser `READ_ONLY_ROLE` with
/// `pg_read_all_data` (PostgreSQL 14+). Run on an admin connection.
pub async fn ensure_read_only_role(admin: &Client) -> Result<()> {
    admin
        .batch_execute(&format!(
            "DO $$ BEGIN
               IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{READ_ONLY_ROLE}') THEN
                 CREATE ROLE {READ_ONLY_ROLE} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;
               END IF;
             END $$;
             GRANT pg_read_all_data TO {READ_ONLY_ROLE};"
        ))
        .await
        .map_err(sql_error)
}

/// Runs one SQL statement on a `connect_read_only` connection inside a
/// read-only transaction. Multiple statements are refused (the server's
/// Parse step rejects them), as are COPY and read-only switches. Use a
/// non-superuser role (see `ensure_read_only_role`) for full protection.
pub async fn run_read_only(client: &Client, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    screen_read_only_sql(sql)?;
    client.prepare(sql).await.map_err(sql_error)?;
    client
        .batch_execute("BEGIN TRANSACTION READ ONLY")
        .await
        .map_err(sql_error)?;
    let result = run_sql(client, sql, max_rows).await;
    let _ = client.batch_execute("ROLLBACK").await;
    result
}
