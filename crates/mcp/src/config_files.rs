//! The Config Files panel's list: the shell startup files on this Mac and
//! every installed coding agent's config files, here and in the project, as
//! paths and what each one is. Never what a file holds: they can hold API
//! keys, and an agent reads the one it needs with its own tools.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::agent_config::{self, AgentConfig};
use sawit_core::shell_config::{self, Scope, ShellConfig};
use serde_json::{Value, json};

use crate::{Sawit, agent_path, fail, ok, project_dir, tilde};

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ListConfigArgs {
    #[schemars(
        description = "Project folder whose agent files (CLAUDE.md, .claude/settings.json, AGENTS.md, .codex/config.toml, …) are listed too. Default: the folder sawit-mcp runs in"
    )]
    pub project: Option<String>,
    #[schemars(
        description = "Only the files whose path, description, layer or shell has this text (any case), as the panel's search does; a group's name (\"Shell\", \"Codex\", \"Claude Code · work\") lists the whole group"
    )]
    pub filter: Option<String>,
}

/// The list as `list_config_files` answers it: the login shell, the projects
/// whose files were looked for, then the panel's groups in its order (the
/// shell files, then each agent and profile, highest priority first), each
/// file with what the panel says of it.
pub(crate) fn listing(
    login: Option<&str>,
    shell: &[ShellConfig],
    agents: &[AgentConfig],
    projects: &[PathBuf],
    filter: Option<&str>,
) -> Value {
    let query = filter.unwrap_or_default().trim().to_lowercase();
    // As the panel's search: everything, a group whose name has the text, or
    // the files that have it.
    let keep = |group: &str, words: &[&str]| {
        query.is_empty()
            || group.to_lowercase().contains(&query)
            || words.iter().any(|w| w.to_lowercase().contains(&query))
    };
    let mut groups = Vec::new();
    let files: Vec<Value> = shell
        .iter()
        .filter(|c| {
            let path = c.path.display().to_string();
            keep("Shell", &[&path, &tilde(&c.path), c.shell, c.note])
        })
        .map(|c| {
            json!({
                "path": c.path.display().to_string(),
                "shell": c.shell,
                "note": c.note,
                "scope": match c.scope {
                    Scope::User => "user",
                    Scope::System => "system",
                },
                "read_by_login_shell": shell_config::reads(login, c.shell),
                "writable": c.writable,
            })
        })
        .collect();
    if !files.is_empty() {
        groups.push(json!({ "group": "Shell", "files": files }));
    }
    for (client, profile, configs) in agent_config::groups(agents) {
        let title = agent_config::group_title(client, profile);
        let files: Vec<Value> = configs
            .into_iter()
            .filter(|c| {
                let path = c.path.display().to_string();
                keep(&title, &[&path, &tilde(&c.path), c.layer.label(), c.note])
            })
            .map(|c| {
                json!({
                    "path": c.path.display().to_string(),
                    "layer": c.layer.label(),
                    "note": c.note,
                    "project": c.project.as_ref().map(|p| p.display().to_string()),
                    "writable": c.writable,
                })
            })
            .collect();
        if !files.is_empty() {
            groups.push(json!({
                "group": title,
                "agent": client.name(),
                "profile": profile,
                "files": files,
            }));
        }
    }
    json!({
        "login_shell": login,
        "projects": projects.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "groups": groups,
    })
}

impl Sawit {
    pub async fn list_config_files_tool(&self, a: ListConfigArgs) -> CallToolResult {
        let project = match a
            .project
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            Some(given) => {
                let dir = agent_path(given);
                if !dir.is_dir() {
                    return fail(format!("{} is not a folder.", dir.display()));
                }
                Some(dir)
            }
            None => project_dir(None),
        };
        let projects: Vec<PathBuf> = project.into_iter().collect();
        let login = shell_config::login_shell();
        let shell = shell_config::detect();
        let agents = agent_config::detect(&projects);
        let list = listing(
            login.as_deref(),
            &shell,
            &agents,
            &projects,
            a.filter.as_deref(),
        );
        ok(serde_json::to_string_pretty(&list).unwrap_or_else(|_| list.to_string()))
    }
}

