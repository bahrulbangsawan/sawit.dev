//! MySQL and MariaDB servers: what the table browser, SQL editor, schema tree, diagrams,
//! exports and `sawit-mcp` need, over the same types the PostgreSQL code returns.
//!
//! A server's databases are the tree's schemas (MySQL's own word for them): the profile's
//! database alone when it names one, else every database but the server's own (`mysql`,
//! `information_schema`, `performance_schema`, `sys`).
//!
//! A handle is a small connection pool ([`My`]), so the grid, the editor and a count can run at
//! once. The read-only handle's sessions are `TRANSACTION READ ONLY` from the start (and again
//! after every reset the pool makes), and what it runs is screened first ([`run`]): only
//! statements that read get there.

mod rows;
mod run;
mod schema;

use std::path::PathBuf;
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{
    ClientIdentity, Column, Opts, OptsBuilder, Pool, PoolConstraints, PoolOpts, SslOpts, Value,
};

use super::profile::{ConnProfile, SslMode};
use super::query::FailureKind;
use crate::{CoreError, Result};

pub use rows::{bulk_sql, order_sql, select_sql, where_sql};
pub use run::split_statements;
pub use schema::SYSTEM_DATABASES;

/// MySQL's port, which a URL without one means.
pub const DEFAULT_PORT: u16 = 3306;

/// What a connect waits for the server when the profile doesn't say.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// A live handle on a MySQL or MariaDB server.
pub struct My {
    pub profile: ConnProfile,
    /// Every session is read-only, and only statements that read are sent.
    pub read_only: bool,
    pool: Pool,
    /// `8.4.13`, `11.4.2-MariaDB`: what the server says it is.
    pub version: String,
}

impl My {
    /// Connects to `profile`'s server, so a wrong password or host shows now rather than on
    /// the first click. `Prefer` tries TLS and, when the server has none, connects without.
    pub async fn connect(
        profile: &ConnProfile,
        password: Option<&str>,
        read_only: bool,
    ) -> Result<My> {
        let tls = match profile.ssl {
            SslMode::Disable => None,
            _ => Some(ssl_opts(profile)?),
        };
        let attempt = |tls: Option<SslOpts>| {
            let pool = Pool::new(opts(profile, password, read_only, tls));
            async move {
                let wait = profile
                    .connect_timeout
                    .filter(|s| *s > 0)
                    .map(Duration::from_secs)
                    .unwrap_or(CONNECT_TIMEOUT);
                let conn = tokio::time::timeout(wait, pool.get_conn()).await;
                match conn {
                    Ok(Ok(conn)) => {
                        let version = server_version(conn).await;
                        Ok((pool, version))
                    }
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(mysql_async::Error::Io(mysql_async::IoError::Io(
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            format!("The server didn't answer within {}s.", wait.as_secs()),
                        ),
                    ))),
                }
            }
        };
        let result = match attempt(tls.clone()).await {
            Err(e) if profile.ssl == SslMode::Prefer && tls_unsupported(&e) => attempt(None).await,
            other => other,
        };
        let (pool, version) = result.map_err(connect_error)?;
        Ok(My {
            profile: profile.clone(),
            read_only,
            pool,
            version,
        })
    }

    async fn conn(&self) -> Result<mysql_async::Conn> {
        self.pool.get_conn().await.map_err(my_error)
    }

    /// `MySQL 8.4`, `MariaDB 11.4`: the kind and the major.minor version, for the sidebar.
    pub fn version_label(&self) -> String {
        version_label(&self.version)
    }
}

/// `MySQL 8.4` from `8.4.13`, `MariaDB 11.4` from `11.4.2-MariaDB-log` (and from the
/// `5.5.5-11.4.2-MariaDB` older clients see).
pub fn version_label(version: &str) -> String {
    let maria = version.to_ascii_lowercase().contains("mariadb");
    let v = version.strip_prefix("5.5.5-").unwrap_or(version);
    let short: Vec<&str> = v
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()
        .unwrap_or_default()
        .split('.')
        .take(2)
        .collect();
    let kind = if maria { "MariaDB" } else { "MySQL" };
    if short.iter().all(|p| p.is_empty()) {
        kind.to_string()
    } else {
        format!("{kind} {}", short.join("."))
    }
}

