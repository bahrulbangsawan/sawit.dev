//! MySQL and MariaDB connection strings: `mysql://` URLs (and `mariadb://`, `jdbc:mysql://`,
//! SQLAlchemy's `mysql+pymysql://`), Go's `user:pass@tcp(host:3306)/db` DSN, and the TLS
//! parameters MySQL's tools, Prisma, PlanetScale, Go, JDBC and node's mysql2 write.

use super::fields::parse_error;
use super::url::{percent_decode, split_hosts, split_userinfo};
use super::{ConnProfile, SslMode};
use crate::Result;
use crate::db::percent_encode;

impl ConnProfile {
    /// `mysql://user@host:3306/db` (a socket as `?socket=`), without the password.
    pub(super) fn mysql_url(&self) -> String {
        let user = percent_encode(&self.user);
        let db = percent_encode(&self.database);
        if self.is_socket() {
            return format!(
                "mysql://{user}@localhost/{db}?socket={}",
                percent_encode(&self.host)
            );
        }
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("mysql://{user}@{host}:{}/{db}", self.port)
    }
}

/// What follows `mysql://`: `user:password@host:port/database?params`. Parameters that only
/// tune a driver (`charset`, `parseTime`, `connection_limit`) are left out; the TLS ones of
/// MySQL's tools, Prisma, PlanetScale, Go, JDBC and node's mysql2 are read.
pub(super) fn parse_mysql_url(rest: &str) -> Result<(ConnProfile, Option<String>)> {
    let (userinfo, rest) = split_userinfo(rest);
    let (user, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((user, pw)) => (percent_decode(user), Some(percent_decode(pw))),
            None => (percent_decode(u), None),
        },
        None => (String::new(), None),
    };
    let rest = rest.split('#').next().unwrap_or_default();
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (hostlist, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (hosts, ports) = split_hosts(hostlist)?;
    let host = hosts.first().cloned().unwrap_or_default();
    let port = match ports.first().map(String::as_str) {
        None | Some("") => crate::db::mysql::DEFAULT_PORT,
        Some(p) => p
            .parse()
            .ok()
            .filter(|n: &u16| *n > 0)
            .ok_or_else(|| parse_error(format!("“{p}” isn't a port number.")))?,
    };
    let host = if host.is_empty() { "localhost" } else { &host };
    let database = percent_decode(path.trim_end_matches('/'));
    mysql_profile(host, port, &database, &user, password, query)
}

/// The profile of a MySQL URL or Go DSN, its `params` (`key=value&…`) applied.
fn mysql_profile(
    host: &str,
    port: u16,
    database: &str,
    user: &str,
    password: Option<String>,
    params: &str,
) -> Result<(ConnProfile, Option<String>)> {
    let mut profile = ConnProfile::mysql(host, port, database, user);
    let mut password = password;
    for pair in params.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        apply_mysql_param(
            &mut profile,
            &mut password,
            &percent_decode(key),
            &percent_decode(value),
        )?;
    }
    profile.name = format!(
        "{}@{}:{}/{}",
        profile.user, profile.host, profile.port, profile.database
    );
    Ok((profile, password.filter(|p| !p.is_empty())))
}