#[tool_router(router = config_files_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List the files of Sawit's Config Files panel as JSON: the shell startup files on this Mac (~/.zshrc, /etc/zprofile, …) with the shell that reads each, when, whether the login shell reads it and whether saving it needs an admin password (writable); then every installed coding agent's config files (Claude Code and Codex per profile, Gemini CLI, Cursor, …) on this Mac and in the project, each agent's highest priority first, with its layer (managed, project, user, …). Paths and what each file is, never its contents (they can hold API keys): read or edit one with your own tools, or open it in Sawit with app_open. `filter` narrows the list as the panel's search does. Works without the app"
    )]
    async fn list_config_files(&self, Parameters(a): Parameters<ListConfigArgs>) -> CallToolResult {
        self.list_config_files_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sawit_core::agent_config::Layer;
    use sawit_core::mcp_clients::Client;

    use super::*;

    fn shell_file(path: &Path, shell: &'static str, note: &'static str) -> ShellConfig {
        ShellConfig {
            path: path.to_path_buf(),
            shell,
            note,
            scope: if path.starts_with("/etc") {
                Scope::System
            } else {
                Scope::User
            },
            writable: !path.starts_with("/etc"),
        }
    }

    fn agent_file(
        client: Client,
        layer: Layer,
        path: &str,
        project: Option<&str>,
        profile: Option<&str>,
    ) -> AgentConfig {
        AgentConfig {
            client,
            layer,
            path: PathBuf::from(path),
            note: "Settings for every project",
            project: project.map(PathBuf::from),
            profile: profile.map(String::from),
            writable: true,
        }
    }

    /// The panel's groups in its order, every file by its path and what it is,
    /// and nothing a file holds: this ~/.zshrc sets an API key.
    #[test]
    fn the_list_is_the_panels_with_paths_and_never_contents() {
        let dir = tempfile::tempdir().unwrap();
        let zshrc = dir.path().join(".zshrc");
        std::fs::write(&zshrc, "export OPENAI_API_KEY=sk-sawit-test-0123456789\n").unwrap();
        let shell = [
            shell_file(&zshrc, "zsh", "Interactive zsh"),
            shell_file(
                Path::new("/Users/me/.bash_profile"),
                "bash",
                "bash login shells",
            ),
            shell_file(Path::new("/etc/zshrc"), "zsh", "Interactive zsh, all users"),
        ];
        let agents = [
            agent_file(
                Client::ClaudeCode,
                Layer::Project,
                "/code/app/CLAUDE.md",
                Some("/code/app"),
                None,
            ),
            agent_file(
                Client::ClaudeCode,
                Layer::User,
                "/Users/me/.claude/settings.json",
                None,
                None,
            ),
            agent_file(
                Client::ClaudeCode,
                Layer::User,
                "/Users/me/.claude-work/settings.json",
                None,
                Some("work"),
            ),
            agent_file(
                Client::Codex,
                Layer::User,
                "/Users/me/.codex/config.toml",
                None,
                None,
            ),
        ];
        let projects = [PathBuf::from("/code/app")];
        let list = listing(Some("zsh"), &shell, &agents, &projects, None);
        assert!(!list.to_string().contains("sk-sawit-test"), "{list}");
        assert_eq!(list["login_shell"], "zsh");
        assert_eq!(list["projects"], json!(["/code/app"]));
        let titles: Vec<&str> = list["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["group"].as_str().unwrap())
            .collect();
        assert_eq!(
            titles,
            ["Shell", "Claude Code", "Claude Code · work", "Codex"]
        );
        assert_eq!(
            list["groups"][0]["files"][0],
            json!({
                "path": zshrc.display().to_string(),
                "shell": "zsh",
                "note": "Interactive zsh",
                "scope": "user",
                "read_by_login_shell": true,
                "writable": true,
            })
        );
        // zsh doesn't read bash's files; saving /etc/zshrc needs an admin password.
        assert_eq!(list["groups"][0]["files"][1]["read_by_login_shell"], false);
        assert_eq!(list["groups"][0]["files"][2]["scope"], "system");
        assert_eq!(list["groups"][0]["files"][2]["writable"], false);
        let claude = &list["groups"][1];
        assert_eq!(claude["agent"], "Claude Code");
        assert_eq!(claude["profile"], Value::Null);
        assert_eq!(
            claude["files"][0],
            json!({
                "path": "/code/app/CLAUDE.md",
                "layer": "Project",
                "note": "Settings for every project",
                "project": "/code/app",
                "writable": true,
            })
        );
        assert_eq!(claude["files"][1]["project"], Value::Null);
        assert_eq!(list["groups"][2]["agent"], "Claude Code");
        assert_eq!(list["groups"][2]["profile"], "work");

        // The search: a group's name lists its files, else the files that have the text.
        let found = |filter: &str| -> Vec<(String, usize)> {
            let list = listing(Some("zsh"), &shell, &agents, &projects, Some(filter));
            let groups = list["groups"].as_array().unwrap().clone();
            groups
                .iter()
                .map(|g| {
                    let files = g["files"].as_array().unwrap().len();
                    (g["group"].as_str().unwrap().to_string(), files)
                })
                .collect()
        };
        assert_eq!(found("ZSH"), [("Shell".to_string(), 2)]);
        assert_eq!(found("· work"), [("Claude Code · work".to_string(), 1)]);
        assert_eq!(found(" codex "), [("Codex".to_string(), 1)]);
        assert_eq!(
            found("project"),
            [
                ("Claude Code".to_string(), 2),
                ("Claude Code · work".to_string(), 1),
                ("Codex".to_string(), 1),
            ]
        );
        assert!(found("no-such-file").is_empty());
    }

    #[tokio::test]
    async fn a_project_that_isnt_a_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "").unwrap();
        let sawit = Sawit::new(sawit_core::instances::AppPaths {
            root: dir.path().to_path_buf(),
        });
        let r = sawit
            .list_config_files_tool(ListConfigArgs {
                project: Some(file.display().to_string()),
                filter: None,
            })
            .await;
        assert_eq!(r.is_error, Some(true));
        let args: ListConfigArgs = serde_json::from_str("{}").unwrap();
        assert!(args.project.is_none() && args.filter.is_none());
    }
}
