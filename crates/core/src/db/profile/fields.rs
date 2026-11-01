//! Connection settings gathered from a URL or a keyword/value string: what each parameter means
//! (libpq's names and the JDBC, node-postgres and Prisma spellings), and the connection service
//! file's sections.

use super::{ChannelBinding, ConnProfile, HostPort, SessionAttrs, SslMode};
use crate::{CoreError, Result};

/// Connection settings gathered from a URL or a keyword/value string.
#[derive(Default)]
pub(super) struct Fields {
    hosts: Vec<String>,
    ports: Vec<String>,
    hostaddr: Option<String>,
    user: Option<String>,
    password: Option<String>,
    database: Option<String>,
    sslmode: Option<SslMode>,
    /// TLS asked for another way (`ssl=true`, Prisma's `sslaccept`, JDBC's
    /// `sslfactory`); `sslmode` wins over it.
    ssl_hint: Option<SslMode>,
    ssl_root_cert: Option<String>,
    system_roots: bool,
    ssl_cert: Option<String>,
    ssl_key: Option<String>,
    options: Option<String>,
    connect_timeout: Option<u64>,
    channel_binding: Option<ChannelBinding>,
    target_session_attrs: Option<SessionAttrs>,
    passfile: Option<String>,
}

pub(super) fn parse_error(message: String) -> CoreError {
    CoreError::Parse(message)
}

impl Fields {
    /// Applies one parameter (libpq names, plus the JDBC, node-postgres and
    /// Prisma spellings people paste). Unknown ones (`application_name`,
    /// Prisma's `schema`, `pgbouncer`, …) are ignored. An empty value
    /// leaves the setting as it was.
    fn set(&mut self, key: &str, value: String) -> Result<()> {
        if value.is_empty() {
            return Ok(());
        }
        let list = |v: &str| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>()
        };
        match key {
            "host" => self.hosts = list(&value),
            "hostaddr" => self.hostaddr = Some(value),
            "port" => self.ports = list(&value),
            "user" | "username" => self.user = Some(value),
            "password" => self.password = Some(value),
            "dbname" | "database" => self.database = Some(value),
            "sslmode" => {
                self.sslmode = Some(SslMode::parse(&value).ok_or_else(|| {
                    parse_error(format!(
                        "Unknown sslmode “{value}”. Use disable, prefer, require, verify-ca or verify-full."
                    ))
                })?)
            }
            "ssl" | "requiressl" => {
                self.ssl_hint = Some(match value.to_ascii_lowercase().as_str() {
                    "true" | "1" | "on" | "yes" | "require" => SslMode::Require,
                    "false" | "0" | "off" | "no" => SslMode::Disable,
                    other => {
                        return Err(parse_error(format!(
                            "Unknown {key} value “{other}”. Use true or false."
                        )));
                    }
                })
            }
            "sslaccept" => {
                self.ssl_hint = Some(match value.as_str() {
                    "strict" => SslMode::VerifyFull,
                    _ => SslMode::Require,
                })
            }
            "sslfactory" if value.ends_with("NonValidatingFactory") => {
                self.ssl_hint = Some(SslMode::Require)
            }
            "sslrootcert" if value == "system" => {
                self.system_roots = true;
                self.ssl_root_cert = None;
            }
            "sslrootcert" => self.ssl_root_cert = Some(expand_home(value)),
            "sslcert" => self.ssl_cert = Some(expand_home(value)),
            "sslkey" => self.ssl_key = Some(expand_home(value)),
            "options" => self.options = Some(value),
            "connect_timeout" | "connectTimeout" | "loginTimeout" => {
                let secs: u64 = value.parse().map_err(|_| {
                    parse_error(format!("{key} “{value}” isn't a number of seconds."))
                })?;
                // libpq: 0 means wait forever; keep Sawit's default instead.
                self.connect_timeout = (secs > 0).then_some(secs.max(2));
            }
            "channel_binding" => {
                self.channel_binding = Some(match value.as_str() {
                    "disable" => ChannelBinding::Disable,
                    "prefer" => ChannelBinding::Prefer,
                    "require" => ChannelBinding::Require,
                    other => {
                        return Err(parse_error(format!(
                            "Unknown channel_binding “{other}”. Use disable, prefer or require."
                        )));
                    }
                })
            }
            "target_session_attrs" | "targetServerType" => {
                self.target_session_attrs = Some(match value.as_str() {
                    "any" | "prefer-standby" | "preferSecondary" | "preferStandby" => {
                        SessionAttrs::Any
                    }
                    "read-write" | "primary" | "master" => SessionAttrs::ReadWrite,
                    "read-only" | "standby" | "secondary" => SessionAttrs::ReadOnly,
                    other => {
                        return Err(parse_error(format!(
                            "Unknown target_session_attrs “{other}”. Use any, read-write or read-only."
                        )));
                    }
                })
            }
            "passfile" => self.passfile = Some(expand_home(value)),
            _ => {}
        }
        Ok(())
    }

    /// Applies `pairs` in order, after the `service` they name (if any), so
    /// explicit settings win over the service file, as in libpq.
    pub(super) fn apply(&mut self, pairs: Vec<(String, String)>) -> Result<()> {
        if let Some((_, name)) = pairs.iter().find(|(k, _)| k == "service") {
            for (k, v) in service_params(name)? {
                self.set(&k, v)?;
            }
        }
        for (k, v) in pairs {
            if k != "service" {
                self.set(&k, v)?;
            }
        }
        Ok(())
    }

    pub(super) fn into_profile(self) -> Result<(ConnProfile, Option<String>)> {
        let mut hosts = self.hosts;
        if hosts.is_empty() {
            hosts.push(self.hostaddr.clone().unwrap_or_else(|| "localhost".into()));
        }
        if hosts.iter().any(String::is_empty) {
            return Err(parse_error("One of the hosts is empty.".into()));
        }
        let port_of = |p: &str| -> Result<u16> {
            match p {
                "" => Ok(5432),
                p => p.parse::<u16>().ok().filter(|n| *n > 0).ok_or_else(|| {
                    parse_error(format!("Port “{p}” isn't a port number (1–65535)."))
                }),
            }
        };
        let ports: Vec<u16> = match self.ports.len() {
            0 => vec![5432; hosts.len()],
            1 => vec![port_of(&self.ports[0])?; hosts.len()],
            n if n == hosts.len() => self
                .ports
                .iter()
                .map(|p| port_of(p))
                .collect::<Result<_>>()?,
            n => {
                return Err(parse_error(format!(
                    "{} hosts but {n} ports; give one port, or one per host.",
                    hosts.len()
                )));
            }
        };
        let user = self
            .user
            .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "postgres".into()));
        let database = self.database.unwrap_or_else(|| user.clone());
        let ssl = self
            .sslmode
            .or(self.ssl_hint)
            .unwrap_or(if self.system_roots {
                // libpq 16: `sslrootcert=system` defaults to verify-full.
                SslMode::VerifyFull
            } else {
                SslMode::Prefer
            });
        let mut pairs = hosts.into_iter().zip(ports);
        let (host, port) = pairs.next().expect("at least one host");
        let profile = ConnProfile {
            ssl,
            ssl_root_cert: self.ssl_root_cert,
            ssl_cert: self.ssl_cert,
            ssl_key: self.ssl_key,
            fallback_hosts: pairs.map(|(host, port)| HostPort { host, port }).collect(),
            options: self.options,
            connect_timeout: self.connect_timeout,
            channel_binding: self.channel_binding,
            target_session_attrs: self.target_session_attrs,
            passfile: self.passfile,
            ..ConnProfile::new(&host, port, &database, &user)
        };
        Ok((profile, self.password))
    }
}