async fn server_version(mut conn: mysql_async::Conn) -> String {
    let asked: Option<String> = conn.query_first("SELECT VERSION()").await.ok().flatten();
    asked.unwrap_or_else(|| {
        let (a, b, c) = conn.server_version();
        format!("{a}.{b}.{c}")
    })
}

/// What each connection of a read-only handle runs first. The statement check
/// (`split_statements`, `write_reason`) reads `\` as an escape and `"` as a string quote; with
/// `NO_BACKSLASH_ESCAPES` or `ANSI_QUOTES` the server would read the same text as other
/// statements than the ones checked, so the session drops those two flags.
fn read_only_setup() -> Vec<String> {
    vec![
        "SET SESSION sql_mode = TRIM(BOTH ',' FROM REPLACE(REPLACE(CONCAT(',', \
         @@SESSION.sql_mode, ','), ',NO_BACKSLASH_ESCAPES,', ','), ',ANSI_QUOTES,', ','))"
            .into(),
        "SET SESSION TRANSACTION READ ONLY".into(),
    ]
}

fn opts(
    profile: &ConnProfile,
    password: Option<&str>,
    read_only: bool,
    tls: Option<SslOpts>,
) -> Opts {
    let setup = if read_only {
        read_only_setup()
    } else {
        Vec::new()
    };
    let mut b = OptsBuilder::default()
        .user(Some(profile.user.clone()).filter(|u| !u.is_empty()))
        .pass(password.map(str::to_string))
        .db_name(Some(profile.database.clone()).filter(|d| !d.is_empty()))
        .ssl_opts(tls)
        .setup(setup)
        .stmt_cache_size(0)
        .pool_opts(
            PoolOpts::default()
                .with_constraints(PoolConstraints::new(0, 4).expect("0 <= 4"))
                .with_inactive_connection_ttl(Duration::from_secs(60)),
        );
    if profile.is_socket() {
        b = b
            .ip_or_hostname("localhost")
            .socket(Some(profile.host.clone()))
            .prefer_socket(true);
    } else {
        b = b
            .ip_or_hostname(if profile.host.is_empty() {
                "localhost".to_string()
            } else {
                profile.host.clone()
            })
            .tcp_port(if profile.port == 0 {
                DEFAULT_PORT
            } else {
                profile.port
            })
            // A local server answering on its socket would skip TLS and the port asked for.
            .prefer_socket(false);
    }
    b.into()
}

/// TLS as the profile asks: `Prefer` and `Require` encrypt without checking the certificate
/// (as MySQL's own `--ssl-mode` does), `VerifyCa` checks the chain, `VerifyFull` the host
/// name too.
fn ssl_opts(profile: &ConnProfile) -> Result<SslOpts> {
    let mut ssl = SslOpts::default();
    if let Some(ca) = &profile.ssl_root_cert {
        ssl = ssl.with_root_certs(vec![PathBuf::from(expand_home(ca)).into()]);
    }
    match (&profile.ssl_cert, &profile.ssl_key) {
        (Some(cert), Some(key)) => {
            ssl = ssl.with_client_identity(Some(ClientIdentity::new(
                PathBuf::from(expand_home(cert)).into(),
                PathBuf::from(expand_home(key)).into(),
            )));
        }
        (None, None) => {}
        _ => {
            return Err(CoreError::Config(
                "A client certificate needs both its certificate (sslcert) and its key (sslkey)."
                    .into(),
            ));
        }
    }
    Ok(match profile.ssl {
        SslMode::Disable | SslMode::Prefer | SslMode::Require => ssl
            .with_danger_accept_invalid_certs(true)
            .with_danger_skip_domain_validation(true),
        SslMode::VerifyCa => ssl.with_danger_skip_domain_validation(true),
        SslMode::VerifyFull => ssl,
    })
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
}

/// The server has no TLS (a MariaDB or MySQL built or set up without it).
fn tls_unsupported(e: &mysql_async::Error) -> bool {
    matches!(
        e,
        mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer)
    )
}

/// A statement's error: the server's code and message, as `mysql` prints them.
pub(crate) fn my_error(e: mysql_async::Error) -> CoreError {
    match e {
        mysql_async::Error::Server(s) => {
            CoreError::Sql(format!("ERROR {} ({}): {}", s.code, s.state, s.message))
        }
        other => CoreError::Sql(other.to_string()),
    }
}

