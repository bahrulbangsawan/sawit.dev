//! `manage_agent_account`: rename, reorder or remove a Claude Code account in Sawit's list, as the
//! Accounts page's account menu does, with the same `sawit_core::agent_accounts` store as
//! `sawit account rename|move|remove`. A running app reads the list again.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::agent_accounts::{Accounts, AgentKind};

use crate::agent_accounts::{ctx, pick};
use crate::{Sawit, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ManageArgs {
    #[schemars(description = "The account: its name or id, as list_agent_accounts lists them")]
    pub account: String,
    #[schemars(
        description = "rename (to `name`), move_up or move_down (its place, and so its cpN number), or remove (from Sawit's list: its folder and sign-in stay on disk; needs confirm)"
    )]
    pub action: String,
    #[schemars(description = "For rename: the new name")]
    pub name: Option<String>,
    #[schemars(description = "Must be true for remove")]
    #[serde(default)]
    pub confirm: bool,
}

#[tool_router(router = agent_accounts_manage_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Rename, reorder or remove a Claude Code account in Sawit's list, as the Accounts page's account menu does: action rename (with name), move_up, move_down (its place sets its cpN shortcut), or remove (needs confirm: true; only Sawit's entry goes, the account's folder and sign-in stay, and adopting the folder brings it back). Answers the list after. Works without the app; a running app shows the change"
    )]
    async fn manage_agent_account(&self, Parameters(a): Parameters<ManageArgs>) -> CallToolResult {
        let (root, _) = ctx(self);
        let mut all = tri!(Accounts::load(&root).map_err(|e| e.to_string()));
        let acc = tri!(pick(&all, Some(&a.account), &root)).clone();
        let said = match a.action.trim().to_lowercase().as_str() {
            "rename" => {
                let Some(name) = a.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
                    return fail("rename needs `name`, the new name.");
                };
                let named = tri!(all.rename(&acc.id, name).map_err(|e| e.to_string()));
                format!("Renamed {} to {named}.", acc.name)
            }
            "move_up" | "move_down" => {
                let delta = if a.action.trim().eq_ignore_ascii_case("move_up") {
                    -1
                } else {
                    1
                };
                match all.move_by(&acc.id, delta) {
                    true => format!("Moved {}.", acc.name),
                    false => {
                        return fail(format!("{} is already at that end of the list.", acc.name));
                    }
                }
            }
            "remove" => {
                if !a.confirm {
                    return fail(format!(
                        "Removing takes {} off Sawit's list: pass confirm: true. Its folder and sign-in stay.",
                        acc.name
                    ));
                }
                all.remove(&acc.id);
                format!(
                    "Removed {} from Sawit's list; {} stays on disk.",
                    acc.name,
                    acc.dir.display()
                )
            }
            other => {
                return fail(format!(
                    "Unknown action “{other}”: use rename, move_up, move_down or remove."
                ));
            }
        };
        tri!(all.save(&root).map_err(|e| e.to_string()));
        self.notify(sawit_core::app_control::Store::Accounts).await;
        let list: Vec<String> = all
            .of(AgentKind::ClaudeCode)
            .iter()
            .enumerate()
            .map(|(i, a)| format!("cp{} {}", i + 1, a.name))
            .collect();
        ok(format!(
            "{said}
{}",
            list.join("\n")
        ))
    }
}
