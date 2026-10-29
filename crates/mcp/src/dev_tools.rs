//! The Tools page: developer tools that install with a one-line script
//! (Homebrew, Bun, uv, Claude Code, …), which are installed and which have a
//! newer release, installing a selection with its prerequisites first, and
//! upgrading installed ones.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::dev_tools::{self, Category, TOOLS};

use crate::{Sawit, fail, ok};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstallToolsArgs {
    #[schemars(description = "Tool ids from list_dev_tools, e.g. [\"bun\", \"uv\"]")]
    pub tools: Vec<String>,
    #[schemars(
        description = "Must be true: installing runs each tool's official install script from the internet"
    )]
    pub confirm: bool,
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

/// Ids in `wanted` that aren't in the catalog.
fn unknown(wanted: &[String]) -> Vec<&str> {
    wanted
        .iter()
        .map(|w| w.as_str())
        .filter(|w| dev_tools::find(w).is_none())
        .collect()
}

impl Sawit {
    pub async fn list_dev_tools_tool(&self) -> CallToolResult {
        let (found, latest) = tokio::join!(dev_tools::detect(), dev_tools::latest_versions());
        let mut out = Vec::new();
        for category in Category::ALL {
            out.push(category.title().to_string());
            for t in TOOLS.iter().filter(|t| t.category == category) {
                let state = match found.get(t.id) {
                    Some(f) => format!(
                        "installed{}{}",
                        f.version
                            .as_ref()
                            .map(|v| format!(" {v}"))
                            .unwrap_or_default(),
                        dev_tools::newer(t.id, &found, &latest)
                            .map(|v| format!(" · {v} available (upgrade_dev_tools)"))
                            .unwrap_or_default()
                    ),
                    None if t.interactive => {
                        "not installed (asks for a password: run `install` in a terminal)".into()
                    }
                    None => "not installed".into(),
                };
                let needs = if t.needs.is_empty() {
                    String::new()
                } else {
                    format!(" · needs {}", t.needs.join(", "))
                };
                let upgrade = if found.contains_key(t.id) {
                    format!("\n    upgrade: {}", t.upgrade_command())
                } else {
                    String::new()
                };
                out.push(format!(
                    "- {} ({}) · {state}{needs}\n    {}\n    install: {}{upgrade}",
                    t.id, t.name, t.description, t.install
                ));
            }
            out.push(String::new());
        }
        ok(out.join("\n").trim_end().to_string())
    }

    pub async fn install_dev_tools_tool(&self, a: InstallToolsArgs) -> CallToolResult {
        if !a.confirm {
            return fail(
                "Refused: installing runs each tool's install script from the internet. Pass confirm=true.",
            );
        }
        let bad = unknown(&a.tools);
        if !bad.is_empty() {
            return fail(format!(
                "Unknown tool {}. See list_dev_tools.",
                bad.join(", ")
            ));
        }
        let found = dev_tools::detect().await;
        let wanted: Vec<&str> = a.tools.iter().map(|t| t.as_str()).collect();
        let plan = dev_tools::plan(&wanted, &|id| found.contains_key(id));
        let mut out = Vec::new();
        for t in wanted.iter().filter(|id| found.contains_key(**id)) {
            out.push(format!("- {t}: already installed"));
        }
        let mut failed = false;
        for tool in plan.iter().filter(|t| !found.contains_key(t.id)) {
            if tool.interactive {
                // It asks for a password, which only a terminal can answer.
                out.push(format!(
                    "- {}: not run, it asks for a password. Run it in a terminal (terminal_run): {}",
                    tool.id, tool.install
                ));
                failed = true;
                break;
            }
            match dev_tools::install(tool, |_| {}).await {
                Ok(()) => out.push(format!("- {}: installed", tool.id)),
                Err(e) => {
                    out.push(format!("- {}: failed. {e}", tool.id));
                    failed = true;
                    // What comes after may need it.
                    break;
                }
            }
        }
        if !failed {
            out.push("Open a new terminal so it sees the new PATH.".into());
        }
        let text = out.join("\n");
        if failed { fail(text) } else { ok(text) }
    }

