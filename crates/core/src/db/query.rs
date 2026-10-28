//! Talking to a database: connect, schema tree, paged reads, ad-hoc SQL.

use std::time::{Duration, Instant};

use futures::{StreamExt, pin_mut};
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use super::profile::ConnProfile;
use super::quote_ident;
use crate::{CoreError, Result};

/// Formats server errors with their detail, hint and position.
pub fn sql_error(e: tokio_postgres::Error) -> CoreError {
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
                msg.push_str(&format!("\n(posisi {pos})"));
            }
            CoreError::Sql(msg)
        }
        None => CoreError::Db(e),
    }
}

fn config(profile: &ConnProfile, password: Option<&str>) -> tokio_postgres::Config {
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&profile.host)
        .port(profile.port)
        .user(&profile.user)
        .dbname(&profile.database)
        .application_name("pgdesk")
        .connect_timeout(Duration::from_secs(5));
    if let Some(pw) = password {
        cfg.password(pw);
    }
    cfg
}

async fn connect_with(cfg: tokio_postgres::Config) -> Result<Client> {
    let (client, connection) = cfg.connect(NoTls).await.map_err(sql_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Connects (must run on the Tokio runtime; the connection task is spawned there).
pub async fn connect(profile: &ConnProfile, password: Option<&str>) -> Result<Client> {
    connect_with(config(profile, password)).await
}

/// A connection whose transactions are read-only by default.
pub async fn connect_read_only(profile: &ConnProfile, password: Option<&str>) -> Result<Client> {
    let mut cfg = config(profile, password);
    cfg.options("-c default_transaction_read_only=on");
    connect_with(cfg).await
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
               format_type(a.atttypid, a.atttypmod), NOT a.attnotnull
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

/// Exact row count for tables up to ~1M rows (by statistics), else the estimate.
pub async fn row_count(client: &Client, schema: &str, table: &str) -> Result<u64> {
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
    if (0..1_000_000).contains(&estimate) || estimate < 0 {
        let sql = format!(
            "SELECT count(*) FROM {}.{}",
            quote_ident(schema),
            quote_ident(table)
        );
        let n: i64 = client.query_one(&sql, &[]).await.map_err(sql_error)?.get(0);
        Ok(n as u64)
    } else {
        Ok(estimate as u64)
    }
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

/// One page of a table, in physical order (stable for an unchanged table).
pub async fn fetch_page(
    client: &Client,
    schema: &str,
    table: &str,
    offset: u64,
    limit: u64,
) -> Result<ResultSet> {
    let sql = format!(
        "SELECT * FROM {}.{} OFFSET {offset} LIMIT {limit}",
        quote_ident(schema),
        quote_ident(table)
    );
    let mut out = run_sql(client, &sql, limit as usize).await?;
    Ok(out.results.pop().unwrap_or_default())
}

/// Runs SQL on a `connect_read_only` connection inside a read-only
/// transaction. Statements that try to switch read-only mode off are refused.
pub async fn run_read_only(client: &Client, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    let lowered = sql.to_lowercase();
    if lowered.contains("transaction_read_only") || lowered.contains("characteristics") {
        return Err(CoreError::Sql("Mode read-only tidak boleh diubah.".into()));
    }
    client
        .batch_execute("BEGIN TRANSACTION READ ONLY")
        .await
        .map_err(sql_error)?;
    let result = run_sql(client, sql, max_rows).await;
    let _ = client.batch_execute("ROLLBACK").await;
    result
}
