//! A restore's work on the library: which prompts it brings in or deletes, and the library
//! changed with GitHub's files in hand.

use std::collections::BTreeMap;

use super::{PromptSync, SyncOutcome, from_markdown};
use crate::Result;
use crate::github::FileState;
use crate::prompts::{Library, Prompt};

/// The prompts a restore brings in: changed on GitHub, only there, or chosen (Keep GitHub's).
pub(super) fn to_restore<'a>(
    plan: &'a [PromptSync],
    keep_github: &[String],
) -> Vec<&'a PromptSync> {
    plan.iter()
        .filter(|p| {
            p.state.downloads()
                || (p.state == FileState::ChangedOnBoth && keep_github.contains(&p.id))
        })
        .collect()
}

/// Files now the same on both sides, with their hash, for the sync's state.
pub type Synced = Vec<(String, Option<String>)>;

/// A restore's work on `library`, with GitHub's files (`contents`, by path) in hand: each prompt
/// to bring in replaces or joins the one here; a conflict not chosen is left and listed. Returns
/// what it did and the files now the same on both sides, for the sync's state.
pub fn apply_restore(
    library: &mut Library,
    plan: &[PromptSync],
    contents: &BTreeMap<String, Vec<u8>>,
    keep_github: &[String],
) -> Result<(SyncOutcome, Synced)> {
    let mut out = SyncOutcome::default();
    let mut synced = Vec::new();
    let chosen: Vec<&str> = to_restore(plan, keep_github)
        .into_iter()
        .map(|p| p.path.as_str())
        .collect();
    for p in plan {
        if chosen.contains(&p.path.as_str()) && p.remote.is_none() {
            // Deleted on GitHub since the last sync: deleted here too.
            let before = library.items.len();
            library.items.retain(|item| item.id != p.id);
            if library.items.len() < before {
                out.deleted.push(p.id.clone());
            }
            synced.push((p.path.clone(), None));
        } else if chosen.contains(&p.path.as_str()) {
            let Some(bytes) = contents.get(&p.path) else {
                continue;
            };
            let prompt = from_markdown(&String::from_utf8_lossy(bytes))?;
            out.synced.push(prompt.title.clone());
            out.synced_ids.push(prompt.id.clone());
            merge(library, prompt);
            synced.push((p.path.clone(), p.remote.clone()));
        } else if p.state == FileState::ChangedOnBoth {
            out.conflicts.push(p.clone());
        } else if p.state == FileState::Same {
            synced.push((p.path.clone(), p.remote.clone()));
        }
    }
    Ok((out, synced))
}

/// Puts `prompt` in `library`: in place of the one with its id (keeping that one's use counts),
/// or added.
pub fn merge(library: &mut Library, prompt: Prompt) {
    match library.items.iter_mut().find(|p| p.id == prompt.id) {
        Some(p) => {
            let (used, last) = (p.used_count, p.last_used_at);
            *p = Prompt {
                used_count: used,
                last_used_at: last,
                ..prompt
            };
        }
        None => library.items.push(prompt),
    }
}
