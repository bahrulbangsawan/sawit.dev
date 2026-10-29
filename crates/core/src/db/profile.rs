//! Connection profiles (TOML, never containing passwords), DATABASE_URL and
//! .env parsing, and Keychain-stored passwords.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result, keychain};

/// TLS policy, as libpq's `sslmode`. `Prefer` and `Require` encrypt without
/// checking the certificate (like libpq); `VerifyCa` checks the chain and
/// `VerifyFull` also the host name, against `sslrootcert` when given, else
/// the system trust store.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    pub fn parse(value: &str) -> Option<SslMode> {
        Some(match value {
            "disable" => SslMode::Disable,
            // libpq's `allow` tries plain first, then TLS; `prefer` reaches
            // the same servers.
            "allow" | "prefer" => SslMode::Prefer,
            // node-postgres: encrypt, don't verify.
            "require" | "no-verify" => SslMode::Require,
            "verify-ca" => SslMode::VerifyCa,
            "verify-full" => SslMode::VerifyFull,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnProfile {
    /// Identifies the profile (tabs, state, `sawit-mcp`); never changes
    /// once saved.
    pub name: String,
    /// What the sidebar shows instead of `name`, set in Connection Settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// Absent in profiles saved by older versions.
    #[serde(default)]
    pub ssl: SslMode,
    /// `sslrootcert`: PEM file of the CA(s) the server certificate must chain
    /// to (Cloud SQL's `server-ca.pem`, the RDS bundle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl_root_cert: Option<String>,
    /// `sslcert` / `sslkey`: client certificate and key (PEM), for servers
    /// that require one (Cloud SQL "trusted client certificates").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl_cert: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl_key: Option<String>,
    /// More servers to try, in order, when `host` doesn't answer (libpq's
    /// `host=a,b` / `postgres://a:5432,b:5433/db`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_hosts: Vec<HostPort>,
    /// libpq `options`, sent along with Sawit's own (`-c search_path=app`,
    /// Neon's `endpoint=ep-…`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
    /// Seconds to wait for the server (libpq `connect_timeout`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_binding: Option<ChannelBinding>,
    /// With several hosts: which kind of server to settle on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session_attrs: Option<SessionAttrs>,
    /// A `.pgpass` file to look the password up in (libpq `passfile`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passfile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostPort {
    pub host: String,
    pub port: u16,
}

/// libpq `channel_binding` (SCRAM tied to the TLS session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelBinding {
    Disable,
    Prefer,
    Require,
}

/// libpq `target_session_attrs`, reduced to what the driver can check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionAttrs {
    Any,
    ReadWrite,
    ReadOnly,
}

impl ConnProfile {
    pub fn new(host: &str, port: u16, database: &str, user: &str) -> ConnProfile {
        ConnProfile {
            name: format!("{user}@{host}:{port}/{database}"),
            label: None,
            host: host.into(),
            port,
            database: database.into(),
            user: user.into(),
            ssl: SslMode::default(),
            ssl_root_cert: None,
            ssl_cert: None,
            ssl_key: None,
            fallback_hosts: Vec::new(),
            options: None,
            connect_timeout: None,
            channel_binding: None,
            target_session_attrs: None,
            passfile: None,
        }
    }

    /// The host is a Unix-socket directory (Cloud SQL's `/cloudsql/…`).
    pub fn is_socket(&self) -> bool {
        self.host.starts_with('/')
    }

    /// The custom label, else the profile's name.
    pub fn display_name(&self) -> &str {
        self.label
            .as_deref()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or(&self.name)
    }

