//! The Tools page: developer tools that install with a one-line script
//! (Homebrew, Bun, uv, Claude Code, …), which are installed (and where) and
//! which have a newer release, installing a selection with its prerequisites
//! first, upgrading installed ones, and uninstalling them.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::dev_tools::{self, Category, Removal, TOOLS, Tool};

use crate::{Sawit, fail, ok, tilde};

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

/// "Gemini CLI needs it", "npm (latest), Gemini CLI need it".
fn needs_it(tools: &[&Tool]) -> String {
    let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
    let s = if names.len() == 1 { "s" } else { "" };
    format!("{} need{s} it and will stop working", names.join(", "))
}

/// What `list_dev_tools` says about removing an installed tool: what
/// [`dev_tools::uninstall_plan`] would do with it alone.
fn uninstall_line(step: &dev_tools::Uninstall) -> String {
    let mut line = match &step.removal {
        Removal::Missing => return String::new(),
        Removal::LeftAlone => {
            "none: this copy was installed another way, so Sawit leaves it alone".to_string()
        }
        Removal::Bundled(with) => format!(
            "none: it comes with {}, and goes when that is uninstalled",
            with.join(", ")
        ),
        Removal::Terminal(command) => {
            format!("{command} (asks for a password: run it in a terminal)")
        }
        Removal::Run(command) => format!("{command} (uninstall_dev_tools)"),
    };
    if !step.needed_by.is_empty() {
        line.push_str(&format!("; {}", needs_it(&step.needed_by)));
    }
    format!("\n    uninstall: {}", line.replace('\n', "\n      "))
}

