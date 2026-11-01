//! Changing what is installed: installing tools with their prerequisites first, upgrading
//! installed ones, and uninstalling them, each with the tool's own scripts and commands.

use std::collections::HashMap;

use rmcp::model::CallToolResult;
use sawit_core::dev_tools::{self, Removal};
use sawit_core::package_manager::{self, PackageManager, Use};

use super::{InstallToolsArgs, UninstallToolsArgs, UpgradeToolsArgs, needs_it, unknown};
use crate::{Sawit, fail, ok, tilde};

/// Why `upgrade_dev_tools` didn't upgrade a tool it was given: an app
/// installed outside what upgrades it, one already the latest, or one not
/// installed.
pub(super) fn not_upgraded(id: &str, found: &HashMap<&'static str, dev_tools::Found>) -> String {
    match found.get(id) {
        Some(f) => match f.outside {
            Some(outside) => format!(
                "- {id}: installed outside {} ({}), so Sawit leaves it alone: not upgraded",
                outside.of(),
                tilde(&f.path)
            ),
            None => format!("- {id}: already the latest"),
        },
        None => format!("- {id}: not installed (install_dev_tools)"),
    }
}

impl Sawit {
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
        let wanted_pm = match a.package_manager.as_deref() {
            None => None,
            Some(name) => match PackageManager::parse(name) {
                Some(pm) => Some(pm),
                None => {
                    return fail(format!(
                        "\"{name}\" isn't a package manager: npm, pnpm, bun or yarn."
                    ));
                }
            },
        };
        let available = package_manager::detect().await;
        let pm = package_manager::choose(
            &available,
            wanted_pm,
            package_manager::remembered(&self.paths),
            Use::Global,
        );
        if let Some(w) = wanted_pm
            && pm != Some(w)
            && a.tools.iter().any(|id| {
                dev_tools::find(id)
                    .and_then(dev_tools::npm_package)
                    .is_some()
            })
        {
            return fail(format!(
                "{} isn't installed here (or, for Yarn 2+, has no global installs). Installed: {}.",
                w.id(),
                package_manager::usable(&available, Use::Global)
                    .iter()
                    .map(|a| a.pm.id())
                    .collect::<Vec<_>>()
                    .join(", ")
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
            let command = dev_tools::install_command(tool, pm);
            match dev_tools::run(tool, &command, |_| {}).await {
                Ok(()) if command != tool.install => {
                    out.push(format!("- {}: installed ({command})", tool.id))
                }
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
            out.push(not_upgraded(id, &found));
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
            // An npm tool upgrades with the package manager that installed it.
            let command = match found.get(tool.id) {
                Some(f) => {
                    dev_tools::upgrade_command(tool, Some(dev_tools::installed_with(&f.path)))
                }
                None => tool.upgrade_command().to_string(),
            };
            match dev_tools::run(tool, &command, |_| {}).await {
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