/// One `key=value` of a MySQL URL or Go DSN.
fn apply_mysql_param(
    profile: &mut ConnProfile,
    password: &mut Option<String>,
    key: &str,
    value: &str,
) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    let truthy = matches!(lower.as_str(), "true" | "1" | "yes" | "on");
    match key.to_ascii_lowercase().replace('-', "_").as_str() {
        // MySQL's `--ssl-mode`, libpq's `sslmode`, JDBC's `sslMode`.
        "ssl_mode" | "sslmode" => {
            profile.ssl = match lower.replace('-', "_").as_str() {
                "disabled" | "disable" | "false" => SslMode::Disable,
                "preferred" | "prefer" | "allow" => SslMode::Prefer,
                "required" | "require" | "true" => SslMode::Require,
                "verify_ca" => SslMode::VerifyCa,
                "verify_identity" | "verify_full" => SslMode::VerifyFull,
                _ => {
                    return Err(parse_error(format!(
                        "“{value}” isn't an ssl-mode. Use DISABLED, PREFERRED, REQUIRED, VERIFY_CA or VERIFY_IDENTITY."
                    )));
                }
            }
        }
        // Prisma and PlanetScale: `sslaccept=strict` checks the certificate.
        "sslaccept" => {
            profile.ssl = if lower == "strict" {
                SslMode::VerifyFull
            } else {
                SslMode::Require
            }
        }
        // node's mysql2 and PlanetScale: `ssl={"rejectUnauthorized":true}`, `ssl=true`.
        "ssl" => {
            let squeezed: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
            profile.ssl = if squeezed.contains("\"rejectunauthorized\":false") {
                SslMode::Require
            } else if squeezed.starts_with('{') || squeezed.contains("amazon") {
                SslMode::VerifyFull
            } else if truthy {
                SslMode::Require
            } else if matches!(lower.as_str(), "false" | "0") {
                SslMode::Disable
            } else {
                profile.ssl
            }
        }
        // Go's `tls=true|skip-verify|preferred|false`.
        "tls" => {
            profile.ssl = match lower.as_str() {
                "true" => SslMode::VerifyFull,
                "skip-verify" | "skip_verify" => SslMode::Require,
                "preferred" => SslMode::Prefer,
                "false" => SslMode::Disable,
                _ => profile.ssl,
            }
        }
        // JDBC's `useSSL`, `requireSSL`, `verifyServerCertificate`.
        "usessl" if !truthy => profile.ssl = SslMode::Disable,
        "requiressl" if truthy => profile.ssl = SslMode::Require,
        "verifyservercertificate" if truthy => profile.ssl = SslMode::VerifyFull,
        "ssl_ca" | "sslca" | "sslrootcert" | "ssl_root_cert" => {
            profile.ssl_root_cert = Some(value.to_string());
            if profile.ssl == SslMode::Prefer {
                profile.ssl = SslMode::VerifyCa;
            }
        }
        "ssl_cert" | "sslcert" => profile.ssl_cert = Some(value.to_string()),
        "ssl_key" | "sslkey" => profile.ssl_key = Some(value.to_string()),
        "connect_timeout" | "connecttimeout" | "timeout" => {
            if let Ok(n) = value.trim_end_matches('s').parse::<u64>() {
                // JDBC and mysql2 count milliseconds.
                profile.connect_timeout = Some(if n > 1000 { n / 1000 } else { n });
            }
        }
        "socket" | "unix_socket" | "socketpath" => profile.host = value.to_string(),
        "user" | "username" => profile.user = value.to_string(),
        "password" => *password = Some(value.to_string()),
        "database" | "dbname" | "db" => profile.database = value.to_string(),
        _ => {}
    }
    Ok(())
}

/// Go's MySQL DSN, `user:password@tcp(host:3306)/database?params` (also `unix(/path)`), when
/// `text` is one.
pub(super) fn go_mysql_dsn(text: &str) -> Option<Result<(ConnProfile, Option<String>)>> {
    let (userinfo, after, socket) = if let Some(i) = text.rfind("@tcp(") {
        (&text[..i], &text[i + 5..], false)
    } else if let Some(i) = text.rfind("@unix(") {
        (&text[..i], &text[i + 6..], true)
    } else if let Some(rest) = text.strip_prefix("tcp(") {
        ("", rest, false)
    } else {
        ("", text.strip_prefix("unix(")?, true)
    };
    let (addr, rest) = after.split_once(')')?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (user, password) = match userinfo.split_once(':') {
        Some((u, p)) => (u.to_string(), Some(p.to_string())),
        None => (userinfo.to_string(), None),
    };
    let (host, port) = match addr.rsplit_once(':') {
        Some((h, p)) if !socket && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (
            h.trim_matches(['[', ']']).to_string(),
            p.parse().unwrap_or(crate::db::mysql::DEFAULT_PORT),
        ),
        _ => (addr.to_string(), crate::db::mysql::DEFAULT_PORT),
    };
    let host = if host.is_empty() {
        "localhost".to_string()
    } else {
        host
    };
    Some(mysql_profile(&host, port, path, &user, password, query))
}
