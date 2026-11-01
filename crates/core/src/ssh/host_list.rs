//! The saved servers: one JSON file, and searching it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SshHost, query_words};
use crate::files::write_text_atomic;
use crate::{CoreError, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostList {
    #[serde(default)]
    pub hosts: Vec<SshHost>,
}

impl HostList {
    /// A missing file is an empty list; an unreadable one is an error, so it
    /// is never replaced by an empty list.
    pub fn load(path: &Path) -> Result<HostList> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HostList::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        write_text_atomic(path, &text)
    }

    pub fn add(&mut self, mut host: SshHost, now: u64) -> String {
        host.created_at = now;
        let id = host.id.clone();
        self.hosts.push(host);
        id
    }

    pub fn get(&self, id: &str) -> Option<&SshHost> {
        self.hosts.iter().find(|h| h.id == id)
    }

    pub fn update(&mut self, id: &str, edit: impl FnOnce(&mut SshHost)) -> bool {
        match self.hosts.iter_mut().find(|h| h.id == id) {
            Some(h) => {
                let before = h.clone();
                edit(h);
                *h != before
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.hosts.len();
        self.hosts.retain(|h| h.id != id);
        self.hosts.len() != before
    }

    /// Hosts matching every word of `query` (name, address, group), grouped
    /// (ungrouped first) and by name within a group.
    pub fn search(&self, query: &str) -> Vec<&SshHost> {
        let words = query_words(query);
        let mut found: Vec<&SshHost> = self
            .hosts
            .iter()
            .filter(|h| {
                let hay =
                    format!("{} {} {} {}", h.name, h.address(), h.group, h.jump).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect();
        found.sort_by_cached_key(|h| (h.group.trim().to_lowercase(), h.label().to_lowercase()));
        found
    }
}
