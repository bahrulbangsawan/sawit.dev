//! Whether a Worker's installed dev tool serves Local Explorer, and what to tell someone when it
//! doesn't: the version it needs and the command that upgrades it.

use std::path::Path;

use serde_json::Value;

use super::config::read_config;
use super::{DevTool, Version, WorkersProject};
use crate::package_manager::PackageManager;
use crate::scripts::read_json;

/// Whether the Worker's installed dev tool serves Local Explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// It does; `cron` says whether it can also invoke Cron Triggers.
    Ready { cron: bool },
    /// The installed version is older than the first one with Local Explorer.
    TooOld { installed: Version, needed: Version },
    /// Nothing says: the tool isn't installed yet, its version doesn't parse, or it is `cf`,
    /// which starts another tool.
    Unknown,
}

/// Whether `project`'s installed dev tool serves Local Explorer.
pub fn support(project: &WorkersProject) -> Support {
    let (Some(needed), Some(installed)) = (
        project.tool.explorer_since(),
        project.version.as_deref().and_then(Version::parse),
    ) else {
        return Support::Unknown;
    };
    if installed < needed {
        return Support::TooOld { installed, needed };
    }
    Support::Ready {
        cron: project.tool.cron_since().is_some_and(|v| installed >= v),
    }
}

/// What to tell someone about `project`'s dev tool and Local Explorer, when there is something:
/// that its version has none and the command that upgrades it, that Cron Triggers need a newer
/// one, or that it isn't installed yet. `root` is the project's.
pub fn requirement_note(project: &WorkersProject, root: &Path) -> Option<String> {
    let tool = project.tool;
    match support(project) {
        Support::TooOld { installed, needed } => Some(format!(
            "{} {installed} has no Local Explorer: it needs {needed} or later. Upgrade with `{}`.",
            capitalized(tool.name()),
            upgrade_command(project, root)
        )),
        Support::Ready { cron: false } if declares_crons(project) => {
            tool.cron_since().map(|since| {
                format!(
                    "Invoking Cron Triggers from Local Explorer needs {} {since} or later; upgrade with `{}`.",
                    tool.name(),
                    upgrade_command(project, root)
                )
            })
        }
        Support::Unknown if project.version.is_none() && tool != DevTool::CfCli => {
            tool.explorer_since().map(|since| {
                format!(
                    "{} isn't in node_modules yet (install the project's dependencies); Local Explorer needs {since} or later.",
                    capitalized(tool.name())
                )
            })
        }
        _ => None,
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Whether `project`'s config declares Cron Triggers (`triggers.crons`, or
/// `triggers.scheduled(…)` in a `cloudflare.config.ts`).
pub fn declares_crons(project: &WorkersProject) -> bool {
    read_config(&project.config).crons
}

/// The command that brings `project`'s dev tool up to date, with the package manager the project
/// uses (its `packageManager` field, else its lockfile, else npm): `pnpm add -D wrangler@latest`.
/// `root` is the project's, where a workspace keeps its lockfile. Sawit shows it and runs it only
/// when asked.
pub fn upgrade_command(project: &WorkersProject, root: &Path) -> String {
    let manifest = read_json(&project.root.join("package.json")).unwrap_or(Value::Null);
    let pm = PackageManager::parse(crate::scripts::package_manager(
        &project.root,
        root,
        &manifest,
    ))
    .unwrap_or(PackageManager::Npm);
    let package = format!("{}@latest", project.tool.package());
    let mut command = pm.dev_install(&package);
    // pnpm adds to a workspace's root only when told it means to.
    if pm == PackageManager::Pnpm && project.root.join("pnpm-workspace.yaml").is_file() {
        command.push_str(" -w");
    }
    command
}