    /// [`url`](Self::url) plus every setting that differs from the default,
    /// so [`parse_database_url`] gives this profile back (password aside).
    pub fn connection_string(&self) -> String {
        let mut params: Vec<(&str, String)> = Vec::new();
        let ssl = match self.ssl {
            SslMode::Disable => Some("disable"),
            SslMode::Prefer => None,
            SslMode::Require => Some("require"),
            SslMode::VerifyCa => Some("verify-ca"),
            SslMode::VerifyFull => Some("verify-full"),
        };
        params.extend(ssl.map(|v| ("sslmode", v.to_string())));
        params.extend(self.ssl_root_cert.clone().map(|v| ("sslrootcert", v)));
        params.extend(self.ssl_cert.clone().map(|v| ("sslcert", v)));
        params.extend(self.ssl_key.clone().map(|v| ("sslkey", v)));
        params.extend(self.options.clone().map(|v| ("options", v)));
        params.extend(self.connect_timeout.map(|v| ("connect_timeout", v.to_string())));
        params.extend(self.channel_binding.map(|v| {
            let v = match v {
                ChannelBinding::Disable => "disable",
                ChannelBinding::Prefer => "prefer",
                ChannelBinding::Require => "require",
            };
            ("channel_binding", v.to_string())
        }));
        params.extend(self.target_session_attrs.map(|v| {
            let v = match v {
                SessionAttrs::Any => "any",
                SessionAttrs::ReadWrite => "read-write",
                SessionAttrs::ReadOnly => "read-only",
            };
            ("target_session_attrs", v.to_string())
        }));
        params.extend(self.passfile.clone().map(|v| ("passfile", v)));
        let mut url = self.url();
        for (key, value) in params {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&format!("{key}={}", percent_encode(&value)));
        }
        url
    }

    /// Keychain account for this profile's password.
    pub fn keychain_account(&self) -> String {
        format!(
            "{}@{}:{}/{}",
            self.user, self.host, self.port, self.database
        )
    }

    /// A URL without the password, for display and `.env` files.
    pub fn url(&self) -> String {
        if self.is_socket() {
            return format!(
                "postgres://{}@:{}/{}?host={}",
                percent_encode(&self.user),
                self.port,
                self.database,
                percent_encode(&self.host)
            );
        }
        let host = |h: &str, port: u16| {
            if h.contains(':') {
                format!("[{h}]:{port}")
            } else {
                format!("{h}:{port}")
            }
        };
        let hosts: Vec<String> = std::iter::once(host(&self.host, self.port))
            .chain(self.fallback_hosts.iter().map(|h| host(&h.host, h.port)))
            .collect();
        format!(
            "postgres://{}@{}/{}",
            percent_encode(&self.user),
            hosts.join(","),
            self.database
        )
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (
            bytes[i],
            bytes.get(i + 1).and_then(|b| hex(*b)),
            bytes.get(i + 2).and_then(|b| hex(*b)),
        ) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(hi << 4 | lo);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

/// Connection settings gathered from a URL or a keyword/value string.
#[derive(Default)]
struct Fields {
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

fn parse_error(message: String) -> CoreError {
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
    fn apply(&mut self, pairs: Vec<(String, String)>) -> Result<()> {
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

    fn into_profile(self) -> Result<(ConnProfile, Option<String>)> {
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
pub(super) fn service_section(text: &str, name: &str) -> Option<Vec<(String, String)>> {
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

/// What people paste from cloud consoles, reduced to the connection string:
/// `psql 'postgresql://…'` (Neon), `DATABASE_URL="…"`, surrounding quotes.
fn unwrap_pasted(input: &str) -> &str {
    let mut s = input.trim();
    if let Some(rest) = s.strip_prefix("psql ") {
        s = rest.trim();
    }
    if let Some(rest) = s.strip_prefix("export ") {
        s = rest.trim();
    }
    if let Some((key, rest)) = s.split_once('=')
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !key.is_empty()
    {
        s = rest.trim();
    }
    for q in ['\'', '"', '`'] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            s = inner.trim();
        }
    }
    s
}

/// URL schemes that mean PostgreSQL, after dropping a `jdbc:` prefix and a
/// `+driver` suffix (`postgresql+asyncpg`, Django's `postgis`).
const PG_SCHEMES: [&str; 4] = ["postgres", "postgresql", "pgsql", "postgis"];

/// Why a URL for something else can't be used, in plain words.
fn other_database(scheme: &str) -> Option<&'static str> {
    Some(match scheme {
        "mysql" | "mariadb" => "This is a MySQL URL; Sawit connects to PostgreSQL.",
        "mongodb" | "mongodb+srv" => "This is a MongoDB URL; Sawit connects to PostgreSQL.",
        "redis" | "rediss" => "This is a Redis URL; Sawit connects to PostgreSQL.",
        "sqlite" | "file" => "This is a SQLite path; Sawit connects to PostgreSQL.",
        "sqlserver" | "mssql" => "This is a SQL Server URL; Sawit connects to PostgreSQL.",
        "prisma" | "prisma+postgres" => {
            "This is a Prisma Accelerate URL. Use the direct database URL (often DIRECT_URL) instead."
        }
        _ => return None,
    })
}

/// Splits `user:password@` off what follows `scheme://`. Passwords pasted
/// without percent-encoding may contain `@ / ? #`, so the split is at the
/// last `@` that isn't inside the query string.
fn split_userinfo(rest: &str) -> (Option<&str>, &str) {
    let in_query = |prefix: &str| {
        prefix.match_indices('?').any(|(i, _)| {
            let after = &prefix[i + 1..];
            let key: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            !key.is_empty() && after[key.len()..].starts_with('=')
        })
    };
    for (at, _) in rest.rmatch_indices('@') {
        if !in_query(&rest[..at]) {
            return (Some(&rest[..at]), &rest[at + 1..]);
        }
    }
    (None, rest)
}

/// `host[:port]` items of a URL's host list (`h1:5432,[::1]:5433,h3`). A
/// port is only split off when it is all digits, so Cloud SQL socket paths
/// (`%2Fcloudsql%2Fproj:region:inst`) stay whole.
fn split_hosts(list: &str) -> Result<(Vec<String>, Vec<String>)> {
    let (mut hosts, mut ports) = (Vec::new(), Vec::new());
    for item in list.split(',') {
        let (host, port) = if let Some(v6) = item.strip_prefix('[') {
            let (h, after) = v6
                .split_once(']')
                .ok_or_else(|| parse_error(format!("“{item}” is missing its closing ].")))?;
            (h, after.strip_prefix(':').unwrap_or(""))
        } else {
            match item.rsplit_once(':') {
                Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, p),
                _ => (item, ""),
            }
        };
        hosts.push(percent_decode(host));
        ports.push(port.to_string());
    }
    // `postgres:///db` and `postgres://:5433/db` mean the default host.
    if hosts.iter().all(String::is_empty) {
        hosts.clear();
    }
    if ports.iter().all(String::is_empty) {
        ports.clear();
    }
    Ok((hosts, ports))
}

/// Parses a connection string in any common form:
/// - URLs: `postgres://` / `postgresql://` (libpq), `jdbc:postgresql://`
///   (Java), `postgresql+psycopg2://` and other SQLAlchemy drivers,
///   `pgsql://` / `postgis://` (Django, PHP), with several hosts
///   (`a:5432,b:5433`), IPv6 (`[::1]`), socket paths, unencoded passwords,
///   and the TLS/timeout parameters of libpq, node-postgres, Prisma and JDBC;
/// - libpq keyword/value pairs (`host=… dbname=… user=…`);
/// - `service=name` from `~/.pg_service.conf`;
/// - whatever wraps them when copied: `psql '…'`, `DATABASE_URL="…"`.
///
/// Query parameters override the URL's parts, as in libpq, so Cloud SQL's
/// `postgres://u:p@/db?host=/cloudsql/project:region:instance` works.
/// Returns the profile and the password (if any), which callers should put
/// in the Keychain.
pub fn parse_database_url(input: &str) -> Result<(ConnProfile, Option<String>)> {
    let url = unwrap_pasted(input);
    if url.is_empty() {
        return Err(parse_error("Paste a connection string first.".into()));
    }
    let Some((scheme, rest)) = url.split_once("://").or_else(|| {
        // `jdbc:postgresql:dbname` (no host).
        url.strip_prefix("jdbc:postgresql:")
            .map(|db| ("postgresql", db))
    }) else {
        if url.contains('=') {
            return parse_keyword_value(url);
        }
        return Err(parse_error(format!(
            "Not a PostgreSQL connection string: {url}. Use postgres://user:password@host:5432/database."
        )));
    };
    let scheme = scheme.to_ascii_lowercase();
    let scheme = scheme.strip_prefix("jdbc:").unwrap_or(&scheme);
    let base = scheme.split('+').next().unwrap_or_default();
    if !PG_SCHEMES.contains(&base) || scheme.starts_with("prisma") {
        return Err(parse_error(match other_database(scheme) {
            Some(why) => why.to_string(),
            None => format!(
                "“{scheme}://” isn't a PostgreSQL URL. Use postgres://user:password@host:5432/database."
            ),
        }));
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    let (userinfo, rest) = split_userinfo(rest);
    if let Some(u) = userinfo {
        let (user, pw) = match u.split_once(':') {
            Some((user, pw)) => (user, Some(pw)),
            None => (u, None),
        };
        pairs.push(("user".into(), percent_decode(user)));
        if let Some(pw) = pw {
            pairs.push(("password".into(), percent_decode(pw)));
        }
    }
    let rest = rest.split('#').next().unwrap_or_default();
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (hostlist, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (hosts, ports) = split_hosts(hostlist)?;
    pairs.push(("host".into(), hosts.join(",")));
    pairs.push(("port".into(), ports.join(",")));
    pairs.push(("dbname".into(), percent_decode(path)));
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        // `+` stays a plus: libpq doesn't treat it as a space.
        pairs.push((percent_decode(key), percent_decode(value)));
    }
    let mut f = Fields::default();
    f.apply(pairs)?;
    f.into_profile()
}

/// libpq's `key=value key='quoted value'` form (what Cloud SQL and many
/// docs show).
fn parse_keyword_value(text: &str) -> Result<(ConnProfile, Option<String>)> {
    let mut pairs = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let key: String =
            std::iter::from_fn(|| chars.next_if(|c| *c != '=' && !c.is_whitespace())).collect();
        if key.is_empty() {
            break;
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.next() != Some('=') {
            return Err(CoreError::Parse(format!(
                "Add “=” and a value after “{key}”."
            )));
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let mut value = String::new();
        if chars.next_if_eq(&'\'').is_some() {
            loop {
                match chars.next() {
                    Some('\\') => value.extend(chars.next()),
                    Some('\'') => break,
                    Some(c) => value.push(c),
                    None => {
                        return Err(CoreError::Parse(
                            "A quoted value is missing its closing quote.".into(),
                        ));
                    }
                }
            }
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                if c == '\\' {
                    value.extend(chars.next());
                } else {
                    value.push(c);
                }
            }
        }
        pairs.push((key, value));
    }
    let mut f = Fields::default();
    f.apply(pairs)?;
    f.into_profile()
}

/// The password for `profile` in a `.pgpass` file (`passfile`, else
/// `$PGPASSFILE`, else `~/.pgpass`), matched like libpq: `*` matches
/// anything, `localhost` also matches socket connections, `\:` and `\\`
/// are escapes, and a file readable by others is ignored.
pub fn pgpass_password(profile: &ConnProfile) -> Option<String> {
    let path = profile
        .passfile
        .clone()
        .or_else(|| std::env::var("PGPASSFILE").ok())
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.pgpass")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&path).ok()?.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    pgpass_lookup(&std::fs::read_to_string(path).ok()?, profile)
}

pub(super) fn pgpass_lookup(text: &str, profile: &ConnProfile) -> Option<String> {
    let host = if profile.is_socket() {
        "localhost"
    } else {
        profile.host.as_str()
    };
    let port = profile.port.to_string();
    let wanted = [
        host,
        port.as_str(),
        profile.database.as_str(),
        profile.user.as_str(),
    ];
    text.lines().find_map(|line| {
        if line.trim_start().starts_with('#') {
            return None;
        }
        let mut fields = vec![String::new()];
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => fields.last_mut()?.extend(chars.next()),
                ':' if fields.len() < 5 => fields.push(String::new()),
                c => fields.last_mut()?.push(c),
            }
        }
        if fields.len() != 5 {
            return None;
        }
        let matches = fields[..4]
            .iter()
            .zip(wanted)
            .all(|(field, want)| field == "*" || field == want);
        matches.then(|| fields[4].clone())
    })
}

/// The saved password: the Keychain first, then `.pgpass`.
pub fn resolve_password(profile: &ConnProfile) -> Option<String> {
    load_password(profile)
        .ok()
        .flatten()
        .or_else(|| pgpass_password(profile))
}

/// Value of `key` in .env text (`KEY=v`, `export KEY="v"`, `KEY='v'`, with
/// trailing `# comments` on unquoted values).
pub fn dotenv_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let (k, v) = line.split_once('=')?;
        if k.trim() != key {
            return None;
        }
        let v = v.trim();
        let value = if let Some(q) = v.strip_prefix('"') {
            q.split_once('"').map(|(inner, _)| inner.to_string())?
        } else if let Some(q) = v.strip_prefix('\'') {
            q.split_once('\'').map(|(inner, _)| inner.to_string())?
        } else {
            v.split(" #").next().unwrap_or_default().trim().to_string()
        };
        Some(value)
    })
}

