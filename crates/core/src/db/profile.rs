//! Connection profiles (TOML, never containing passwords), DATABASE_URL and
//! .env parsing, and Keychain-stored passwords.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result};

/// TLS policy, as libpq's `sslmode`. `Prefer` and `Require` encrypt without
/// checking the certificate (like libpq); `VerifyFull` checks it against the
/// system trust store (`verify-ca` is treated as `verify-full`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyFull,
}

impl SslMode {
    pub fn parse(value: &str) -> Option<SslMode> {
        Some(match value {
            "disable" | "allow" => SslMode::Disable,
            "prefer" => SslMode::Prefer,
            "require" => SslMode::Require,
            "verify-ca" | "verify-full" => SslMode::VerifyFull,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnProfile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// Absent in profiles saved by older versions.
    #[serde(default)]
    pub ssl: SslMode,
}

impl ConnProfile {
    pub fn new(host: &str, port: u16, database: &str, user: &str) -> ConnProfile {
        ConnProfile {
            name: format!("{user}@{host}:{port}/{database}"),
            host: host.into(),
            port,
            database: database.into(),
            user: user.into(),
            ssl: SslMode::default(),
        }
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
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!(
            "postgres://{}@{}:{}/{}",
            percent_encode(&self.user),
            host,
            self.port,
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

/// Parses `postgres://user:pass@host:port/db?params`. Returns the profile and
/// the password (if any), which callers should put in the Keychain.
pub fn parse_database_url(url: &str) -> Result<(ConnProfile, Option<String>)> {
    let bad = || CoreError::Parse(format!("not a PostgreSQL URL: {url}"));
    let rest = url
        .trim()
        .strip_prefix("postgres://")
        .or_else(|| url.trim().strip_prefix("postgresql://"))
        .ok_or_else(bad)?;
    let rest = rest.split('#').next().unwrap_or_default();
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((user, pw)) => (percent_decode(user), Some(percent_decode(pw))),
            None => (percent_decode(u), None),
        },
        None => (
            std::env::var("USER").unwrap_or_else(|_| "postgres".into()),
            None,
        ),
    };
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or_else(bad)?;
        (h.to_string(), after.strip_prefix(':').map(str::to_string))
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.to_string())),
            None => (hostport.to_string(), None),
        }
    };
    if host.contains(',') {
        return Err(CoreError::Parse(
            "URLs with multiple hosts are not supported yet; use a single host.".into(),
        ));
    }
    let mut ssl = SslMode::default();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == "sslmode" {
            let value = percent_decode(value);
            ssl = SslMode::parse(&value)
                .ok_or_else(|| CoreError::Parse(format!("unknown sslmode: {value}")))?;
        }
    }
    let host = if host.is_empty() {
        "localhost".to_string()
    } else {
        percent_decode(&host)
    };
    let port = match port {
        Some(p) if !p.is_empty() => p.parse().map_err(|_| bad())?,
        _ => 5432,
    };
    let database = if path.is_empty() {
        user.clone()
    } else {
        percent_decode(path)
    };
    let profile = ConnProfile {
        ssl,
        ..ConnProfile::new(&host, port, &database, &user)
    };
    Ok((profile, password))
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

const KEYCHAIN_SERVICE: &str = "pgdesk";
const ERR_NOT_FOUND: i32 = -25300;

pub fn store_password(profile: &ConnProfile, password: &str) -> Result<()> {
    security_framework::passwords::set_generic_password(
        KEYCHAIN_SERVICE,
        &profile.keychain_account(),
        password.as_bytes(),
    )
    .map_err(|e| CoreError::Config(format!("Keychain: {e}")))
}

pub fn load_password(profile: &ConnProfile) -> Result<Option<String>> {
    match security_framework::passwords::get_generic_password(
        KEYCHAIN_SERVICE,
        &profile.keychain_account(),
    ) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.code() == ERR_NOT_FOUND => Ok(None),
        Err(e) => Err(CoreError::Config(format!("Keychain: {e}"))),
    }
}

pub fn delete_password(profile: &ConnProfile) -> Result<()> {
    match security_framework::passwords::delete_generic_password(
        KEYCHAIN_SERVICE,
        &profile.keychain_account(),
    ) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERR_NOT_FOUND => Ok(()),
        Err(e) => Err(CoreError::Config(format!("Keychain: {e}"))),
    }
}
