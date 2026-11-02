//! The commands the user saved for a project, one or more command lines each, all kept in one
//! JSON file keyed by project folder.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::files::write_text_atomic;
use crate::{CoreError, Result};

/// How the command lines of a saved command run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    /// One after another in one terminal, stopping at the first failure.
    #[default]
    Sequence,
    /// All at once, each in its own terminal.
    Parallel,
}

/// A command the user saved for a project: one or more command lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCommand {
    pub id: String,
    pub name: String,
    pub commands: Vec<String>,
    #[serde(default)]
    pub mode: RunMode,
}

impl SavedCommand {
    pub fn new(name: &str, commands: Vec<String>, mode: RunMode) -> SavedCommand {
        SavedCommand {
            id: new_id(),
            name: name.trim().to_string(),
            commands: clean_commands(commands),
            mode,
        }
    }

    /// What each terminal types: all lines joined with `&&` in sequence,
    /// one terminal per line in parallel.
    pub fn lines(&self) -> Vec<String> {
        match self.mode {
            RunMode::Sequence if !self.commands.is_empty() => vec![self.commands.join(" && ")],
            RunMode::Sequence => Vec::new(),
            RunMode::Parallel => self.commands.clone(),
        }
    }
}

/// Trimmed, without empty lines.
pub fn clean_commands(commands: Vec<String>) -> Vec<String> {
    commands
        .into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    format!("{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Every project's saved commands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCommands {
    /// By project folder (absolute path).
    pub projects: BTreeMap<String, Vec<SavedCommand>>,
}

impl SavedCommands {
    /// A missing file is empty; a corrupt one is an error (so it is never
    /// overwritten with nothing). Each project's commands are under its folder
    /// as it is on disk ([`Self::keyed_on_disk`]).
    pub fn load(path: &Path) -> Result<SavedCommands> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<SavedCommands>(&text)
                .map(SavedCommands::keyed_on_disk)
                .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SavedCommands::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// The commands keyed by each project's folder as it is on disk ([`crate::files::resolve`]),
    /// which is how the app opens a project and sawit-mcp names one. Commands saved under
    /// another name of a folder (a link to it, as a project opened through a link was keyed
    /// before) join the folder's own, after them, and a command already there by id isn't
    /// added twice.
    fn keyed_on_disk(self) -> SavedCommands {
        let mut projects: BTreeMap<String, Vec<SavedCommand>> = BTreeMap::new();
        let (own, other): (Vec<_>, Vec<_>) = self
            .projects
            .into_iter()
            .map(|(key, commands)| {
                let real = crate::files::resolve(Path::new(&key));
                (Self::key(&real), key, commands)
            })
            .partition(|(real, key, _)| real == key);
        for (real, _, commands) in own.into_iter().chain(other) {
            let list = projects.entry(real).or_default();
            for command in commands {
                if !list.iter().any(|c| c.id == command.id) {
                    list.push(command);
                }
            }
        }
        SavedCommands { projects }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        write_text_atomic(path, &format!("{text}\n"))
    }

    fn key(root: &Path) -> String {
        root.to_string_lossy().into_owned()
    }

    pub fn for_project(&self, root: &Path) -> &[SavedCommand] {
        self.projects
            .get(&Self::key(root))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn get(&self, root: &Path, id: &str) -> Option<&SavedCommand> {
        self.for_project(root).iter().find(|c| c.id == id)
    }

    /// Adds `command`, or replaces the one with its id.
    pub fn upsert(&mut self, root: &Path, command: SavedCommand) {
        let list = self.projects.entry(Self::key(root)).or_default();
        match list.iter_mut().find(|c| c.id == command.id) {
            Some(existing) => *existing = command,
            None => list.push(command),
        }
    }

    pub fn remove(&mut self, root: &Path, id: &str) -> bool {
        let key = Self::key(root);
        let Some(list) = self.projects.get_mut(&key) else {
            return false;
        };
        let before = list.len();
        list.retain(|c| c.id != id);
        let removed = list.len() != before;
        if list.is_empty() {
            self.projects.remove(&key);
        }
        removed
    }
}
