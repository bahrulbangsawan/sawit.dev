//! Installing Sawit Skills and removing them, for every agent on this Mac or for one project:
//! the Sawit pack's skills through the Skills page's own scripts, the block file by file, and
//! Sawit's MCP server through the MCP page's own install. One run at a time, from any of the app,
//! an agent or `sawit`.

use std::path::{Path, PathBuf};

use super::agents::agent_files;
use super::files::{self, Change, Done};
use super::ledger::Ledger;
use super::mcp::{self, McpLedger};
use super::project;
use super::report::{Outcome, SkillsOutcome};
use crate::dev_tools;
use crate::instances::AppPaths;
use crate::mcp_clients;
use crate::package_manager::PackageManager;
use crate::skills::{self, Action, Method, SAWIT_PACK};

/// Where a run puts the blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every agent on this Mac: the skills, each agent's own instruction file, and Sawit's MCP
    /// server where a client lacks it.
    Global,
    /// One project's instruction file ([`project`]); the skills and the MCP server are the
    /// agents', so they stay as they are.
    Project(PathBuf),
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Installs Sawit Skills in `scope`, or brings what is there up to this Sawit's version.
/// `server` is the `sawit-mcp` the MCP entries run. Fails only when nothing could start (another
/// run holds the lock, Sawit's list of files can't be read); each part's own failure is in the
/// outcome.
pub async fn install(scope: &Scope, paths: &AppPaths, server: &Path) -> Result<Outcome, String> {
    let _lock = lock()?;
    let mut ledger = Ledger::load(paths)?;
    let mut out = Outcome::default();
    match scope {
        Scope::Global => {
            install_skills(paths, &mut out).await;
            let (agent_files, skipped) = agent_files(&home(), &mcp_clients::targets());
            for f in agent_files {
                let result = files::put(&f.path, &mut ledger);
                out.files.push((f.label, f.path, result).into());
            }
            out.skipped = skipped;
            let mut added = McpLedger::load(paths)?;
            out.mcp = mcp::ensure(server, &mut added).await;
        }
        Scope::Project(dir) => {
            for path in project::files_for_put(dir) {
                let result = files::put(&path, &mut ledger);
                out.files
                    .push((project_label(dir, &path), path, result).into());
            }
        }
    }
    Ok(out)
}

/// Removes Sawit Skills from `scope`: the pack's skills (only Sawit's own copies), the blocks,
/// and, globally, the MCP entries an install added (an entry that was there before stays).
pub async fn remove(scope: &Scope, paths: &AppPaths) -> Result<Outcome, String> {
    let _lock = lock()?;
    let mut ledger = Ledger::load(paths)?;
    let mut out = Outcome::default();
    match scope {
        Scope::Global => {
            remove_skills(paths, &mut out).await;
            let (agent_files, skipped) = agent_files(&home(), &mcp_clients::targets());
            for f in agent_files {
                for path in std::iter::once(&f.path).chain(&f.also) {
                    let result = files::take(path, &mut ledger);
                    if path != &f.path && matches!(&result, Ok(c) if c.done == Done::Absent) {
                        continue;
                    }
                    out.files
                        .push((f.label.clone(), path.clone(), result).into());
                }
            }
            out.skipped = skipped;
            let mut added = McpLedger::load(paths)?;
            out.mcp = mcp::remove(&mut added).await;
        }
        Scope::Project(dir) => {
            let having = project::files_with_block(dir);
            if having.is_empty() {
                let path = dir.join(project::PROJECT_FILES[0]);
                let result: Result<Change, String> = Ok(Change {
                    done: Done::Absent,
                    backup: None,
                });
                out.files
                    .push((project_label(dir, &path), path, result).into());
            }
            for path in having {
                let result = files::take(&path, &mut ledger);
                out.files
                    .push((project_label(dir, &path), path, result).into());
            }
        }
    }
    Ok(out)
}

/// One run at a time: the app, an agent and `sawit` share it.
fn lock() -> Result<dev_tools::Lock, String> {
    dev_tools::lock("sawit-skills", "Sawit Skills").map_err(|e| e.to_string())
}