fn expand_home(path: String) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path,
    }
}

/// The settings of `[name]` in the connection service file
/// (`$PGSERVICEFILE`, else `~/.pg_service.conf`, else
/// `$PGSYSCONFDIR/pg_service.conf`).
fn service_params(name: &str) -> Result<Vec<(String, String)>> {
    let candidates = [
        std::env::var("PGSERVICEFILE").ok(),
        std::env::var("HOME")
            .ok()
            .map(|h| format!("{h}/.pg_service.conf")),
        std::env::var("PGSYSCONFDIR")
            .ok()
            .map(|d| format!("{d}/pg_service.conf")),
    ];
    for path in candidates.into_iter().flatten() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(params) = service_section(&text, name) {
            return Ok(params);
        }
    }
    Err(parse_error(format!(
        "Service “{name}” isn't defined in ~/.pg_service.conf."
    )))
}

/// `key=value` lines of section `[name]` in pg_service.conf text.
pub(in crate::db) fn service_section(text: &str, name: &str) -> Option<Vec<(String, String)>> {
    let mut found = None;
    let mut current: Option<&str> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(section.trim());
            if current == Some(name) {
                found.get_or_insert_with(Vec::new);
            }
            continue;
        }
        if current == Some(name)
            && let Some((k, v)) = line.split_once('=')
        {
            found
                .get_or_insert_with(Vec::new)
                .push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    found
}
