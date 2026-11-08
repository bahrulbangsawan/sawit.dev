//! Sawit's own MCP tools that act only with `confirm: true`, and what Claude Code's rules say
//! of each.

use super::{Bucket, RulesFile};

/// Every `sawit` MCP tool that refuses unless called with `confirm: true`, and what it does that
/// makes it ask. A test in `crates/mcp` keeps this list equal to the tools' own parameters.
pub const CONFIRM_TOOLS: &[(&str, &str)] = &[
    (
        "app_close_all_tabs",
        "Closes every center tab, discarding unsaved changes with force",
    ),
    ("delete_prompt", "Deletes a prompt from the library"),
    ("drop_database", "Drops a PostgreSQL database"),
    ("drop_relation", "Drops a table, view or other relation"),
    ("edit_permission", "Changes a Claude Code permission rule"),
    (
        "git_discard",
        "Throws away files' unstaged changes, with no undo",
    ),
    ("git_merge", "Aborts a merge, throwing away what it left"),
    ("execute_sql", "Runs SQL that can change or delete data"),
    (
        "install_dev_tools",
        "Runs install scripts from the internet",
    ),
    (
        "install_sawit_skills",
        "Writes Sawit's skills into agents' folders",
    ),
    (
        "install_skill",
        "Installs or updates a skill pack in agents' folders",
    ),
    ("keymap_bind", "Changes a key binding"),
    ("keymap_unbind", "Removes a key binding"),
    (
        "manage_agent_account",
        "Takes a Claude Code account off Sawit's list",
    ),
    (
        "local_explorer_request",
        "Sends a request that can change a local Workers resource",
    ),
    ("remove_command", "Deletes a saved Run command"),
    ("remove_connection", "Deletes a saved database connection"),
    (
        "remove_instance",
        "Deletes a PostgreSQL instance and its data",
    ),
    ("remove_ssh_host", "Deletes a saved SSH host"),
    ("stack_create", "Creates a project, running its installer"),
    ("stop_processes", "Stops the processes on a port"),
    (
        "terminal_close",
        "Closes a terminal, ending what runs there",
    ),
    ("truncate_table", "Empties a table"),
    ("uninstall_dev_tools", "Deletes an installed developer tool"),
    (
        "uninstall_mcp_server",
        "Removes an MCP server from an agent's config",
    ),
    (
        "uninstall_skill",
        "Removes a skill pack from agents' folders",
    ),
    (
        "update_ssh_host",
        "Changes a saved SSH host, its saved password too",
    ),
    ("upgrade_dev_tools", "Runs upgrades and install scripts"),
    ("write_env", "Writes DATABASE_URL into a project's .env"),
];

/// What Claude Code's rules make of one Sawit tool: the bucket of the strongest rule that
/// matches it (deny over ask over allow, as Claude Code decides), and the rule and file; none
/// matching means it asks, Claude Code's default for MCP tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolState {
    pub tool: &'static str,
    pub what: &'static str,
    /// `None`: no rule names it.
    pub rule: Option<(Bucket, String, String)>,
}

/// Whether `rule` names the MCP tool `tool` of the `sawit` server: `mcp__sawit__<tool>`, the
/// whole server (`mcp__sawit`, `mcp__sawit__*`), or a wildcard ending (`mcp__sawit__app_*`).
pub fn matches(rule: &str, tool: &str) -> bool {
    let rule = rule.trim();
    let full = format!("mcp__sawit__{tool}");
    rule == full
        || rule == "mcp__sawit"
        || rule
            .strip_suffix('*')
            .is_some_and(|prefix| prefix.starts_with("mcp__sawit") && full.starts_with(prefix))
}

/// [`CONFIRM_TOOLS`] with what the rules in `files` (Claude Code's, in its order) say of each.
pub fn sawit_tool_states(files: &[RulesFile]) -> Vec<ToolState> {
    CONFIRM_TOOLS
        .iter()
        .map(|(tool, what)| {
            let mut found = None;
            for bucket in [Bucket::Deny, Bucket::Ask, Bucket::Allow] {
                found = files.iter().find_map(|f| {
                    f.rules
                        .get(bucket)
                        .iter()
                        .find(|r| matches(r, tool))
                        .map(|r| (bucket, r.clone(), f.scope.label()))
                });
                if found.is_some() {
                    break;
                }
            }
            ToolState {
                tool,
                what,
                rule: found,
            }
        })
        .collect()
}