    pub async fn upgrade_dev_tools_tool(&self, a: UpgradeToolsArgs) -> CallToolResult {
        if !a.confirm {
            return fail(
                "Refused: upgrading runs each tool's upgrade command or install script. Pass confirm=true.",
            );
        }
        let bad = unknown(&a.tools);
        if !bad.is_empty() {
            return fail(format!(
                "Unknown tool {}. See list_dev_tools.",
                bad.join(", ")
            ));
        }
        let (found, latest) = tokio::join!(dev_tools::detect(), dev_tools::latest_versions());
        let wanted: Vec<&str> = a.tools.iter().map(|t| t.as_str()).collect();
        let plan = dev_tools::upgrade_plan(&wanted, &found, &latest);
        let mut out = Vec::new();
        for id in wanted
            .iter()
            .filter(|id| !plan.iter().any(|t| t.id == **id))
        {
            out.push(if found.contains_key(id) {
                format!("- {id}: already the latest")
            } else {
                format!("- {id}: not installed (install_dev_tools)")
            });
        }
        if plan.is_empty() && wanted.is_empty() {
            out.push("Every installed tool is up to date.".into());
        }
        let mut failed = false;
        for tool in plan {
            let to = dev_tools::newer(tool.id, &found, &latest)
                .map(|v| format!(" to {v}"))
                .unwrap_or_default();
            if tool.interactive {
                // Its installer may ask for a password; only a terminal can answer.
                out.push(format!(
                    "- {}: not run, it may ask for a password. Run it in a terminal (terminal_run): {}",
                    tool.id,
                    tool.upgrade_command()
                ));
                failed = true;
                continue;
            }
            match dev_tools::run(tool, tool.upgrade_command(), |_| {}).await {
                Ok(()) => out.push(format!("- {}: upgraded{to}", tool.id)),
                Err(e) => {
                    out.push(format!("- {}: failed. {e}", tool.id));
                    failed = true;
                }
            }
        }
        let text = out.join("\n");
        if failed { fail(text) } else { ok(text) }
    }
}

#[tool_router(router = dev_tools_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List the developer tools of Sawit's Tools page (package managers, runtimes, AI agents, terminal apps) with whether each is installed, its version, the newer release when there is one, prerequisites, and its install and upgrade commands. Reads the latest versions from npm/GitHub. Works without the app"
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
        description = "Upgrade installed developer tools (the Tools page's Upgrade / Upgrade All): with no ids, every installed tool that list_dev_tools shows a newer release for; with ids, those tools unless already the latest. Runs each tool's own upgrade command (bun upgrade, uv self update, claude update, …) or else its install script FROM THE INTERNET: needs confirm=true. Tools that may ask for a password (Homebrew) are not run; their command is returned for a terminal. Returns one line per tool. Can take minutes. Works without the app"
    )]
    async fn upgrade_dev_tools(
        &self,
        Parameters(a): Parameters<UpgradeToolsArgs>,
    ) -> CallToolResult {
        self.upgrade_dev_tools_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_ids_are_reported() {
        let first = TOOLS[0].id.to_string();
        assert!(unknown(std::slice::from_ref(&first)).is_empty());
        assert_eq!(unknown(&[first, "nope".into()]), ["nope"]);
    }

    #[test]
    fn upgrade_args() {
        let a: UpgradeToolsArgs = serde_json::from_str(r#"{"confirm":true}"#).unwrap();
        assert!(a.tools.is_empty() && a.confirm);
        let a: UpgradeToolsArgs =
            serde_json::from_str(r#"{"tools":["bun"],"confirm":false}"#).unwrap();
        assert_eq!(a.tools, ["bun"]);
    }

    #[tokio::test]
    async fn upgrade_needs_confirm() {
        let r = Sawit::new(sawit_core::instances::AppPaths {
            root: std::env::temp_dir(),
        })
        .upgrade_dev_tools_tool(UpgradeToolsArgs {
            tools: vec![],
            confirm: false,
        })
        .await;
        assert_eq!(r.is_error, Some(true));
    }
}
