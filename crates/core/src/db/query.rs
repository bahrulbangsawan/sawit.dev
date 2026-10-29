//! Talking to a database: connect, schema tree, paged reads, ad-hoc SQL.

use std::time::{Duration, Instant};

use futures::{StreamExt, pin_mut};
use tokio_postgres::config::{
    ChannelBinding as PgChannelBinding, SslMode as PgSslMode, TargetSessionAttrs,
};
use tokio_postgres::types::Type;
use tokio_postgres::{Client, SimpleQueryMessage};

use super::profile::{ChannelBinding, ConnProfile, SessionAttrs, SslMode};
use super::quote_ident;
use super::rows::RowQuery;
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

/// Why a connection couldn't be opened, which decides whether retrying can
/// help and what to tell the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Wrong user or password, or the server refuses this user/host.
    Auth,
    /// The database name doesn't exist on the server.
    DatabaseMissing,
    /// The server can't be reached: offline, DNS, refused, timed out, reset.
    Network,
    /// The server is up but can't take us now: starting, shutting down, too
    /// many connections, a serverless compute waking up.
    ServerBusy,
    /// TLS failed: certificate not trusted, server without TLS, …
    Tls,
    /// The settings themselves are unusable (certificate files, options).
    Config,
    Other,
}

impl FailureKind {
    /// Trying again later may succeed without changing anything.
    pub fn retryable(self) -> bool {
        matches!(self, FailureKind::Network | FailureKind::ServerBusy)
    }

    /// What to do about it, in a few words.
    pub fn hint(self) -> &'static str {
        match self {
            FailureKind::Auth => "Check the user name and password.",
            FailureKind::DatabaseMissing => "Check the database name.",
            FailureKind::Network => {
                "Check the host, port and your internet connection (or VPN / IP allowlist)."
            }
            FailureKind::ServerBusy => "The server is busy or starting; try again shortly.",
            FailureKind::Tls => "Check sslmode and the certificate settings.",
            FailureKind::Config => "Check the connection settings.",
            FailureKind::Other => "",
        }
    }
}

/// Sorts a failed connect by SQLSTATE, the I/O error underneath, or (for
/// proxies such as Neon's that answer with plain text) the message.
pub fn failure_kind(e: &CoreError) -> FailureKind {
    match e {
        CoreError::Connect { kind, .. } => *kind,
        CoreError::Config(_) | CoreError::Parse(_) => FailureKind::Config,
        CoreError::Io(_) => FailureKind::Network,
        CoreError::Db(e) => classify(e),
        _ => FailureKind::Other,
    }
}

fn classify(e: &tokio_postgres::Error) -> FailureKind {
    use tokio_postgres::error::SqlState;
    if let Some(code) = e.code() {
        let c = code.code();
        return match code {
            _ if *code == SqlState::INVALID_PASSWORD
                || *code == SqlState::INVALID_AUTHORIZATION_SPECIFICATION =>
            {
                FailureKind::Auth
            }
            _ if *code == SqlState::INVALID_CATALOG_NAME => FailureKind::DatabaseMissing,
            _ if *code == SqlState::TOO_MANY_CONNECTIONS
                || *code == SqlState::CANNOT_CONNECT_NOW
                || *code == SqlState::ADMIN_SHUTDOWN
                || *code == SqlState::CRASH_SHUTDOWN =>
            {
                FailureKind::ServerBusy
            }
            _ if c.starts_with("08") => FailureKind::Network,
            _ => classify_message(&e.to_string()),
        };
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = source {
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            use std::io::ErrorKind::*;
            if matches!(
                io.kind(),
                ConnectionRefused
                    | ConnectionReset
                    | ConnectionAborted
                    | NotConnected
                    | TimedOut
                    | HostUnreachable
                    | NetworkUnreachable
                    | NetworkDown
                    | AddrNotAvailable
                    | BrokenPipe
                    | UnexpectedEof
            ) {
                return FailureKind::Network;
            }
        }
        source = err.source();
    }
    classify_message(&with_causes(e))
}

