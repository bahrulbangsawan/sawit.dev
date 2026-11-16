//! Instance registry (TOML) and Sawit's on-disk layout.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ports::BindAddr;
use crate::{CoreError, Result};

/// Sawit's data directory and the files inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    /// `$SAWIT_HOME` if set (development/tests), else
    /// `~/Library/Application Support/sawit` (moved there from the
    /// pre-rename `…/pgdesk` on first use).
    pub fn default_macos() -> AppPaths {
        let override_root = std::env::var_os("SAWIT_HOME").filter(|r| !r.is_empty());
        let custom = override_root.is_some();
        let paths = AppPaths::resolve(override_root, std::env::var_os("HOME"));
        if !custom && let Some(support) = paths.root.parent() {
            migrate_legacy_root(&support.join("pgdesk"), &paths.root);
        }
        paths
    }

    pub fn resolve(
        override_root: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
    ) -> AppPaths {
        if let Some(root) = override_root.filter(|r| !r.is_empty()) {
            return AppPaths { root: root.into() };
        }
        let home = home.map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
        AppPaths {
            root: home.join("Library/Application Support/sawit"),
        }
    }

    pub fn instances_file(&self) -> PathBuf {
        self.root.join("instances.toml")
    }

    pub fn cluster_dir(&self, name: &str) -> PathBuf {
        self.root.join("clusters").join(name)
    }

    pub fn log_file(&self, name: &str) -> PathBuf {
        self.root.join("logs").join(format!("{name}.log"))
    }

    pub fn socket_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    /// Where the running app listens for the `sawit` CLI. Socket paths are
    /// limited to 104 bytes on macOS, so a deep root falls back to `/tmp`
    /// (named per user and root).
    pub fn cli_socket(&self) -> PathBuf {
        use std::hash::{Hash, Hasher};
        let path = self.root.join("sawit.sock");
        if path.as_os_str().len() < 100 {
            return path;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.root.hash(&mut hasher);
        let user = std::env::var("USER").unwrap_or_default();
        PathBuf::from(format!("/tmp/sawit-{user}-{:x}.sock", hasher.finish()))
    }

    pub fn managed_bin_root(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// The user's key bindings, over the app's defaults (Zed's keymap format).
    pub fn keymap_file(&self) -> PathBuf {
        self.root.join("keymap.json")
    }

    /// Recently opened project folders (welcome screen).
    pub fn recent_file(&self) -> PathBuf {
        self.root.join("recent.toml")
    }

    /// Each project's pinned terminal tabs ([`crate::terminal::pins`]).
    pub fn terminal_pins_file(&self) -> PathBuf {
        self.root.join("terminal-pins.json")
    }

    /// Disposable files (e.g. PNG copies of HEIC images).
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// The skills Sawit ships, written out for an install to copy, one folder each.
    pub fn bundled_skills_dir(&self) -> PathBuf {
        self.root.join("bundled-skills")
    }
}

/// Moves the data directory of PG Desk (Sawit's former name) to `root` and
/// re-points the paths saved in its `.toml` files (instances, binaries).
/// Does nothing once `root` exists. Running servers keep working: Postgres
/// holds its data dir open, and `pg_ctl` finds `postmaster.pid` at the new
/// path.
fn migrate_legacy_root(legacy: &Path, root: &Path) {
    if root.exists() || !legacy.is_dir() || std::fs::rename(legacy, root).is_err() {
        return;
    }
    let (from, to) = (
        format!("{}/", legacy.display()),
        format!("{}/", root.display()),
    );
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().is_some_and(|e| e == "toml")
            && let Ok(text) = std::fs::read_to_string(&path)
            && text.contains(&from)
        {
            let _ = std::fs::write(&path, text.replace(&from, &to));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListenScope {
    Localhost,
    All,
}

impl ListenScope {
    pub fn bind_addrs(&self) -> Vec<BindAddr> {
        match self {
            ListenScope::Localhost => vec![BindAddr::LoopbackV4, BindAddr::LoopbackV6],
            ListenScope::All => vec![BindAddr::AnyV4, BindAddr::AnyV6],
        }
    }

    pub fn listen_addresses(&self) -> &'static str {
        match self {
            ListenScope::Localhost => "localhost",
            ListenScope::All => "*",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    pub name: String,
    pub bin_dir: PathBuf,
    pub data_dir: PathBuf,
    pub port: u16,
    pub listen: ListenScope,
    /// Created by Sawit (vs. an adopted existing data dir).
    pub managed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default, rename = "instance")]
    pub instances: Vec<Instance>,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Registry> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| CoreError::Config(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes atomically (private temp file, fsync, rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| CoreError::Config(e.to_string()))?;
        crate::files::write_atomic(path, text.as_bytes())?;
        Ok(())
    }

    /// Reads the list at `path` again, applies `change` and saves it: what another process
    /// (`sawit-mcp`) saved since this one last read is kept, and a file that can't be read is
    /// never saved over (its error is returned).
    pub fn update<T>(
        path: &Path,
        change: impl FnOnce(&mut Registry) -> Result<T>,
    ) -> Result<(Registry, T)> {
        let mut registry = Registry::load(path)?;
        let out = change(&mut registry)?;
        registry.save(path)?;
        Ok((registry, out))
    }

    pub fn add(&mut self, inst: Instance) -> Result<()> {
        validate_name(&inst.name)?;
        if self.get(&inst.name).is_some() {
            return Err(CoreError::Config(format!(
                "An instance named “{}” already exists. Choose another name.",
                inst.name
            )));
        }
        if let Some(other) = self.instances.iter().find(|i| i.data_dir == inst.data_dir) {
            return Err(CoreError::Config(format!(
                "The data directory {} is already used by “{}”.",
                inst.data_dir.display(),
                other.name
            )));
        }
        self.instances.push(inst);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) {
        self.instances.retain(|i| i.name != name);
    }

    pub fn get(&self, name: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.name == name)
    }

    /// Pairs of instances configured on the same port.
    pub fn port_clashes(&self) -> Vec<(String, String, u16)> {
        let mut clashes = Vec::new();
        for (i, a) in self.instances.iter().enumerate() {
            for b in &self.instances[i + 1..] {
                if a.port == b.port {
                    clashes.push((a.name.clone(), b.name.clone(), a.port));
                }
            }
        }
        clashes
    }

    /// (name, data dir) of every instance, for owner identification.
    pub fn data_dirs(&self) -> Vec<(String, PathBuf)> {
        self.instances
            .iter()
            .map(|i| (i.name.clone(), i.data_dir.clone()))
            .collect()
    }
}

/// Instance names: 1–40 of `a-z 0-9 _ -` (they become directory names).
pub fn validate_name(name: &str) -> Result<()> {
    let ok = (1..=40).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(CoreError::Config(
            "Use 1–40 characters for the instance name: lowercase a–z, 0–9, '-' or '_'.".into(),
        ))
    }
}

/// Reads an uncommented `port = N` from a postgresql.conf.
pub fn parse_conf_port(conf: &str) -> Option<u16> {
    conf.lines().find_map(|line| {
        let line = line.split('#').next()?.trim();
        let (key, value) = line.split_once('=')?;
        (key.trim() == "port").then(|| value.trim().parse().ok())?
    })
}

/// macOS limits Unix socket paths to 103 bytes; longer ones make the server fail to start.
pub fn validate_socket_dir(dir: &Path, port: u16) -> Result<()> {
    let socket = dir.join(format!(".s.PGSQL.{port}"));
    let len = socket.as_os_str().len();
    if len <= 103 {
        Ok(())
    } else {
        Err(CoreError::Config(format!(
            "The socket path {} is {len} bytes, over the macOS limit of 103. Move Sawit's data to a shorter path.",
            socket.display()
        )))
    }
}

#[cfg(test)]
mod tests;
