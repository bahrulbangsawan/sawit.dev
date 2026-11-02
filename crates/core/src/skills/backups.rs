//! Where an install keeps an agent's copy of a skill Sawit ships when it replaces one that differs
//! from this Sawit's version: in Sawit's data folder, out of every agent's reach.

use std::path::PathBuf;

use super::bundled::Held;
use super::commands::{Env, Place};
use super::{Pack, Target};

/// Where an install or update of `pack` in `places` keeps the copies it replaces that differ from
/// this Sawit's version (a skill Sawit ships only): see [`backup`].
pub fn kept(pack: &Pack, places: &[Place], env: &Env) -> Vec<PathBuf> {
    let Some(name) = pack.bundled() else {
        return Vec::new();
    };
    places
        .iter()
        .filter(|(_, i)| i.bundled.get(name) == Some(&Held::Differs))
        .map(|(t, _)| backup(env, t, name))
        .collect()
}

/// Where the copy of skill `name` in `target`'s skills folder goes when an install replaces it:
/// `skill-backups/<agent>/<name>` in Sawit's data folder, beside `bundled-skills`. Never in the
/// skills folder itself, where an agent would load it as a second skill of the same name.
pub fn backup(env: &Env, target: &Target, name: &str) -> PathBuf {
    env.bundled
        .with_file_name("skill-backups")
        .join(folder_name(&target.label))
        .join(name)
}

/// An agent's label as one folder name: "Claude Code · work" is `claude-code-work`.
fn folder_name(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}
