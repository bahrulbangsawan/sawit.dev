//! What Sawit remembers of the instruction files it changed, in its data folder
//! (`instruction-files.json`): the files it made, which removing may delete once they hold
//! nothing else, and where it copied each file it found there before its first change
//! (`instruction-backups/home/` and the file's path in the home folder, or `root/` and its whole
//! path).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::instances::AppPaths;

/// One file Sawit changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Sawit made it.
    #[serde(default)]
    pub created: bool,
    /// The copy of it made before Sawit first changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<PathBuf>,
}

/// The files Sawit changed, by their path, and where that list and the copies are kept.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    pub files: BTreeMap<PathBuf, Entry>,
    /// `instruction-files.json`.
    at: PathBuf,
    /// `instruction-backups`.
    backups: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    files: BTreeMap<PathBuf, Entry>,
}

impl Ledger {
    /// The list in Sawit's data folder `paths` (empty when there is none yet).
    pub fn load(paths: &AppPaths) -> Result<Ledger, String> {
        let at = paths.root.join("instruction-files.json");
        let files = match std::fs::read_to_string(&at) {
            Ok(text) => {
                serde_json::from_str::<Stored>(&text)
                    .map_err(|e| format!("{} can't be read ({e}): Sawit won't guess which files it made, so it changes none. Move it away to start over.", at.display()))?
                    .files
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(format!("Couldn't read {}: {e}", at.display())),
        };
        Ok(Ledger {
            files,
            at,
            backups: paths.root.join("instruction-backups"),
        })
    }

    /// Whether Sawit made `file`.
    pub fn created(&self, file: &Path) -> bool {
        self.files.get(file).is_some_and(|e| e.created)
    }

    /// Where the copy of `file` goes, under `instruction-backups`: `home/` and its path in the
    /// home folder (`home/.claude/CLAUDE.md`), else `root/` and its whole path.
    pub fn backup_path(&self, file: &Path) -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        match home.as_deref().and_then(|h| file.strip_prefix(h).ok()) {
            Some(rel) if home.as_deref().is_some_and(|h| h.parent().is_some()) => {
                self.backups.join("home").join(rel)
            }
            _ => self
                .backups
                .join("root")
                .join(file.strip_prefix("/").unwrap_or(file)),
        }
    }

    /// Before Sawit's first change to `file`: a copy of it in Sawit's data folder when it
    /// `exists` (an older copy there is kept: it is the file as Sawit first found it), else a note
    /// that Sawit made it. Returns the copy made now. Later changes do nothing here.
    pub fn first_change(&mut self, file: &Path, exists: bool) -> Result<Option<PathBuf>, String> {
        if self.files.contains_key(file) {
            return Ok(None);
        }
        let mut entry = Entry {
            created: !exists,
            backup: None,
        };
        let mut made = None;
        if exists {
            let to = self.backup_path(file);
            if !to.exists() {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Couldn't make {}: {e}", parent.display()))?;
                }
                std::fs::copy(file, &to).map_err(|e| {
                    format!("Couldn't copy {} to {}: {e}", file.display(), to.display())
                })?;
                made = Some(to.clone());
            }
            entry.backup = Some(to);
        }
        self.files.insert(file.to_path_buf(), entry);
        self.save()?;
        Ok(made)
    }

    /// Drops `file` from the list: Sawit deleted it.
    pub fn forget(&mut self, file: &Path) -> Result<(), String> {
        if self.files.remove(file).is_some() {
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> Result<(), String> {
        let stored = Stored {
            files: self.files.clone(),
        };
        let text = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
        if let Some(parent) = self.at.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Couldn't make {}: {e}", parent.display()))?;
        }
        super::files::write_whole(&self.at, &format!("{text}\n"))
    }
}
