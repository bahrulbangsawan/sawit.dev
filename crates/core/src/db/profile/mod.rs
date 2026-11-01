//! Connection profiles (TOML, never containing passwords), DATABASE_URL and
//! .env parsing, and Keychain-stored passwords.

mod dotenv;
mod fields;
mod password;
mod url;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result};

pub use dotenv::{dotenv_upsert, dotenv_value, read_dotenv_database_url};
pub use password::{
    delete_password, load_password, pgpass_password, resolve_password, store_password,
};
pub use url::parse_database_url;

#[cfg(test)]
pub(super) use fields::service_section;
#[cfg(test)]
pub(super) use password::pgpass_lookup;

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
    /// What kind of database this is; absent (PostgreSQL) in every profile saved before
    /// SQLite and D1 came.
    #[serde(default, skip_serializing_if = "DbKind::is_postgres")]
    pub kind: DbKind,
    /// Where a SQLite or D1 database is (the host, port and user above are empty for them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lite: Option<LiteSource>,
}

/// The kinds of database Sawit opens. PostgreSQL has every feature; SQLite and Cloudflare D1
/// share one backend ([`super::sqlite`]) and leave out what only a server has (instances,
/// `pg_dump`, roles, materialized views).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DbKind {
    #[default]
    Postgres,
    /// A D1 binding's local database, the file Miniflare keeps in `.wrangler/state`.
    D1Local,
    /// A D1 database on Cloudflare, through the user's Wrangler.
    D1Remote,
    /// Any other `.sqlite` file.
    Sqlite,
}

impl DbKind {
    pub fn is_postgres(&self) -> bool {
        *self == DbKind::Postgres
    }

    /// The name people know it by, for tooltips and accessible names.
    pub fn title(self) -> &'static str {
        match self {
            DbKind::Postgres => "PostgreSQL",
            DbKind::D1Local => "Cloudflare D1 (local)",
            DbKind::D1Remote => "Cloudflare D1 (remote)",
            DbKind::Sqlite => "SQLite",
        }
    }

    /// The `kind` `detect_databases` reports: `postgres`, `d1-local`, `d1-remote`, `sqlite`.
    pub fn id(self) -> &'static str {
        match self {
            DbKind::Postgres => "postgres",
            DbKind::D1Local => "d1-local",
            DbKind::D1Remote => "d1-remote",
            DbKind::Sqlite => "sqlite",
        }
    }

    /// SQLite underneath: a file, or D1.
    pub fn is_lite(self) -> bool {
        !self.is_postgres()
    }

    /// The schema the tree opens on its own: `public`, or SQLite's `main`.
    pub fn default_schema(self) -> &'static str {
        match self {
            DbKind::Postgres => "public",
            _ => super::sqlite::MAIN,
        }
    }
}

/// Where a SQLite or D1 database is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiteSource {
    /// The database file (SQLite, D1's local copy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<std::path::PathBuf>,
    /// The Workers project whose config declares it (D1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<std::path::PathBuf>,
    /// The Worker's name, which the Local Explorer runs it under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    /// The binding (`env.DB`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    /// `database_name`, which Wrangler's remote commands take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_name: Option<String>,
    /// The id Miniflare and the Local Explorer know a local database by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d1_id: Option<String>,
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
            kind: DbKind::Postgres,
            lite: None,
        }
    }

    /// A local D1 database of a Workers project, named by its target id
    /// ([`super::d1::D1Target`]).
    pub fn d1_local(d1: &super::d1::LocalD1) -> ConnProfile {
        let target = super::d1::D1Target::Local {
            project: d1.project.clone(),
            binding: d1.binding.clone(),
        };
        ConnProfile {
            name: target.id(),
            label: Some(d1.binding.clone()),
            kind: DbKind::D1Local,
            lite: Some(LiteSource {
                file: Some(d1.file.clone()),
                project: Some(d1.project.clone()),
                worker: d1.worker.clone(),
                binding: Some(d1.binding.clone()),
                database_name: d1.database_name.clone(),
                d1_id: Some(d1.miniflare_id.clone()),
            }),
            ..ConnProfile::new(
                "",
                0,
                d1.database_name.as_deref().unwrap_or(&d1.binding),
                "",
            )
        }
    }

    /// A D1 database on Cloudflare that `binding` of the project in `project` names.
    pub fn d1_remote(project: &Path, binding: &str, database: &str) -> ConnProfile {
        let target = super::d1::D1Target::Remote {
            project: project.to_path_buf(),
            database: database.to_string(),
        };
        ConnProfile {
            name: target.id(),
            label: Some(format!("{binding} (remote)")),
            kind: DbKind::D1Remote,
            lite: Some(LiteSource {
                project: Some(project.to_path_buf()),
                binding: Some(binding.to_string()),
                database_name: Some(database.to_string()),
                ..LiteSource::default()
            }),
            ..ConnProfile::new("", 0, database, "")
        }
    }

    /// A SQLite file (a D1 file no binding names, or one opened by hand).
    pub fn sqlite_file(file: &Path) -> ConnProfile {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        ConnProfile {
            name: super::d1::D1Target::File(file.to_path_buf()).id(),
            label: Some(name.clone()),
            kind: DbKind::Sqlite,
            lite: Some(LiteSource {
                file: Some(file.to_path_buf()),
                ..LiteSource::default()
            }),
            ..ConnProfile::new("", 0, &name, "")
        }
    }

    /// The database file of a SQLite or local D1 profile.
    pub fn file(&self) -> Option<&Path> {
        self.lite.as_ref()?.file.as_deref()
    }

    /// Where it points, for tooltips: the connection string, or the file, or the remote D1
    /// database and its project.
    pub fn location(&self) -> String {
        let lite = self.lite.clone().unwrap_or_default();
        match self.kind {
            DbKind::Postgres => self.connection_string(),
            DbKind::D1Remote => format!(
                "{} on Cloudflare, through Wrangler in {}",
                lite.database_name.unwrap_or_default(),
                lite.project.unwrap_or_default().display()
            ),
            DbKind::D1Local | DbKind::Sqlite => lite.file.unwrap_or_default().display().to_string(),
        }
    }

    /// "PostgreSQL 16 · kyzn", "Cloudflare D1 (local) · DB": the kind first, so a logo is
    /// never the only way to tell. `version` is the server's, when known.
    pub fn accessible_name(&self, version: Option<&str>) -> String {
        let kind = match version {
            Some(v) if self.kind.is_postgres() => format!("{} {v}", self.kind.title()),
            _ => self.kind.title().to_string(),
        };
        let name = match (self.kind, &self.lite) {
            // A binding goes by its name; the kind already says local or remote.
            (DbKind::D1Local | DbKind::D1Remote, Some(lite)) => lite
                .binding
                .clone()
                .unwrap_or_else(|| self.database.clone()),
            _ => self.display_name().to_string(),
        };
        format!("{kind} · {name}")
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

    /// Keychain account for this profile's password.
    pub fn keychain_account(&self) -> String {
        format!(
            "{}@{}:{}/{}",
            self.user, self.host, self.port, self.database
        )
    }
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
