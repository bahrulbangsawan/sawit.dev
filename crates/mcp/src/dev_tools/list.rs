//! `list_dev_tools`: the Tools page's catalog with what is installed, where, and at which version,
//! the newer releases, and for an installed tool what upgrades and what removes it.

use rmcp::model::CallToolResult;
use sawit_core::dev_tools::{self, Category, Removal, TOOLS, Tool};
use sawit_core::package_manager::PackageManager;

use super::needs_it;
use crate::{Sawit, ok, tilde};

/// What `list_dev_tools` says upgrades an installed tool: its upgrade
/// command, or none for an app installed outside what upgrades it.
pub(super) fn upgrade_line(tool: &Tool, found: &dev_tools::Found) -> String {
    match found.outside {
        Some(outside) => format!(
            "none: installed outside {}, so Sawit leaves this copy alone",
            outside.of()
        ),
        None => tool.upgrade_command().to_string(),
    }
}

/// What `list_dev_tools` says about removing an installed tool: what
/// [`dev_tools::uninstall_plan`] would do with it alone.
pub(super) fn uninstall_line(step: &dev_tools::Uninstall) -> String {
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
                        "installed{}{}{}",
                        f.version
                            .as_ref()
                            .map(|v| format!(" {v}"))
                            .unwrap_or_default(),
                        dev_tools::newer(t.id, &found, &latest)
                            .map(|v| format!(" · {v} available (upgrade_dev_tools)"))
                            .unwrap_or_default(),
                        f.outside
                            .map(|o| format!(" · installed outside {}", o.of()))
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
                        upgrade_line(t, f),
                        dev_tools::uninstall_plan(&[t.id], &found)
                            .first()
                            .map(uninstall_line)
                            .unwrap_or_default()
                    ),
                    None => String::new(),
                };
                // The other package managers' commands, for package_manager.
                let others = match dev_tools::npm_package(t) {
                    Some(_) => format!(
                        " (package_manager: {})",
                        [
                            PackageManager::Pnpm,
                            PackageManager::Bun,
                            PackageManager::Yarn
                        ]
                        .into_iter()
                        .map(|pm| dev_tools::install_command(t, Some(pm)))
                        .collect::<Vec<_>>()
                        .join(", ")
                    ),
                    None => String::new(),
                };
                out.push(format!(
                    "- {} ({}) · {state}{needs}\n    {}\n    install: {}{others}{installed}",
                    t.id, t.name, t.description, t.install
                ));
            }
            out.push(String::new());
        }
        ok(out.join("\n").trim_end().to_string())
    }
}