/// "This project (shop): AGENTS.md".
fn project_label(dir: &Path, path: &Path) -> String {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
    let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
    format!("{} ({rel})", name.unwrap_or_else(|| "project".into()))
}

/// The Sawit pack and what it needs from the Skills page's machinery.
fn pack() -> &'static skills::Pack {
    skills::get(SAWIT_PACK).expect("the catalog has the Sawit pack")
}

/// Copies the pack into every agent's skills folder that doesn't have this Sawit's version, in one
/// run of the Skills page's script; a folder of a skill's name that isn't Sawit's stays, and that
/// agent is said to be left out.
async fn install_skills(paths: &AppPaths, out: &mut Outcome) {
    let (pack, env) = (pack(), skills::Env::with_data(paths));
    let report = skills::report();
    let mut places = Vec::new();
    for (target, installed) in report.targets.iter().zip(&report.installed) {
        let place = (target, installed);
        let result = if let Some((_, name)) = skills::not_sawits(pack.skills, &[place]) {
            Err(format!(
                "its {name} folder isn't Sawit's skill (its SKILL.md names another): left alone"
            ))
        } else if matches!(
            skills::status(pack, installed),
            skills::Status::Installed { .. }
        ) {
            Ok("already current".to_string())
        } else {
            places.push(place);
            continue;
        };
        out.skills.push(skills_outcome(target, result));
    }
    if places.is_empty() {
        return;
    }
    let action = if places.iter().all(|(_, i)| skills::status(pack, i).full()) {
        Action::Update
    } else {
        Action::Install
    };
    let ran = run_script(pack, action, &places, &env).await;
    out.notes.extend(
        skills::kept(pack, &places, &env)
            .into_iter()
            .filter(|_| ran.is_ok())
            .map(|b| {
                format!(
                    "Kept the copy of a skill that differed from this Sawit's version: {}",
                    b.display()
                )
            }),
    );
    let after = skills::report();
    for (target, _) in places {
        let now = after
            .targets
            .iter()
            .position(|t| t.dir == target.dir)
            .map(|ix| after.status(pack, ix));
        let result = match (&ran, now) {
            (_, Some(skills::Status::Installed { .. })) => Ok(match action {
                Action::Update => "updated".to_string(),
                _ => "installed".to_string(),
            }),
            (Err(e), _) => Err(e.clone()),
            (Ok(()), _) => Err("the copy didn't finish: try again".to_string()),
        };
        out.skills.push(skills_outcome(target, result));
    }
}

/// Deletes the pack's skills (Sawit's own copies only) from every agent's skills folder.
async fn remove_skills(paths: &AppPaths, out: &mut Outcome) {
    let (pack, env) = (pack(), skills::Env::with_data(paths));
    let report = skills::report();
    let places: Vec<skills::Place> = report
        .targets
        .iter()
        .zip(&report.installed)
        .filter(|place| skills::removes_from(pack, place))
        .collect();
    if places.is_empty() {
        return;
    }
    let ran = run_script(pack, Action::Remove, &places, &env).await;
    for (target, _) in places {
        let result = match &ran {
            Ok(()) => Ok("removed".to_string()),
            Err(e) => Err(e.clone()),
        };
        out.skills.push(skills_outcome(target, result));
    }
}

async fn run_script(
    pack: &skills::Pack,
    action: Action,
    places: &[skills::Place<'_>],
    env: &skills::Env,
) -> Result<(), String> {
    let lines = skills::script(
        pack,
        Method::Bundled,
        action,
        places,
        PackageManager::Npm,
        env,
    )?;
    skills::run(pack, action, &lines, env, |_| {})
        .await
        .map_err(|e| e.to_string())
}

fn skills_outcome(target: &skills::Target, result: Result<String, String>) -> SkillsOutcome {
    SkillsOutcome {
        label: target.label.clone(),
        dir: target.dir.clone(),
        result,
    }
}

/// The `sawit-mcp` beside `exe` (the app's binary, or `sawit`): what the MCP page installs.
pub fn server_beside(exe: &Path) -> PathBuf {
    exe.with_file_name("sawit-mcp")
}