/// The error with its causes ("error connecting to server: failed to lookup
/// address information: …"): the driver's own message alone rarely says
/// what went wrong.
fn with_causes(e: &tokio_postgres::Error) -> String {
    let mut out = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(err) = source {
        let text = err.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = err.source();
    }
    out
}

pub(super) fn classify_message(message: &str) -> FailureKind {
    let m = message.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| m.contains(w));
    if any(&[
        "password authentication failed",
        "authentication failed",
        "no pg_hba.conf entry",
    ]) {
        FailureKind::Auth
    } else if any(&["does not exist"]) && m.contains("database") {
        FailureKind::DatabaseMissing
    } else if any(&[
        "the database system is starting up",
        "the database system is shutting down",
        "too many connections",
        "remaining connection slots",
        "couldn't connect to compute node",
        "compute is starting",
    ]) {
        FailureKind::ServerBusy
    } else if any(&[
        "timed out",
        "timeout",
        "failed to lookup address",
        "nodename nor servname",
        "name or service not known",
        "connection refused",
        "connection reset",
        "network is unreachable",
        "no route to host",
        "unexpected eof",
        "connection closed",
    ]) {
        FailureKind::Network
    } else if any(&["tls", "ssl", "certificate"]) {
        FailureKind::Tls
    } else if any(&[
        "endpoint id is not specified",
        "invalid value for parameter",
    ]) {
        FailureKind::Config
    } else {
        FailureKind::Other
    }
}

/// Session settings every connection gets, as (name, value).
const BASE_SETTINGS: &[(&str, &str)] = &[
    // Paging without a primary key uses OFFSET/LIMIT in physical order;
    // synchronized seqscans (tables > shared_buffers/4) would start each
    // scan mid-table.
    ("synchronize_seqscans", "off"),
];

/// Connection poolers (PgBouncer, Supavisor, Neon's `-pooler` endpoints)
/// refuse the `options` startup parameter.
pub(crate) fn rejects_startup_options(message: &str) -> bool {
    message.contains("unsupported startup parameter")
}

/// Login role for agent (MCP) access to Sawit instances: reads everything,
/// cannot write, cannot run server programs or write server files.
pub const READ_ONLY_ROLE: &str = "sawit_ro";

/// How long to wait for a server that doesn't answer, unless the profile
/// says otherwise (long enough for a serverless compute to wake up).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

