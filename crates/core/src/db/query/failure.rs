//! Why a statement or a connect failed: server errors with their detail, and failed connects
//! sorted into kinds that say whether retrying can help.

use crate::CoreError;

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

pub(super) fn classify(e: &tokio_postgres::Error) -> FailureKind {
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
pub(super) fn with_causes(e: &tokio_postgres::Error) -> String {
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

pub(in crate::db) fn classify_message(message: &str) -> FailureKind {
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
