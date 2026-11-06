//! The prompt library on GitHub: each prompt a Markdown file with front matter (name, tags,
//! variables) in a private repository of the user's (`sawit-prompts` unless they pick another),
//! readable there, with a `README.md` that lists them by name. It syncs both ways as the shell
//! files do ([`crate::github::plan_sync`]): **Back Up Now** sends what changed here, **Restore**
//! brings in what changed on GitHub, and a prompt changed on both sides is shown, never
//! overwritten, until the user picks a side.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::github::{self, FileState, OwnRepo, RemoteTree, SyncBase};
use crate::prompts::{Library, Prompt, normalize_tags, variables};
use crate::{CoreError, Result};

/// The repository prompts go to unless the user picks another.
pub const DEFAULT_REPO: &str = "sawit-prompts";
/// Where the files are in the repository.
const DIR: &str = "prompts/";
/// The settings and the last sync, in Sawit's data folder.
pub const SETTINGS: &str = "prompts-github.json";
pub const SYNC_STATE: &str = "prompts-sync.json";

/// What the user chose for the backup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettings {
    /// A repository of the signed-in account, by name.
    pub repo: String,
    /// Back up a little after each change.
    #[serde(default)]
    pub auto: bool,
}

impl Default for BackupSettings {
    fn default() -> Self {
        BackupSettings {
            repo: DEFAULT_REPO.into(),
            auto: false,
        }
    }
}

impl BackupSettings {
    pub fn load(path: &Path) -> BackupSettings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// The prompt's path in the repository: by its id, which never changes, so a rename is an edit.
pub fn file_path(prompt: &Prompt) -> String {
    format!("{DIR}{}.md", prompt.id)
}

/// The prompt as a Markdown file: front matter (each value JSON, which YAML reads too), a blank
/// line, then the body as written. Use counts stay on this Mac: using a prompt isn't a change.
pub fn to_markdown(prompt: &Prompt) -> String {
    format!(
        "---\nid: {}\nname: {}\ntags: {}\nvariables: {}\nfavorite: {}\ncreated_at: {}\nupdated_at: {}\n---\n\n{}",
        json(&prompt.id),
        json(&prompt.title),
        json(&prompt.tags),
        json(&variables(&prompt.body)),
        prompt.favorite,
        prompt.created_at,
        prompt.updated_at,
        prompt.body
    )
}

/// A front matter value: JSON, which YAML reads as it is.
fn json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// A prompt back from its Markdown file. The variables are the body's own (`{{name}}`), so the
/// front matter's list is for people reading the repository.
pub fn from_markdown(text: &str) -> Result<Prompt> {
    let bad =
        |why: &str| CoreError::Parse(format!("A prompt file on GitHub is unreadable: {why}."));
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| bad("no front matter"))?;
    let (front, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| bad("front matter never ends"))?;
    let body = body.strip_prefix('\n').unwrap_or(body);
    let mut fields: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
    for line in front.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let parsed = serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
        fields.insert(key.trim(), parsed);
    }
    let text_of = |k: &str| fields.get(k).and_then(|v| v.as_str()).map(String::from);
    let num = |k: &str| fields.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let tags: Vec<String> = fields
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(Prompt {
        id: text_of("id").ok_or_else(|| bad("no id"))?,
        title: text_of("name").unwrap_or_default(),
        body: body.to_string(),
        tags: normalize_tags(&tags),
        favorite: fields
            .get("favorite")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        created_at: num("created_at"),
        updated_at: num("updated_at"),
        used_count: 0,
        last_used_at: 0,
    })
}

/// The repository's `README.md`: every prompt by name, linking to its file.
pub fn readme(library: &Library) -> String {
    let mut items: Vec<&Prompt> = library.items.iter().collect();
    items.sort_by_key(|p| p.title.to_lowercase());
    let mut out = String::from(
        "# Prompts\n\nSaved by [Sawit](https://sawit.dev). Each prompt is a Markdown file with its name, tags and `{{variables}}` in the front matter.\n\n",
    );
    for p in items {
        let tags = if p.tags.is_empty() {
            String::new()
        } else {
            format!(
                " — {}",
                p.tags
                    .iter()
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let title = p.title.replace(['[', ']'], "");
        out.push_str(&format!("- [{title}]({}){tags}\n", file_path(p)));
    }
    out
}

/// The library as files: path in the repository, contents.
pub fn library_files(library: &Library) -> BTreeMap<String, Vec<u8>> {
    library
        .items
        .iter()
        .map(|p| (file_path(p), to_markdown(p).into_bytes()))
        .collect()
}

/// One prompt as the sync sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptSync {
    pub path: String,
    pub id: String,
    /// Its name here, else on GitHub when known.
    pub title: String,
    pub state: FileState,
    #[serde(skip)]
    pub remote: Option<String>,
}

/// How every prompt stands: the library here, the repository's tree, the last sync.
pub fn plan_prompts(library: &Library, tree: &RemoteTree, base: &SyncBase) -> Vec<PromptSync> {
    let local: BTreeMap<String, String> = library_files(library)
        .iter()
        .map(|(path, bytes)| (path.clone(), github::blob_sha(bytes)))
        .collect();
    let remote: BTreeMap<String, String> = tree
        .files
        .iter()
        .filter(|(path, _)| path.starts_with(DIR) && path.ends_with(".md"))
        .map(|(p, s)| (p.clone(), s.clone()))
        .collect();
    github::plan_sync(&local, &remote, base)
        .into_iter()
        .map(|(path, state)| {
            let id = path
                .trim_start_matches(DIR)
                .trim_end_matches(".md")
                .to_string();
            PromptSync {
                title: library
                    .get(&id)
                    .map(|p| p.title.clone())
                    .unwrap_or_default(),
                remote: remote.get(&path).cloned(),
                path,
                id,
                state,
            }
        })
        .collect()
}

/// The repository, found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptBackup {
    pub login: String,
    pub name: String,
    pub repo: OwnRepo,
    pub tree: RemoteTree,
}

