//! The Permissions page for agents: Claude Code's permission rules in each settings file (read,
//! and changed one rule at a time with a diff, a backup and `confirm`), which of Sawit's tools
//! ask for `confirm` and what the rules say of them, the other agents' permission settings, and
//! the macOS permissions Sawit needs.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::dev_tools::extras::Privacy;
use sawit_core::permissions::{self as perms, Bucket, Change};
use serde_json::json;

use crate::{Sawit, fail, ok, project_dir, tilde};

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ListPermissionsArgs {
    #[schemars(
        description = "Project folder whose .claude/settings.json and .claude/settings.local.json are listed too. Default: the folder sawit-mcp runs in"
    )]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EditPermissionArgs {
    #[schemars(
        description = "Which Claude Code settings file: \"user\" (~/.claude/settings.json), \"user:<profile>\" (a CLAUDE_CONFIG_DIR profile's), \"project\" (<project>/.claude/settings.json, shared with the team) or \"local\" (<project>/.claude/settings.local.json, only you). The managed file is never changed"
    )]
    pub file: String,
    #[schemars(
        description = "Project folder for \"project\" and \"local\". Default: the folder sawit-mcp runs in"
    )]
    pub project: Option<String>,
    #[schemars(description = "\"add\", \"remove\" or \"move\"")]
    pub action: String,
    #[schemars(
        description = "The rule, as Claude Code writes it: Bash(npm run test:*), Read(~/notes/**), WebFetch(domain:example.com), mcp__sawit__execute_sql. Rules that look like they hold a secret are refused"
    )]
    pub rule: String,
    #[schemars(description = "For add and move: \"allow\", \"ask\" or \"deny\"")]
    pub to: Option<String>,
    #[schemars(
        description = "Must be true to save. Without it nothing is written and the diff the change would make is returned"
    )]
    #[serde(default)]
    pub confirm: bool,
}

/// The projects a tool's `project` names, or the one sawit-mcp runs in.
fn projects(project: Option<&str>) -> Vec<PathBuf> {
    project_dir(project).into_iter().collect()
}

fn files_for(projects: &[PathBuf]) -> Vec<perms::RulesFile> {
    perms::claude_files(
        &perms::claude_profiles(),
        projects,
        std::path::Path::new(perms::MANAGED),
    )
}

impl Sawit {
    pub async fn list_permissions_tool(&self, a: ListPermissionsArgs) -> CallToolResult {
        let projects = projects(a.project.as_deref());
        let files = files_for(&projects);
        let claude: Vec<_> = files
            .iter()
            .map(|f| {
                json!({
                    "file": f.scope.id(),
                    "label": f.scope.label(),
                    "path": tilde(&f.path),
                    "project": f.scope.project().map(|p| p.display().to_string()),
                    "exists": f.exists,
                    "editable": f.editable,
                    "problem": f.problem,
                    "allow": f.rules.allow,
                    "ask": f.rules.ask,
                    "deny": f.rules.deny,
                })
            })
            .collect();
        let tools: Vec<_> = perms::sawit_tool_states(&files)
            .into_iter()
            .map(|s| {
                let (state, by) = match &s.rule {
                    Some((b, rule, file)) => (b.key(), Some(format!("{rule} in {file}"))),
                    None => ("ask", None),
                };
                json!({ "tool": s.tool, "confirm_needed_for": s.what, "claude_code": state, "rule": by })
            })
            .collect();
        let macos: Vec<_> = Privacy::ALL
            .iter()
            .map(|p| {
                json!({
                    "permission": p.name(),
                    "granted": p.state(),
                    "why": p.why(),
                    "settings": p.settings_url(),
                })
            })
            .collect();
        let others: Vec<_> = perms::other_agents(&projects)
            .into_iter()
            .map(|o| json!({ "agent": o.agent, "path": tilde(&o.path), "settings": o.lines }))
            .collect();
        ok(json!({
            "claude_code": claude,
            "sawit_tools_needing_confirm": tools,
            "macos": macos,
            "other_agents": others,
            "note": "Claude Code applies deny, then ask, then allow; a Sawit tool no rule names asks (Claude Code's default for MCP tools). Sawit's own confirm=true is asked for besides. Other agents' settings are shown, not edited. macOS 'granted' is null where macOS doesn't tell without asking."
        })
        .to_string())
    }