/// A failed connect, sorted by what to do about it ([`FailureKind`]).
fn connect_error(e: mysql_async::Error) -> CoreError {
    let kind = match &e {
        mysql_async::Error::Server(s) => match s.code {
            // Access denied for the user, or to the database.
            1045 | 1044 | 1698 | 1251 => FailureKind::Auth,
            1049 => FailureKind::DatabaseMissing,
            // Too many connections, shutting down, host blocked for errors.
            1040 | 1053 | 1203 | 1129 => FailureKind::ServerBusy,
            1130 => FailureKind::Auth,
            _ => super::query::classify_message(&s.message),
        },
        mysql_async::Error::Io(mysql_async::IoError::Tls(_)) => FailureKind::Tls,
        mysql_async::Error::Io(_) => FailureKind::Network,
        mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer) => {
            FailureKind::Tls
        }
        other => super::query::classify_message(&other.to_string()),
    };
    let message = match (&e, kind) {
        (mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer), _) => {
            "The server doesn't offer TLS. Set sslmode to disable or prefer to connect without it."
                .to_string()
        }
        (mysql_async::Error::Server(s), _) => format!("{} (MySQL error {})", s.message, s.code),
        _ => e.to_string(),
    };
    CoreError::Connect { kind, message }
}

/// Quotes an identifier for MySQL: backticks, a backtick doubled.
pub fn quote(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// `schema`.`table`.
pub fn relation(schema: &str, table: &str) -> String {
    if schema.is_empty() {
        quote(table)
    } else {
        format!("{}.{}", quote(schema), quote(table))
    }
}

/// A string literal that reads the same whether or not the server's `sql_mode` has
/// `NO_BACKSLASH_ESCAPES`: quotes doubled, and a value with a backslash written in hex.
pub fn literal(value: &str) -> String {
    if value.contains('\\') || value.contains('\0') {
        let hex: String = value.bytes().map(|b| format!("{b:02X}")).collect();
        format!("CONVERT(X'{hex}' USING utf8mb4)")
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

/// A cell as the grid shows it: NULL is `None`; a binary string or BLOB is `\x` and hex (as
/// PostgreSQL's `bytea`); a BIT is its number; text and numbers as the server wrote them.
pub(crate) fn value_text(value: &Value, column: &Column) -> Option<String> {
    use mysql_async::consts::ColumnType as T;
    Some(match value {
        Value::NULL => return None,
        Value::Bytes(b) => {
            let binary_charset = column.character_set() == 63;
            match column.column_type() {
                T::MYSQL_TYPE_BIT => {
                    let n = b.iter().fold(0u128, |n, &x| (n << 8) | u128::from(x));
                    n.to_string()
                }
                T::MYSQL_TYPE_GEOMETRY => hex(b),
                T::MYSQL_TYPE_TINY_BLOB
                | T::MYSQL_TYPE_MEDIUM_BLOB
                | T::MYSQL_TYPE_LONG_BLOB
                | T::MYSQL_TYPE_BLOB
                | T::MYSQL_TYPE_VARCHAR
                | T::MYSQL_TYPE_VAR_STRING
                | T::MYSQL_TYPE_STRING
                    if binary_charset =>
                {
                    hex(b)
                }
                _ => String::from_utf8_lossy(b).into_owned(),
            }
        }
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Date(y, mo, d, h, mi, s, us) => {
            let mut text = format!("{y:04}-{mo:02}-{d:02}");
            if (*h, *mi, *s, *us) != (0, 0, 0, 0) {
                text.push_str(&format!(" {h:02}:{mi:02}:{s:02}"));
                if *us > 0 {
                    text.push_str(&format!(".{us:06}"));
                }
            }
            text
        }
        Value::Time(neg, days, h, mi, s, us) => {
            let hours = u64::from(*days) * 24 + u64::from(*h);
            let mut text = format!("{}{hours:02}:{mi:02}:{s:02}", if *neg { "-" } else { "" });
            if *us > 0 {
                text.push_str(&format!(".{us:06}"));
            }
            text
        }
    })
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(2 + 2 * bytes.len());
    text.push_str("\\x");
    for &x in bytes {
        text.push(char::from(HEX[usize::from(x >> 4)]));
        text.push(char::from(HEX[usize::from(x & 0xf)]));
    }
    text
}

#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;