impl Sawit {
    pub async fn list_dev_tools_tool(&self) -> CallToolResult {
        let (detection, latest) = tokio::join!(dev_tools::detect(), dev_tools::latest_versions());
        let found = detection.found;
        let mut out = Vec::new();
        if let Some(problem) = &detection.problem {
            out.push(format!(
                "Note: the login shell couldn't say which tools it finds ({problem}). Only tools at their usual places show as installed; others may be installed too.\n"
            ));
        }
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
                let installed = match found.get(t.id) {
                    Some(f) => format!(
                        "\n    found: {}\n    upgrade: {}{}",
                        tilde(&f.path),
                        t.upgrade_command(),
                        dev_tools::uninstall_plan(&[t.id], &found)
                            .first()
                            .map(uninstall_line)
                            .unwrap_or_default()
                    ),
                    None => String::new(),
                };
                out.push(format!(
                    "- {} ({}) · {state}{needs}\n    {}\n    install: {}{installed}",
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
        let detection = dev_tools::detect().await;
        if let Some(problem) = &detection.problem {
            // Which tools are missing is unknown: installing could run an
            // installer over a copy that's there.
            return fail(format!(
                "Refused: the login shell couldn't say which tools are installed ({problem}), so nothing was run. Try again."
            ));
        }
        let found = detection.found;
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
        let (detection, latest) = tokio::join!(dev_tools::detect(), dev_tools::latest_versions());
        let found = detection.found;
        let wanted: Vec<&str> = a.tools.iter().map(|t| t.as_str()).collect();
        let plan = dev_tools::upgrade_plan(&wanted, &found, &latest);
        let mut out = Vec::new();
        if let Some(problem) = &detection.problem {
            out.push(format!(
                "Note: the login shell couldn't say which tools it finds ({problem}); only tools at their usual places were considered."
            ));
        }
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

    pub async fn uninstall_dev_tools_tool(&self, a: UninstallToolsArgs) -> CallToolResult {
        if !a.confirm {
            return fail(
                "Refused: uninstalling deletes each tool's files, which can't be undone. list_dev_tools shows what each uninstall runs and what needs the tool; pass confirm=true.",
            );
        }
        if a.tools.is_empty() {
            return fail("Name the tools to uninstall: ids from list_dev_tools.");
        }
        let bad = unknown(&a.tools);
        if !bad.is_empty() {
            return fail(format!(
                "Unknown tool {}. See list_dev_tools.",
                bad.join(", ")
            ));
        }
        let detection = dev_tools::detect().await;
        if let Some(problem) = &detection.problem {
            // Which copy is the one in use is unknown: an uninstaller could
            // remove one the user didn't mean.
            return fail(format!(
                "Refused: the login shell couldn't say where the tools are ({problem}), so nothing was removed. Try again."
            ));
        }
        let wanted: Vec<&str> = a.tools.iter().map(|t| t.as_str()).collect();
        let plan = dev_tools::uninstall_plan(&wanted, &detection.found);
        // One line per tool, in the order they ran; `None` until detection
        // says what a run uninstaller left.
        let mut lines: Vec<Option<String>> = Vec::new();
        let mut failed = false;
        for step in &plan {
            let id = step.tool.id;
            lines.push(match &step.removal {
                Removal::Missing => Some(format!("- {id}: not installed; nothing to remove")),
                Removal::LeftAlone => Some(format!(
                    "- {id}: found at {}, which its installer, Homebrew or npm didn't put there; Sawit leaves it alone",
                    step.found.as_deref().map(tilde).unwrap_or_default()
                )),
                Removal::Bundled(with) => Some(format!(
                    "- {id}: it comes with {}, so it isn't uninstalled on its own; uninstalling that removes it",
                    with.join(", ")
                )),
                Removal::Terminal(command) => {
                    failed = true;
                    Some(format!(
                        "- {id}: not run, it asks for a password. Run it in a terminal (terminal_run): {command}"
                    ))
                }
                Removal::Run(command) => match dev_tools::uninstall(step.tool, command, |_| {}).await
                {
                    Ok(()) => None,
                    Err(e) => {
                        failed = true;
                        Some(format!("- {id}: failed. {e}"))
                    }
                },
            });
        }
        if lines.iter().any(Option::is_none) {
            // As the page does: a copy installed another way is still found.
            let after = dev_tools::detect().await;
            for (step, line) in plan.iter().zip(lines.iter_mut()) {
                if line.is_some() {
                    continue;
                }
                let id = step.tool.id;
                let mut text = match after.found.get(id) {
                    Some(f) => {
                        failed = true;
                        format!(
                            "- {id}: its uninstaller ran, but it is still found at {}: that copy was installed another way (Homebrew, say)",
                            tilde(&f.path)
                        )
                    }
                    None => format!("- {id}: removed"),
                };
                if !step.needed_by.is_empty() {
                    text.push_str(&format!(". {}", needs_it(&step.needed_by)));
                }
                *line = Some(text);
            }
            if let Some(problem) = &after.problem {
                lines.push(Some(format!(
                    "Note: the login shell couldn't check again ({problem}); only the usual places were looked at."
                )));
            }
        }
        let text = lines.into_iter().flatten().collect::<Vec<_>>().join("\n");
        if failed { fail(text) } else { ok(text) }
    }
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
        description = "Upgrade installed developer tools (the Tools page's Upgrade / Upgrade All): with no ids, every installed tool that list_dev_tools shows a newer release for; with ids, those tools unless already the latest. Runs each tool's own upgrade command (bun upgrade, uv self update, claude update, …) or else its install script FROM THE INTERNET: needs confirm=true. Tools that may ask for a password (Homebrew) are not run; their command is returned for a terminal. Returns one line per tool. Can take minutes. Works without the app"
    )]
    async fn upgrade_dev_tools(
        &self,
        Parameters(a): Parameters<UpgradeToolsArgs>,
    ) -> CallToolResult {
        self.upgrade_dev_tools_tool(a).await
    }

    #[tool(
        description = "Uninstall developer tools by id, as the Tools page's trash button does. DESTRUCTIVE: deletes each tool's files (and what it installed: nvm's Node.js versions, uv's Pythons, …), which can't be undone: needs confirm=true. Removes only the copy the tool's own installer, Homebrew (brew uninstall) or npm (npm uninstall -g) put in place, with the command list_dev_tools shows as `uninstall`; a copy installed another way is left alone. Homebrew's own uninstaller asks for a password, so its command is returned for a terminal. Installed tools that need a removed one are named: they stop working. Detects again and returns one line per tool: removed, still found (another copy), not installed or left alone. Works without the app"
    )]
    async fn uninstall_dev_tools(
        &self,
        Parameters(a): Parameters<UninstallToolsArgs>,
    ) -> CallToolResult {
        self.uninstall_dev_tools_tool(a).await
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

    fn said(result: &CallToolResult) -> String {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn uninstall_args_need_confirm() {
        let a: UninstallToolsArgs =
            serde_json::from_str(r#"{"tools":["bun"],"confirm":true}"#).unwrap();
        assert_eq!(a.tools, ["bun"]);
        assert!(a.confirm);
        assert!(serde_json::from_str::<UninstallToolsArgs>(r#"{"tools":["bun"]}"#).is_err());
    }

    /// Nothing is detected, let alone run, until the agent confirms and names
    /// tools of the catalog.
    #[tokio::test]
    async fn uninstall_refuses_before_anything_runs() {
        let sawit = Sawit::new(sawit_core::instances::AppPaths {
            root: std::env::temp_dir(),
        });
        let refused = |tools: &[&str], confirm: bool| {
            let tools = tools.iter().map(|t| t.to_string()).collect();
            sawit.uninstall_dev_tools_tool(UninstallToolsArgs { tools, confirm })
        };
        let r = refused(&["bun"], false).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("confirm=true"), "{}", said(&r));
        let r = refused(&[], true).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("Name the tools"), "{}", said(&r));
        let r = refused(&["bun", "sawit-palsu"], true).await;
        assert_eq!(r.is_error, Some(true));
        assert!(
            said(&r).contains("Unknown tool sawit-palsu"),
            "{}",
            said(&r)
        );
    }

    /// list_dev_tools tells an agent what uninstall_dev_tools would run for an
    /// installed tool, and what would stop working, before it asks the user.
    #[test]
    fn the_list_says_how_each_tool_is_removed() {
        let tool = |id: &str| dev_tools::find(id).unwrap();
        let line = |id: &str, removal: Removal, needed_by: Vec<&'static Tool>| {
            uninstall_line(&dev_tools::Uninstall {
                tool: tool(id),
                found: Some("/nonexistent-sawit-test/bin/x".into()),
                removal,
                needed_by,
            })
        };
        assert_eq!(
            line(
                "nvm",
                Removal::Run(r#"rm -rf "$NVM_DIR""#.into()),
                vec![tool("node")]
            ),
            "\n    uninstall: rm -rf \"$NVM_DIR\" (uninstall_dev_tools); Node.js needs it and will stop working"
        );
        // Each line of a longer uninstaller under the first.
        assert_eq!(
            line(
                "uv",
                Removal::Run("uv cache clean\nrm -f uv".into()),
                vec![]
            ),
            "\n    uninstall: uv cache clean\n      rm -f uv (uninstall_dev_tools)"
        );
        assert!(
            line("homebrew", Removal::Terminal("uninstall.sh".into()), vec![])
                .contains("asks for a password: run it in a terminal")
        );
        assert!(
            line("python", Removal::LeftAlone, vec![]).contains("Sawit leaves it alone"),
            "{}",
            line("python", Removal::LeftAlone, vec![])
        );
        assert_eq!(
            line("npm", Removal::Bundled(vec!["Node.js"]), vec![]),
            "\n    uninstall: none: it comes with Node.js, and goes when that is uninstalled"
        );
        assert_eq!(line("bun", Removal::Missing, vec![]), "");
        assert_eq!(
            needs_it(&[tool("npm"), tool("gemini")]),
            "npm (latest), Gemini CLI need it and will stop working"
        );
    }
}