fn config(profile: &ConnProfile, password: Option<&str>) -> tokio_postgres::Config {
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&profile.host).port(profile.port);
    // Tried in order when the first doesn't answer.
    for h in &profile.fallback_hosts {
        cfg.host(&h.host).port(h.port);
    }
    if let Some(cb) = profile.channel_binding {
        cfg.channel_binding(match cb {
            ChannelBinding::Disable => PgChannelBinding::Disable,
            ChannelBinding::Prefer => PgChannelBinding::Prefer,
            ChannelBinding::Require => PgChannelBinding::Require,
        });
    }
    if let Some(attrs) = profile.target_session_attrs {
        cfg.target_session_attrs(match attrs {
            SessionAttrs::Any => TargetSessionAttrs::Any,
            SessionAttrs::ReadWrite => TargetSessionAttrs::ReadWrite,
            SessionAttrs::ReadOnly => TargetSessionAttrs::ReadOnly,
        });
    }
    cfg.user(&profile.user)
        .dbname(&profile.database)
        .application_name("sawit")
        .connect_timeout(
            profile
                .connect_timeout
                .map(Duration::from_secs)
                .unwrap_or(CONNECT_TIMEOUT),
        )
        // Notice dead links (NAT idle drops on cloud databases) in about a
        // minute instead of the OS default of two hours.
        .keepalives(true)
        .keepalives_idle(Duration::from_secs(30))
        .keepalives_interval(Duration::from_secs(10))
        .keepalives_retries(3)
        .ssl_mode(match profile.ssl {
            // No TLS over Unix sockets (Cloud SQL's /cloudsql/…), as in libpq.
            _ if profile.is_socket() => PgSslMode::Disable,
            SslMode::Disable => PgSslMode::Disable,
            SslMode::Prefer => PgSslMode::Prefer,
            SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => PgSslMode::Require,
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
    let options: Vec<String> = profile
        .options
        .iter()
        .cloned()
        .chain(settings.iter().map(|(k, v)| format!("-c {k}={v}")))
        .collect();
    cfg.options(options.join(" "));
    match open(cfg, profile).await {
        // Behind a transaction-mode pooler a session SET would stay on the
        // pooled server connection and leak into other clients (a production
        // app suddenly getting read-only transactions), so nothing is SET:
        // the browser only sends its own SELECTs, `fetch_page` sets what it
        // needs per statement, and `run_read_only` opens a READ ONLY
        // transaction itself.
        // The profile's own `options` go too: a pooler takes none.
        Err(e) if rejects_startup_options(&e.to_string()) => {
            open(config(profile, password), profile).await
        }
        other => other,
    }
}

async fn open(cfg: tokio_postgres::Config, profile: &ConnProfile) -> Result<Client> {
    let (client, connection) = cfg
        .connect(super::tls::connector(profile)?)
        .await
        .map_err(|e| CoreError::Connect {
            kind: classify(&e),
            message: match e.as_db_error() {
                Some(_) => sql_error(e).to_string(),
                None => with_causes(&e),
            },
        })?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Asks the server to stop the query currently running on the connection
/// `token` came from (the query then fails with "Query canceled.").
pub async fn cancel(token: &tokio_postgres::CancelToken, profile: &ConnProfile) -> Result<()> {
    token
        .cancel_query(super::tls::connector(profile)?)
        .await
        .map_err(sql_error)
}

/// Connects (must run on the Tokio runtime; the connection task is spawned there).
pub async fn connect(profile: &ConnProfile, password: Option<&str>) -> Result<Client> {
    connect_with(profile, password, BASE_SETTINGS).await
}

/// A connection whose transactions are read-only by default (except behind
/// a pooler; see `connect_with`).
pub async fn connect_read_only(profile: &ConnProfile, password: Option<&str>) -> Result<Client> {
    let mut settings = BASE_SETTINGS.to_vec();
    settings.push(("default_transaction_read_only", "on"));
    connect_with(profile, password, &settings).await
}

/// Everything the database views need on connect: the read-write and
/// read-only connections and the schema tree. Both connections open at
/// once and the schema loads as soon as the first is up, so a remote
/// database costs one handshake's latency rather than three steps in a row.
pub async fn connect_all(
    profile: &ConnProfile,
    password: Option<&str>,
) -> Result<(Client, Client, Vec<SchemaInfo>)> {
    let rw = async {
        let client = connect(profile, password).await?;
        let schema = load_schema(&client).await?;
        Ok::<_, CoreError>((client, schema))
    };
    let ((client, schema), ro) = tokio::try_join!(rw, connect_read_only(profile, password))?;
    Ok((client, ro, schema))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RelKind {
    #[default]
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

/// A foreign key from a relation's `columns` to `ref_columns` of another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

/// What a constraint enforces (`pg_constraint.contype`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    PrimaryKey,
    ForeignKey,
    Unique,
    Check,
    Exclusion,
    /// `NOT NULL` as a catalogued constraint (PostgreSQL 18+), or a kind
    /// a newer server adds.
    Other,
}

impl ConstraintKind {
    fn from_contype(contype: &str) -> ConstraintKind {
        match contype {
            "p" => ConstraintKind::PrimaryKey,
            "f" => ConstraintKind::ForeignKey,
            "u" => ConstraintKind::Unique,
            "c" => ConstraintKind::Check,
            "x" => ConstraintKind::Exclusion,
            _ => ConstraintKind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConstraintKind::PrimaryKey => "primary key",
            ConstraintKind::ForeignKey => "foreign key",
            ConstraintKind::Unique => "unique",
            ConstraintKind::Check => "check",
            ConstraintKind::Exclusion => "exclusion",
            ConstraintKind::Other => "constraint",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    pub name: String,
    pub kind: ConstraintKind,
    /// `pg_get_constraintdef`: `CHECK ((qty > 0))`, `FOREIGN KEY (…) REFERENCES …`.
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexInfo {
    pub name: String,
    /// Access method: btree, hash, gin, gist, brin, …
    pub method: String,
    pub unique: bool,
    pub primary: bool,
    /// `pg_get_indexdef`: the full `CREATE INDEX` statement.
    pub definition: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relation {
    pub name: String,
    pub kind: RelKind,
    pub columns: Vec<ColumnInfo>,
    /// The connected role may SELECT from it.
    pub readable: bool,
    /// Primary-key columns in key order (empty for views and keyless tables).
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    /// Every constraint, by name (tables only).
    pub constraints: Vec<Constraint>,
    /// Indexes, by name (tables and materialized views).
    pub indexes: Vec<IndexInfo>,
    /// The planner's row estimate; `None` before the first ANALYZE.
    pub estimated_rows: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    pub name: String,
    pub relations: Vec<Relation>,
}

pub async fn list_databases(client: &Client) -> Result<Vec<String>> {
    let rows = client
        .query_typed(
            "SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY datname",
            &[],
        )
        .await
        .map_err(sql_error)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// System schemas, hidden from the tree (`n` is pg_namespace).
const USER_SCHEMAS: &str = "n.nspname NOT IN ('pg_catalog', 'information_schema')
      AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%'";

/// User schemas with their tables/views, columns, constraints, indexes and
/// row estimates (system schemas hidden), in one round trip: four
/// statements sent as one simple query, which also works through
/// transaction-mode poolers.
pub async fn load_schema(client: &Client) -> Result<Vec<SchemaInfo>> {
    let sql = format!(
        "SELECT n.nspname FROM pg_namespace n WHERE {USER_SCHEMAS} ORDER BY 1;
        WITH rels AS (
          SELECT c.oid, n.nspname, c.relname, c.relkind, c.reltuples::bigint AS est,
                 has_table_privilege(c.oid, 'SELECT') AS readable
          FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
          WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND {USER_SCHEMAS})
        SELECT r.nspname, r.relname, r.relkind::text, r.est, r.readable,
               a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull
        FROM rels r
        LEFT JOIN pg_attribute a ON a.attrelid = r.oid AND a.attnum > 0 AND NOT a.attisdropped
        ORDER BY r.nspname, r.relname, a.attnum;
        SELECT n.nspname, c.relname, con.contype::text,
          (SELECT json_agg(a.attname ORDER BY k.o) FROM unnest(con.conkey) WITH ORDINALITY k(n, o)
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n)::text,
          fn.nspname, fc.relname,
          (SELECT json_agg(a.attname ORDER BY k.o) FROM unnest(con.confkey) WITH ORDINALITY k(n, o)
             JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n)::text,
          con.conname, pg_get_constraintdef(con.oid)
        FROM pg_constraint con
        JOIN pg_class c ON c.oid = con.conrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        LEFT JOIN pg_class fc ON fc.oid = con.confrelid
        LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace
        WHERE {USER_SCHEMAS}
        ORDER BY n.nspname, c.relname, con.conname;
        SELECT n.nspname, c.relname, ic.relname, am.amname, i.indisunique, i.indisprimary,
               pg_get_indexdef(i.indexrelid)
        FROM pg_index i
        JOIN pg_class c ON c.oid = i.indrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_class ic ON ic.oid = i.indexrelid
        JOIN pg_am am ON am.oid = ic.relam
        WHERE c.relkind IN ('r', 'p', 'm') AND {USER_SCHEMAS}
        ORDER BY n.nspname, c.relname, ic.relname"
    );
    let mut statement = 0;
    let mut schemas: Vec<SchemaInfo> = Vec::new();
    for msg in client.simple_query(&sql).await.map_err(sql_error)? {
        let row = match msg {
            SimpleQueryMessage::Row(row) => row,
            SimpleQueryMessage::CommandComplete(_) => {
                statement += 1;
                continue;
            }
            _ => continue,
        };
        let text = |i: usize| row.get(i).unwrap_or_default().to_string();
        let names = |i: usize| -> Vec<String> {
            row.get(i)
                .and_then(|v| serde_json::from_str(v).ok())
                .unwrap_or_default()
        };
        match statement {
            0 => schemas.push(SchemaInfo {
                name: text(0),
                relations: vec![],
            }),
            1 => {
                let (schema, name) = (text(0), text(1));
                let Some(s) = schemas.iter_mut().find(|s| s.name == schema) else {
                    continue;
                };
                if s.relations.last().is_none_or(|r| r.name != name) {
                    let kind = match row.get(2) {
                        Some("v") => RelKind::View,
                        Some("m") => RelKind::MaterializedView,
                        Some("f") => RelKind::Foreign,
                        _ => RelKind::Table,
                    };
                    s.relations.push(Relation {
                        name: name.clone(),
                        kind,
                        readable: row.get(4) == Some("t"),
                        estimated_rows: row
                            .get(3)
                            .and_then(|v| v.parse::<i64>().ok())
                            .filter(|n| *n >= 0),
                        ..Relation::default()
                    });
                }
                if let Some(col) = row.get(5) {
                    let rel = s.relations.last_mut().expect("pushed above");
                    rel.columns.push(ColumnInfo {
                        name: col.to_string(),
                        data_type: text(6),
                        nullable: row.get(7) == Some("t"),
                    });
                }
            }
            _ => {
                let (schema, name) = (text(0), text(1));
                let Some(rel) = schemas
                    .iter_mut()
                    .find(|s| s.name == schema)
                    .and_then(|s| s.relations.iter_mut().find(|r| r.name == name))
                else {
                    continue;
                };
                if statement == 3 {
                    rel.indexes.push(IndexInfo {
                        name: text(2),
                        method: text(3),
                        unique: row.get(4) == Some("t"),
                        primary: row.get(5) == Some("t"),
                        definition: text(6),
                    });
                    continue;
                }
                let kind = ConstraintKind::from_contype(row.get(2).unwrap_or_default());
                match kind {
                    ConstraintKind::PrimaryKey => rel.primary_key = names(3),
                    ConstraintKind::ForeignKey => rel.foreign_keys.push(ForeignKey {
                        columns: names(3),
                        ref_schema: text(4),
                        ref_table: text(5),
                        ref_columns: names(6),
                    }),
                    _ => {}
                }
                rel.constraints.push(Constraint {
                    name: text(7),
                    kind,
                    definition: text(8),
                });
            }
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
        .query_typed(
            "SELECT c.reltuples::bigint FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2",
            &[(&schema, Type::TEXT), (&table, Type::TEXT)],
        )
        .await
        .map_err(sql_error)?
        .first()
        .map(|r| r.get(0))
        .unwrap_or(-1);
    count_with_estimate(client, schema, table, estimate, timeout).await
}

/// [`row_count`] when the planner's estimate is already known (from
/// [`load_schema`]); `None` (never analyzed) counts exactly.
pub async fn row_count_estimated(
    client: &Client,
    schema: &str,
    table: &str,
    estimate: Option<i64>,
) -> Result<RowCount> {
    count_with_estimate(client, schema, table, estimate.unwrap_or(-1), COUNT_TIMEOUT).await
}

async fn count_with_estimate(
    client: &Client,
    schema: &str,
    table: &str,
    estimate: i64,
    timeout: &str,
) -> Result<RowCount> {
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
        .query_typed(
            "SELECT a.attname::text
             FROM pg_index i
             JOIN pg_class c ON c.oid = i.indrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord)
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
             WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2
             ORDER BY k.ord",
            &[(&schema, Type::TEXT), (&table, Type::TEXT)],
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
    fetch_rows(
        client,
        schema,
        table,
        order_by,
        &RowQuery::default(),
        offset,
        limit,
    )
    .await
}

/// The relation's alias in browser queries; a whole-row search reads
/// `alias::text`, which a column of the same name would shadow.
const ROW_ALIAS: &str = "\"sawit row\"";

/// [`fetch_page`] narrowed and sorted by `query`; `order_by` (the key)
/// still breaks ties.
pub async fn fetch_rows(
    client: &Client,
    schema: &str,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    offset: u64,
    limit: u64,
) -> Result<ResultSet> {
    let sql = select_sql(schema, table, order_by, query, Some((offset, limit)));
    let mut out = run_sql(client, &sql, limit as usize).await?;
    Ok(out.results.pop().unwrap_or_default())
}

/// The browser's SELECT for `query`; `page` is (offset, limit), `None` for
/// every row.
pub(super) fn select_sql(
    schema: &str,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    page: Option<(u64, u64)>,
) -> String {
    // Without any order, pages follow physical order, which synchronized
    // seqscans would shuffle. SET LOCAL lasts for this one implicit
    // transaction, so it is safe behind a pooler too.
    let (prelude, order) = match query.order_sql(ROW_ALIAS, order_by) {
        Some(cols) => ("", format!(" ORDER BY {cols}")),
        None => ("SET LOCAL synchronize_seqscans = off; ", String::new()),
    };
    let filter = query
        .where_sql(ROW_ALIAS)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let range = page
        .map(|(offset, limit)| format!(" OFFSET {offset} LIMIT {limit}"))
        .unwrap_or_default();
    format!(
        "{prelude}SELECT {ROW_ALIAS}.* FROM {}.{} AS {ROW_ALIAS}{filter}{order}{range}",
        quote_ident(schema),
        quote_ident(table)
    )
}

/// Runs one data-changing statement (see [`bulk_sql`](super::bulk_sql)) on
/// a read-write connection; a single statement commits or fails as a whole.
/// Returns the rows it changed.
pub async fn run_bulk(client: &Client, sql: &str) -> Result<u64> {
    let messages = client.simple_query(sql).await.map_err(sql_error)?;
    Ok(messages
        .iter()
        .find_map(|m| match m {
            SimpleQueryMessage::CommandComplete(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0))
}

/// Rows matching `query`'s filters, counted exactly within `COUNT_TIMEOUT`;
/// `None` when that ran out (the estimate says nothing about a filter).
pub async fn row_count_matching(
    client: &Client,
    schema: &str,
    table: &str,
    query: &RowQuery,
) -> Result<Option<RowCount>> {
    let filter = query
        .where_sql(ROW_ALIAS)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let sql = format!(
        "SET LOCAL statement_timeout = '{COUNT_TIMEOUT}'; SELECT count(*) FROM {}.{} AS {ROW_ALIAS}{filter}",
        quote_ident(schema),
        quote_ident(table)
    );
    match client.simple_query(&sql).await {
        Ok(messages) => Ok(messages.iter().find_map(|m| match m {
            SimpleQueryMessage::Row(r) => r
                .get(0)
                .and_then(|v| v.parse().ok())
                .map(|rows| RowCount { rows, exact: true }),
            _ => None,
        })),
        Err(e) if e.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) => Ok(None),
        Err(e) => Err(sql_error(e)),
    }
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
