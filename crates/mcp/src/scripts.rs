//! Run & debug: a project's scripts (package.json, Makefile, …) and the
//! commands saved for it in Sawit (`run-commands.json`).

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Store;
use sawit_core::scripts::{self, RunMode, SavedCommand, SavedCommands, ScriptGroup};

use crate::{Sawit, blocking, existing_dir, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProjectArgs {
    #[schemars(description = "Project folder (absolute path, as opened in Sawit)")]
    pub project_dir: String,
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// One after another in one terminal, stopping at the first failure.
    #[default]
    Sequence,
    /// All at once, each in its own terminal.
    Parallel,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SaveCommandArgs {
    #[schemars(description = "Project folder (absolute path, as opened in Sawit)")]
    pub project_dir: String,
    #[schemars(description = "Name shown in the Run panel")]
    pub name: String,
    #[schemars(description = "Command lines, e.g. [\"docker compose up -d\", \"bun dev\"]")]
    pub commands: Vec<String>,
    #[schemars(
        description = "sequence (default: one terminal, stop at first failure) or parallel (one terminal each)"
    )]
    #[serde(default)]
    pub mode: Mode,
    #[schemars(description = "Id of a saved command to replace (see list_scripts)")]
    pub id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RemoveCommandArgs {
    #[schemars(description = "Project folder (absolute path, as opened in Sawit)")]
    pub project_dir: String,
    #[schemars(description = "Saved command id or exact name")]
    pub id: String,
}

/// The project key `run-commands.json` uses: the folder without a trailing slash.
fn project_root(dir: &str) -> Result<PathBuf, String> {
    let path = existing_dir(dir)?;
    let text = path.to_string_lossy();
    Ok(PathBuf::from(text.trim_end_matches('/')).components().collect())
}

pub(crate) fn groups_text(groups: &[ScriptGroup]) -> Vec<String> {
    let mut out = Vec::new();
    for g in groups {
        let rel = if g.rel.is_empty() { String::new() } else { format!(" in {}", g.rel) };
        out.push(format!("{} ({}, {}{rel}):", g.label, g.tool, g.source));
        for s in &g.scripts {
            out.push(format!("  - {}: `{}`", s.name, s.command));
        }
    }
    out
}

fn saved_text(saved: &[SavedCommand]) -> Vec<String> {
    saved
        .iter()
        .map(|c| {
            format!(
                "  - {} · id {} · {}: {}",
                c.name,
                c.id,
                match c.mode {
                    RunMode::Sequence => "sequence",
                    RunMode::Parallel => "parallel",
                },
                c.commands.join(" ; ")
            )
        })
        .collect()
}

impl Sawit {
    fn run_file(&self) -> PathBuf {
        self.paths.root.join("run-commands.json")
    }

    fn saved_commands(&self) -> Result<SavedCommands, String> {
        SavedCommands::load(&self.run_file()).map_err(|e| {
            format!("{e}\nSawit won't save over this file until it is fixed or moved.")
        })
    }

    async fn list_scripts_tool(&self, dir: &str) -> CallToolResult {
        let root = tri!(project_root(dir));
        let scan_root = root.clone();
        let groups = tri!(blocking(move || scripts::scan(&scan_root)).await);
        let saved = tri!(self.saved_commands());
        let mut out = Vec::new();
        let mine = saved.for_project(&root);
        if !mine.is_empty() {
            out.push("Saved commands:".to_string());
            out.extend(saved_text(mine));
            out.push(String::new());
        }
        if groups.is_empty() {
            out.push("No scripts found (package.json, Makefile, …).".into());
        } else {
            out.extend(groups_text(&groups));
        }
        ok(out.join("\n"))
    }

    async fn save_command_tool(&self, a: SaveCommandArgs) -> CallToolResult {
        let root = tri!(project_root(&a.project_dir));
        if a.name.trim().is_empty() {
            return fail("Give the command a name.");
        }
        let mode = match a.mode {
            Mode::Sequence => RunMode::Sequence,
            Mode::Parallel => RunMode::Parallel,
        };
        let mut command = SavedCommand::new(&a.name, a.commands, mode);
        if command.commands.is_empty() {
            return fail("Give at least one command line.");
        }
        let mut saved = tri!(self.saved_commands());
        if let Some(id) = a.id.filter(|i| !i.trim().is_empty()) {
            if saved.get(&root, &id).is_none() {
                return fail(format!("No saved command with id {id} in this project."));
            }
            command.id = id;
        }
        let id = command.id.clone();
        saved.upsert(&root, command);
        tri!(saved.save(&self.run_file()).map_err(|e| e.to_string()));
        self.notify(Store::RunCommands).await;
        ok(format!("Saved “{}” (id {id}) for {}.", a.name.trim(), root.display()))
    }

    async fn remove_command_tool(&self, a: RemoveCommandArgs) -> CallToolResult {
        let root = tri!(project_root(&a.project_dir));
        let mut saved = tri!(self.saved_commands());
        let id = match find_command(saved.for_project(&root), &a.id) {
            Ok(id) => id,
            Err(e) => return fail(e),
        };
        saved.remove(&root, &id);
        tri!(saved.save(&self.run_file()).map_err(|e| e.to_string()));
        self.notify(Store::RunCommands).await;
        ok(format!("Removed saved command {}.", a.id))
    }
}

/// A saved command's id, by id or unique name.
pub(crate) fn find_command(list: &[SavedCommand], key: &str) -> Result<String, String> {
    if list.iter().any(|c| c.id == key) {
        return Ok(key.to_string());
    }
    let named: Vec<&SavedCommand> = list.iter().filter(|c| c.name == key).collect();
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(format!("No saved command “{key}” in this project. See list_scripts.")),
        _ => Err(format!("Several saved commands are named “{key}”; use the id.")),
    }
}

#[tool_router(router = scripts_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List a project's runnable scripts (package.json incl. workspaces, Makefile, …) with the command that runs each, plus the commands saved for it in Sawit's Run panel"
    )]
    async fn list_scripts(&self, Parameters(a): Parameters<ProjectArgs>) -> CallToolResult {
        self.list_scripts_tool(&a.project_dir).await
    }

    #[tool(
        description = "Save a named command (one or more command lines) for a project in Sawit's Run panel; pass id to replace one"
    )]
    async fn save_command(&self, Parameters(a): Parameters<SaveCommandArgs>) -> CallToolResult {
        self.save_command_tool(a).await
    }

    #[tool(description = "Remove a saved command from a project's Run panel")]
    async fn remove_command(
        &self,
        Parameters(a): Parameters<RemoveCommandArgs>,
    ) -> CallToolResult {
        self.remove_command_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_commands() {
        let a = SavedCommand::new("dev", vec!["bun dev".into()], RunMode::Sequence);
        let b = SavedCommand::new("test", vec!["bun test".into()], RunMode::Sequence);
        let list = vec![a.clone(), b];
        assert_eq!(find_command(&list, &a.id).unwrap(), a.id);
        assert_eq!(find_command(&list, "dev").unwrap(), a.id);
        assert!(find_command(&list, "nope").is_err());
    }

    #[test]
    fn project_key_has_no_trailing_slash() {
        let dir = tempfile::tempdir().unwrap();
        let with_slash = format!("{}/", dir.path().display());
        assert_eq!(project_root(&with_slash).unwrap(), dir.path());
        assert!(project_root("relative/path").is_err());
    }
}
