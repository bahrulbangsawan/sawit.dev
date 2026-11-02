//! What each skills folder has (its skill folders, and a Claude Code profile's plugins and
//! marketplaces), and a pack's state there.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use super::bundled::{self, Held};
use super::{Pack, Target, targets};

/// What one target has: the skills in its folder, and for a Claude Code
/// profile the plugins installed for the user, their skills and the
/// marketplaces it knows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    /// Skill folders with a `SKILL.md` (links followed).
    pub folder: HashSet<String>,
    /// Skills the profile's plugins bring.
    pub from_plugins: HashSet<String>,
    /// `name@marketplace` of every plugin installed for the user.
    pub plugins: HashSet<String>,
    /// The marketplaces the profile has added.
    pub marketplaces: HashSet<String>,
    /// What the folder holds where each skill Sawit ships goes, by its name, when anything.
    pub bundled: HashMap<String, Held>,
}

impl Installed {
    pub fn has(&self, skill: &str) -> bool {
        self.folder.contains(skill) || self.from_plugins.contains(skill)
    }
}

/// The skill folders in `dir`: each folder (or link to one) with a `SKILL.md`.
fn skills_in(dir: &Path) -> HashSet<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("SKILL.md").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Reads what `target` has, from its folder and, for a Claude Code profile,
/// `plugins/installed_plugins.json` and `plugins/known_marketplaces.json`.
pub fn read_installed(target: &Target) -> Installed {
    let mut installed = Installed {
        folder: skills_in(&target.dir),
        bundled: bundled::names()
            .filter_map(|name| {
                Some((
                    name.to_string(),
                    bundled::held(name, &target.dir.join(name))?,
                ))
            })
            .collect(),
        ..Default::default()
    };
    let Some(claude) = &target.claude else {
        return installed;
    };
    let plugins = claude.dir.join("plugins");
    let json = |file: &str| -> Option<Value> {
        serde_json::from_str(&std::fs::read_to_string(plugins.join(file)).ok()?).ok()
    };
    if let Some(Value::Object(known)) = json("known_marketplaces.json") {
        installed.marketplaces = known.keys().cloned().collect();
    }
    if let Some(list) = json("installed_plugins.json")
        .as_ref()
        .and_then(|v| v.get("plugins"))
        .and_then(Value::as_object)
    {
        for (key, installs) in list {
            // A plugin installed for one project only isn't the user's.
            let user = installs
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i.get("scope").and_then(Value::as_str) == Some("user"));
            let mut any = false;
            for install in user {
                any = true;
                if let Some(path) = install.get("installPath").and_then(Value::as_str) {
                    installed
                        .from_plugins
                        .extend(skills_in(&Path::new(path).join("skills")));
                }
            }
            if any {
                installed.plugins.insert(key.clone());
            }
        }
    }
    installed
}

/// A pack's state in one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Every skill of the pack is there; `plugin` when its plugin brings them.
    Installed {
        plugin: bool,
    },
    /// Every skill Sawit ships that it installs is there, Sawit's own, but one differs from this
    /// Sawit's version: edited there, or another Sawit's version (an update is available).
    Differs,
    /// Some of its skills are there.
    Partial {
        have: usize,
        of: usize,
    },
    Missing,
}

impl Status {
    /// Installed, or partly.
    pub fn any(self) -> bool {
        self != Status::Missing
    }

    /// Every skill of the pack is there: installed, or in a version that differs.
    pub fn full(self) -> bool {
        matches!(self, Status::Installed { .. } | Status::Differs)
    }
}

/// `pack`'s state in a target that has `installed`. A skill Sawit ships counts only as a folder
/// whose SKILL.md is Sawit's, compared with this Sawit's version.
pub fn status(pack: &Pack, installed: &Installed) -> Status {
    if let Some(names) = pack.bundled() {
        let held = |name: &&str| installed.bundled.get(*name).copied();
        let sawits = names
            .iter()
            .filter(|n| matches!(held(n), Some(Held::Same | Held::Differs)))
            .count();
        return match sawits {
            0 => Status::Missing,
            n if n < names.len() => Status::Partial {
                have: n,
                of: names.len(),
            },
            _ if names.iter().all(|n| held(n) == Some(Held::Same)) => {
                Status::Installed { plugin: false }
            }
            _ => Status::Differs,
        };
    }
    if pack
        .plugin()
        .and_then(|m| m.plugin_key())
        .is_some_and(|key| installed.plugins.contains(&key))
    {
        return Status::Installed { plugin: true };
    }
    let have = pack.skills.iter().filter(|s| installed.has(s)).count();
    match have {
        0 => Status::Missing,
        n if n == pack.skills.len() => Status::Installed { plugin: false },
        n => Status::Partial {
            have: n,
            of: pack.skills.len(),
        },
    }
}

/// The Skills page's list, as `list_skills` and `sawit list skills` show it.
#[derive(Debug, Clone)]
pub struct Report {
    pub targets: Vec<Target>,
    /// What each target has, by its place in `targets`.
    pub installed: Vec<Installed>,
}

impl Report {
    pub fn status(&self, pack: &Pack, target: usize) -> Status {
        self.installed
            .get(target)
            .map_or(Status::Missing, |i| status(pack, i))
    }
}

/// The [`Report`] for this Mac.
pub fn report() -> Report {
    let targets = targets();
    let installed = targets.iter().map(read_installed).collect();
    Report { targets, installed }
}