/// Sets `key=value` in .env text: replaces the first active assignment or appends.
pub fn dotenv_upsert(text: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let is_key = |l: &str| {
        let l = l.trim();
        let l = l.strip_prefix("export ").unwrap_or(l).trim_start();
        l.split_once('=').is_some_and(|(k, _)| k.trim() == key)
    };
    match lines.iter().position(|l| is_key(l)) {
        Some(i) => lines[i] = format!("{key}={value}"),
        None => lines.push(format!("{key}={value}")),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Reads `DATABASE_URL` from a .env file.
pub fn read_dotenv_database_url(path: &Path) -> Result<Option<String>> {
    Ok(dotenv_value(
        &std::fs::read_to_string(path)?,
        "DATABASE_URL",
    ))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profiles {
    #[serde(default, rename = "profile")]
    pub items: Vec<ConnProfile>,
}

impl Profiles {
    pub fn load(path: &Path) -> Result<Profiles> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| CoreError::Config(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Profiles::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| CoreError::Config(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Adds or replaces the profile with the same name.
    pub fn upsert(&mut self, profile: ConnProfile) {
        match self.items.iter_mut().find(|p| p.name == profile.name) {
            Some(existing) => *existing = profile,
            None => self.items.push(profile),
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.items.retain(|p| p.name != name);
    }
}

const KEYCHAIN_SERVICE: &str = "sawit";
/// The service passwords were saved under before the rename to Sawit (PG
/// Desk); they move to `KEYCHAIN_SERVICE` when first read.
const LEGACY_KEYCHAIN_SERVICE: &str = "pgdesk";
pub fn store_password(profile: &ConnProfile, password: &str) -> Result<()> {
    keychain::set(KEYCHAIN_SERVICE, &profile.keychain_account(), password)
}

pub fn load_password(profile: &ConnProfile) -> Result<Option<String>> {
    let account = profile.keychain_account();
    if let Some(password) = keychain::get(KEYCHAIN_SERVICE, &account)? {
        return Ok(Some(password));
    }
    let Some(password) = keychain::get(LEGACY_KEYCHAIN_SERVICE, &account)? else {
        return Ok(None);
    };
    if store_password(profile, &password).is_ok() {
        let _ = keychain::delete(LEGACY_KEYCHAIN_SERVICE, &account);
    }
    Ok(Some(password))
}

pub fn delete_password(profile: &ConnProfile) -> Result<()> {
    let account = profile.keychain_account();
    // Not migrated yet; it must not come back on the next read.
    let _ = keychain::delete(LEGACY_KEYCHAIN_SERVICE, &account);
    keychain::delete(KEYCHAIN_SERVICE, &account)
}
