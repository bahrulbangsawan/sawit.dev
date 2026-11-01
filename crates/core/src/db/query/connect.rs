//! Opening connections: the driver's settings from a profile, the session settings Sawit adds
//! (dropped behind a pooler), cancelling a running query.

use std::time::Duration;

use tokio_postgres::Client;
use tokio_postgres::config::{
    ChannelBinding as PgChannelBinding, SslMode as PgSslMode, TargetSessionAttrs,
};

use super::failure::{classify, sql_error, with_causes};
use super::schema::{SchemaInfo, load_schema};
use crate::db::profile::{ChannelBinding, ConnProfile, SessionAttrs, SslMode};
use crate::{CoreError, Result};

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
        .connect(crate::db::tls::connector(profile)?)
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
        .cancel_query(crate::db::tls::connector(profile)?)
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