/// Finds the repository `name` of the signed-in account (refusing a public one). `None` when
/// it doesn't exist yet.
pub async fn fetch_backup(token: &str, name: &str) -> Result<(String, Option<PromptBackup>)> {
    let login = github::account(token).await?.login;
    let Some(repo) = github::own_repo(token, &login, name).await? else {
        return Ok((login, None));
    };
    if !repo.private {
        return Err(CoreError::GitHub(format!(
            "github.com/{login}/{name} is public, so Sawit won't keep your prompts there. Make it private on GitHub, or pick another repository."
        )));
    }
    let tree = github::fetch_tree(token, &login, name, &repo.default_branch).await?;
    Ok((
        login.clone(),
        Some(PromptBackup {
            login,
            name: name.to_string(),
            repo,
            tree,
        }),
    ))
}

/// What a backup or a restore did, and what it left for the user to choose.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SyncOutcome {
    pub repo: String,
    /// Prompts sent or brought in, by name.
    pub synced: Vec<String>,
    /// The same prompts, by id.
    pub synced_ids: Vec<String>,
    /// Changed on both sides: neither was overwritten.
    pub conflicts: Vec<PromptSync>,
    pub created_repo: bool,
}

/// Sends the prompts changed here (and those only here) to the repository, creating it the
/// first time, with the README. `keep_local`: ids changed on both sides whose version here wins.
pub async fn back_up(
    token: &str,
    library: &Library,
    settings: &BackupSettings,
    state: &Path,
    keep_local: &[String],
) -> Result<SyncOutcome> {
    let (login, found) = fetch_backup(token, &settings.repo).await?;
    let (backup, created) = match found {
        Some(b) => (b, false),
        None => {
            let repo = github::create_private_repo(
                token,
                &settings.repo,
                "Prompts from Sawit's prompt library: one Markdown file each.",
            )
            .await?;
            let backup = PromptBackup {
                login: login.clone(),
                name: settings.repo.clone(),
                repo,
                tree: RemoteTree::default(),
            };
            (backup, true)
        }
    };
    let base = SyncBase::load(state);
    let plan = plan_prompts(library, &backup.tree, &base);
    let files = library_files(library);
    let mut out = SyncOutcome {
        repo: format!("{login}/{}", settings.repo),
        created_repo: created,
        ..Default::default()
    };
    let mut send = Vec::new();
    for p in plan {
        let chosen = p.state == FileState::ChangedOnBoth && keep_local.contains(&p.id);
        if p.state.uploads() || chosen {
            if let Some(bytes) = files.get(&p.path) {
                out.synced.push(p.title.clone());
                out.synced_ids.push(p.id.clone());
                send.push((p.path.clone(), bytes.clone()));
            }
        } else if p.state == FileState::ChangedOnBoth {
            out.conflicts.push(p);
        }
    }
    let mut synced: Vec<(String, Option<String>)> = send
        .iter()
        .map(|(path, bytes)| (path.clone(), Some(github::blob_sha(bytes))))
        .collect();
    // Same on both sides counts as synced too, so a later change is known for what it is.
    for p in plan_prompts(library, &backup.tree, &base) {
        if p.state == FileState::Same {
            synced.push((p.path, p.remote));
        }
    }
    let mut commit = backup.tree.commit.clone();
    if !send.is_empty() || created {
        send.push(("README.md".into(), readme(library).into_bytes()));
        let host = crate::shell_config::computer_name();
        let committed = github::commit_files(
            token,
            &login,
            &settings.repo,
            &backup.repo.default_branch,
            &send,
            &format!("Back up {} prompts from {host}", out.synced.len()),
        )
        .await?;
        commit = committed.commit;
    }
    let mut base = base;
    base.record(&commit, github::now(), &synced);
    base.save(state)?;
    Ok(out)
}

/// Brings in the prompts changed on GitHub (and those only there) to `library`, keeping their
/// use counts here. `keep_github`: ids changed on both sides whose GitHub version wins. The
/// caller saves the library.
pub async fn restore(
    token: &str,
    library: &mut Library,
    settings: &BackupSettings,
    state: &Path,
    keep_github: &[String],
) -> Result<SyncOutcome> {
    let (login, found) = fetch_backup(token, &settings.repo).await?;
    let backup = found.ok_or_else(|| {
        CoreError::GitHub(format!(
            "github.com/{login}/{} doesn't exist yet: back up first.",
            settings.repo
        ))
    })?;
    let base = SyncBase::load(state);
    let plan = plan_prompts(library, &backup.tree, &base);
    let mut contents = BTreeMap::new();
    for p in to_restore(&plan, keep_github) {
        if let Some(sha) = &p.remote {
            let bytes = github::fetch_blob(token, &login, &settings.repo, sha).await?;
            contents.insert(p.path.clone(), bytes);
        }
    }
    let (mut out, synced) = apply_restore(library, &plan, &contents, keep_github)?;
    out.repo = format!("{login}/{}", settings.repo);
    let mut base = base;
    base.record(&backup.tree.commit, github::now(), &synced);
    base.save(state)?;
    Ok(out)
}

/// The prompts a restore brings in: changed on GitHub, only there, or chosen (Keep GitHub's).
fn to_restore<'a>(plan: &'a [PromptSync], keep_github: &[String]) -> Vec<&'a PromptSync> {
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
        if chosen.contains(&p.path.as_str()) {
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

#[cfg(test)]
mod tests;
