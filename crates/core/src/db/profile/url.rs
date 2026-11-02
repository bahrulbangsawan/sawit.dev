//! Connection strings: a profile written as a URL, and every common form people paste read
//! back into one.

use super::fields::{Fields, parse_error};
use super::{ChannelBinding, ConnProfile, SessionAttrs, SslMode};
use crate::db::percent_encode;
use crate::{CoreError, Result};

impl ConnProfile {
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
        params.extend(
            self.connect_timeout
                .map(|v| ("connect_timeout", v.to_string())),
        );
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
