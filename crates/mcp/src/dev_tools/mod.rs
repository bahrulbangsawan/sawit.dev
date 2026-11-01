//! The Tools page: developer tools that install with a one-line script
//! (Homebrew, Bun, uv, Claude Code, …), which are installed (and where) and
//! which have a newer release, installing a selection with its prerequisites
//! first, upgrading installed ones, and uninstalling them.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::dev_tools::{self, Tool};

use crate::Sawit;

mod install;
mod list;
#[cfg(test)]
mod tests;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstallToolsArgs {
    #[schemars(description = "Tool ids from list_dev_tools, e.g. [\"bun\", \"uv\"]")]
    pub tools: Vec<String>,
    #[schemars(
        description = "Must be true: installing runs each tool's official install script from the internet"
    )]
    pub confirm: bool,
    #[schemars(
        description = "For the tools npm installs globally (Gemini CLI, the Cloudflare CLI): \"npm\" (npm install -g), \"pnpm\" (pnpm add -g), \"bun\" (bun add -g) or \"yarn\" (yarn global add, Yarn 1). Default: the one picked last in Sawit, else npm"
    )]
    pub package_manager: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UpgradeToolsArgs {
    #[schemars(
        description = "Installed tool ids from list_dev_tools, e.g. [\"bun\"]. Empty or left out: every installed tool with a newer release"
    )]
    #[serde(default)]
    pub tools: Vec<String>,
    #[schemars(
        description = "Must be true: upgrading runs each tool's own upgrade command (or its install script from the internet)"
    )]
    pub confirm: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UninstallToolsArgs {
    #[schemars(
        description = "Installed tool ids from list_dev_tools, e.g. [\"bun\"]; its `uninstall` line says what each runs"
    )]
    pub tools: Vec<String>,
    #[schemars(
        description = "Must be true: uninstalling deletes each tool's files (and what it installed: nvm's Node.js versions, uv's Pythons, …), which can't be undone"
    )]
    pub confirm: bool,
}

/// Ids in `wanted` that aren't in the catalog.
fn unknown(wanted: &[String]) -> Vec<&str> {
    wanted
        .iter()
        .map(|w| w.as_str())
        .filter(|w| dev_tools::find(w).is_none())
        .collect()
}

/// "Gemini CLI needs it and will stop working", "PureMac won't get updates
/// through Homebrew any more": what the installed tools that need a removed
/// one lose, per kind ([`dev_tools::needed_by_warning`]).
fn needs_it(tools: &[&Tool]) -> String {
    dev_tools::needed_by_warning(tools)
}

#[tool_router(router = dev_tools_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List the developer tools of Sawit's Tools page (package managers, runtimes, AI agents, terminal apps) with whether each is installed, its version, the newer release when there is one, prerequisites, and its install command; for an installed one also where it was found and its upgrade and uninstall commands (with the installed tools that need it). Reads the latest versions from npm/GitHub. Works without the app"
    )]
    async fn list_dev_tools(&self) -> CallToolResult {
        self.list_dev_tools_tool().await
    }

    #[tool(
        description = "Install developer tools from list_dev_tools by id, prerequisites first, skipping installed ones. RUNS EACH TOOL'S OFFICIAL INSTALL SCRIPT FROM THE INTERNET (curl … | sh): needs confirm=true. Tools that ask for a password (Homebrew) are not run; their command is returned for a terminal. Can take minutes. Works without the app"
    )]
    async fn install_dev_tools(
        &self,
        Parameters(a): Parameters<InstallToolsArgs>,
    ) -> CallToolResult {
        self.install_dev_tools_tool(a).await
    }

    #[tool(
        description = "Upgrade installed developer tools (the Tools page's Upgrade / Upgrade All): with no ids, every installed tool that list_dev_tools shows a newer release for; with ids, those tools unless already the latest. An app installed outside what upgrades it (PureMac dragged in from its disk image rather than Homebrew's cask, Zed or Cap without their install script's command) is never upgraded. Runs each tool's own upgrade command (bun upgrade, uv self update, claude update, …) or else its install script FROM THE INTERNET: needs confirm=true. Tools that may ask for a password (Homebrew) are not run; their command is returned for a terminal. Returns one line per tool. Can take minutes. Works without the app"
    )]
    async fn upgrade_dev_tools(
        &self,
        Parameters(a): Parameters<UpgradeToolsArgs>,
    ) -> CallToolResult {
        self.upgrade_dev_tools_tool(a).await
    }

    #[tool(
        description = "Uninstall developer tools by id, as the Tools page's trash button does. DESTRUCTIVE: deletes each tool's files (and what it installed: nvm's Node.js versions, uv's Pythons, …), which can't be undone: needs confirm=true. Removes only the copy the tool's own installer, Homebrew (brew uninstall) or npm (npm uninstall -g) put in place, with the command list_dev_tools shows as `uninstall`; a copy installed another way is left alone. Homebrew's own uninstaller asks for a password, so its command is returned for a terminal. Installed tools that need a removed one are named: they stop working, except an app from a Homebrew cask, which keeps working but no longer gets updates through Homebrew. Detects again and returns one line per tool: removed, still found (another copy), not installed or left alone. Works without the app"
    )]
    async fn uninstall_dev_tools(
        &self,
        Parameters(a): Parameters<UninstallToolsArgs>,
    ) -> CallToolResult {
        self.uninstall_dev_tools_tool(a).await
    }
}
