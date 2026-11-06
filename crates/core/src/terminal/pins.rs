//! Pinned terminal tabs, kept per project (backlog item 2): a pinned tab comes back pinned (and
//! locked, if it was) when its project opens again, as a fresh shell in the folder it was in.
//! Terminals themselves don't outlive the app, so only what makes the tab is kept: its name,
//! its folder and its lock.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One pinned tab of a project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedTab {
    /// The name the user gave the tab, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where its shell was (its current folder when it was pinned or last saved).
    pub cwd: PathBuf,
    #[serde(default)]
    pub locked: bool,
}

/// Every project's pinned tabs, by project folder, in strip order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pins {
    #[serde(default)]
    pub projects: BTreeMap<PathBuf, Vec<PinnedTab>>,
}

impl Pins {
    /// The pins in `file`; none when it is missing or can't be read.
    pub fn load(file: &Path) -> Pins {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the pins to `file` (atomically), or removes it when there are none.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if self.projects.values().all(Vec::is_empty) {
            return match std::fs::remove_file(file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::files::write_text_atomic(file, &text)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    /// Replaces `project`'s pins with `tabs` (none forgets the project).
    pub fn set(&mut self, project: &Path, tabs: Vec<PinnedTab>) {
        if tabs.is_empty() {
            self.projects.remove(project);
        } else {
            self.projects.insert(project.to_path_buf(), tabs);
        }
    }

    pub fn of(&self, project: &Path) -> &[PinnedTab] {
        self.projects.get(project).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_round_trip_and_go_when_empty() {
        let dir = std::env::temp_dir().join(format!("sawit-pins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("terminal-pins.json");
        let mut pins = Pins::default();
        let tab = PinnedTab {
            name: Some("dev".into()),
            cwd: "/p/web".into(),
            locked: true,
        };
        pins.set(Path::new("/p"), vec![tab.clone()]);
        pins.save(&file).unwrap();
        assert_eq!(Pins::load(&file).of(Path::new("/p")), &[tab]);
        pins.set(Path::new("/p"), vec![]);
        pins.save(&file).unwrap();
        assert!(!file.exists());
        assert_eq!(Pins::load(&file), Pins::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
