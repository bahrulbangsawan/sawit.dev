//! Instance registry (TOML) and PG Desk's on-disk layout.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ports::BindAddr;
use crate::{CoreError, Result};

/// PG Desk's data directory and the files inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    /// `~/Library/Application Support/pgdesk`.
    pub fn default_macos() -> AppPaths {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/tmp".into());
        AppPaths {
            root: home.join("Library/Application Support/pgdesk"),
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

    pub fn managed_bin_root(&self) -> PathBuf {
        self.root.join("bin")
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
    /// Created by PG Desk (vs. an adopted existing data dir).
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

    /// Writes atomically (temp file + rename).
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

    pub fn add(&mut self, inst: Instance) -> Result<()> {
        validate_name(&inst.name)?;
        if self.get(&inst.name).is_some() {
            return Err(CoreError::Config(format!(
                "instance \"{}\" already exists",
                inst.name
            )));
        }
        if let Some(other) = self.instances.iter().find(|i| i.data_dir == inst.data_dir) {
            return Err(CoreError::Config(format!(
                "data dir {} is already used by \"{}\"",
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
        Err(CoreError::Config(format!(
            "invalid instance name {name:?}: use 1-40 characters of a-z, 0-9, '-' or '_'"
        )))
    }
}

/// macOS limits Unix socket paths to 103 bytes; longer ones make the server fail to start.
pub fn validate_socket_dir(dir: &Path, port: u16) -> Result<()> {
    let socket = dir.join(format!(".s.PGSQL.{port}"));
    let len = socket.as_os_str().len();
    if len <= 103 {
        Ok(())
    } else {
        Err(CoreError::Config(format!(
            "socket path {} is {len} bytes; macOS allows at most 103",
            socket.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(name: &str, port: u16) -> Instance {
        Instance {
            name: name.into(),
            bin_dir: "/opt/homebrew/opt/postgresql@16/bin".into(),
            data_dir: format!("/data/{name}").into(),
            port,
            listen: ListenScope::Localhost,
            managed: true,
        }
    }

    #[test]
    fn roundtrip_toml() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/instances.toml");
        let mut reg = Registry::default();
        reg.add(inst("app", 5440)).unwrap();
        reg.add(Instance {
            listen: ListenScope::All,
            ..inst("crm", 5441)
        })
        .unwrap();
        reg.save(&file).unwrap();
        assert_eq!(Registry::load(&file).unwrap(), reg);
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            Registry::load(&dir.path().join("nope.toml"))
                .unwrap()
                .instances
                .is_empty()
        );
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut reg = Registry::default();
        reg.add(inst("app", 5440)).unwrap();
        assert!(
            reg.add(Instance {
                data_dir: "/other".into(),
                ..inst("app", 5441)
            })
            .is_err()
        );
    }

    #[test]
    fn duplicate_data_dir_rejected() {
        let mut reg = Registry::default();
        reg.add(inst("app", 5440)).unwrap();
        assert!(
            reg.add(Instance {
                data_dir: "/data/app".into(),
                ..inst("app2", 5441)
            })
            .is_err()
        );
    }

    #[test]
    fn same_port_listed_in_port_clashes() {
        let mut reg = Registry::default();
        reg.add(inst("a", 5433)).unwrap();
        reg.add(inst("b", 5433)).unwrap();
        reg.add(inst("c", 5434)).unwrap();
        assert_eq!(
            reg.port_clashes(),
            vec![("a".to_string(), "b".to_string(), 5433)]
        );
    }

    #[test]
    fn remove_and_get() {
        let mut reg = Registry::default();
        reg.add(inst("a", 5433)).unwrap();
        assert!(reg.get("a").is_some());
        reg.remove("a");
        assert!(reg.get("a").is_none());
    }

    #[test]
    fn invalid_names_rejected() {
        for bad in ["My DB", "", &"x".repeat(41), "a/b", "../x"] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
        for good in ["app", "kyzn-crm_2", &"x".repeat(40)] {
            assert!(validate_name(good).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn socket_path_too_long_is_rejected() {
        let long = std::path::PathBuf::from(format!("/{}", "d".repeat(99)));
        assert!(validate_socket_dir(&long, 5432).is_err());
        assert!(
            validate_socket_dir(
                Path::new("/Users/rulisme/Library/Application Support/pgdesk/run"),
                5432
            )
            .is_ok()
        );
    }

    #[test]
    fn listen_scope_addresses() {
        assert_eq!(
            ListenScope::Localhost.bind_addrs(),
            vec![BindAddr::LoopbackV4, BindAddr::LoopbackV6]
        );
        assert_eq!(ListenScope::All.listen_addresses(), "*");
    }

    #[test]
    fn app_paths_layout() {
        let p = AppPaths { root: "/r".into() };
        assert_eq!(p.cluster_dir("app"), Path::new("/r/clusters/app"));
        assert_eq!(p.log_file("app"), Path::new("/r/logs/app.log"));
        assert_eq!(p.socket_dir(), Path::new("/r/run"));
        assert_eq!(p.managed_bin_root(), Path::new("/r/bin"));
        assert_eq!(p.instances_file(), Path::new("/r/instances.toml"));
    }
}