    pub async fn edit_permission_tool(&self, a: EditPermissionArgs) -> CallToolResult {
        let projects = projects(a.project.as_deref());
        let files = files_for(&projects);
        if a.file.trim().eq_ignore_ascii_case("managed") {
            return fail(
                "The managed settings come from your organization: Sawit never changes them.",
            );
        }
        let Some(file) = perms::find_file(&files, &a.file, projects.first().map(|p| p.as_path()))
        else {
            let known: Vec<String> = files.iter().map(|f| f.scope.id()).collect();
            return fail(format!(
                "No Claude Code settings file \"{}\". The files: {}{}.",
                a.file,
                known.join(", "),
                if projects.is_empty() {
                    " (pass project for project and local)"
                } else {
                    ""
                }
            ));
        };
        if let Some(problem) = &file.problem {
            return fail(format!("{} can't be edited: {problem}", tilde(&file.path)));
        }
        if !file.editable {
            return fail(format!("{} isn't writable.", tilde(&file.path)));
        }
        let to = a.to.as_deref().map(Bucket::parse);
        let rule = a.rule.trim().to_string();
        let change = match (a.action.trim().to_lowercase().as_str(), to) {
            ("add", Some(Some(to))) => Change::Add { rule, to },
            ("move", Some(Some(to))) => Change::Move { rule, to },
            ("remove", _) => Change::Remove { rule },
            ("add" | "move", _) => return fail("Pass to: \"allow\", \"ask\" or \"deny\"."),
            (other, _) => {
                return fail(format!("\"{other}\" isn't an action: add, remove or move."));
            }
        };
        let plan = match perms::plan(&file.path, &change) {
            Ok(plan) => plan,
            Err(e) => return fail(e),
        };
        if !a.confirm {
            return fail(format!(
                "Not saved: pass confirm=true to save. {} in {} would change it so:\n{}",
                change.describe(),
                tilde(&file.path),
                plan.diff
            ));
        }
        if let Err(e) = perms::save(&plan) {
            return fail(e.to_string());
        }
        let backup = perms::backup_path(&file.path);
        ok(format!(
            "{} in {}.{}\n{}",
            change.describe(),
            tilde(&file.path),
            if plan.before.is_empty() {
                " The file is new.".to_string()
            } else {
                format!(" The file as it was is in {}.", tilde(&backup))
            },
            plan.diff
        ))
    }
}

#[tool_router(router = permissions_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List what agents may do on this Mac, as Sawit's Permissions page shows it: Claude Code's permission rules (permissions.allow / ask / deny) in each settings file (user and each profile, the project's shared and local files, managed), which of Sawit's own tools need confirm=true and what Claude Code's rules say of each, the other agents' permission settings (Codex's approval_policy and sandbox_mode, Gemini CLI's and Qwen Code's tools.allowed / exclude, OpenCode's permission; read only), and the macOS permissions Sawit needs with their state. Never prints secrets: settings files' env blocks aren't shown. Works without the app"
    )]
    async fn list_permissions(
        &self,
        Parameters(a): Parameters<ListPermissionsArgs>,
    ) -> CallToolResult {
        self.list_permissions_tool(a).await
    }

    #[tool(
        description = "Add, remove or move one Claude Code permission rule between allow, ask and deny in one settings file (user, user:<profile>, project or local), as the Permissions page does. Without confirm=true nothing is written and the diff is returned; with it, the file is backed up beside itself (<file>.sawit-backup) and only the rule's line changes: every other key, the order and the formatting stay. Refuses the managed file, a file that isn't JSON, and rules that look like secrets. Works without the app"
    )]
    async fn edit_permission(
        &self,
        Parameters(a): Parameters<EditPermissionArgs>,
    ) -> CallToolResult {
        self.edit_permission_tool(a).await
    }
}
