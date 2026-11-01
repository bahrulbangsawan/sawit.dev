//! The Skills page: the catalog of agent skill packs (from their official
//! sources), the agents' skills folders on this Mac and which packs each has,
//! and installing, updating or removing a pack there, through the `skills`
//! CLI (with the package manager picked), a Claude Code plugin, or a copy
//! from a git clone.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::package_manager::{self, PackageManager, Use};
use sawit_core::skills::{self, Action, Method, Pack, Status};

use crate::{Sawit, blocking, fail, ok, tilde, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstallSkillArgs {
    #[schemars(description = "Skill pack id from list_skills, e.g. \"superpowers\" or \"cf-cli\"")]
    pub skill: String,
    #[schemars(
        description = "Agents to install into, as list_skills names them: \"Claude Code\", \"Claude Code · work\" (a profile), \"Codex, Zed\" or one of its agents (\"codex\"), \"Cursor\", or a skills folder"
    )]
    pub agents: Vec<String>,
    #[schemars(
        description = "How to install, from the pack's ways in list_skills: \"skills-cli\" (npx skills add), \"plugin\" (Claude Code plugin; Claude Code profiles only) or \"copy\" (git clone and copy). Default: the pack's first way that reaches every agent given"
    )]
    pub method: Option<String>,
    #[schemars(
        description = "For the skills CLI: the package manager that runs it: \"npm\" (npx -y), \"pnpm\" (pnpm dlx), \"bun\" (bunx) or \"yarn\" (yarn dlx, Yarn 2+). Default: the one picked last in Sawit, else the first installed"
    )]
    pub package_manager: Option<String>,
    #[schemars(
        description = "Must be true: installing downloads the skills (and runs the skills CLI or Claude Code's plugin install) from the internet"
    )]
    pub confirm: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UninstallSkillArgs {
    #[schemars(description = "Skill pack id from list_skills")]
    pub skill: String,
    #[schemars(description = "Agents to remove it from, as list_skills names them")]
    pub agents: Vec<String>,
    #[schemars(description = "Must equal `skill` exactly, to confirm")]
    pub confirm: String,
}

fn status_text(status: Status) -> String {
    match status {
        Status::Installed { plugin: true } => "installed (plugin)".into(),
        Status::Installed { plugin: false } => "installed".into(),
        Status::Partial { have, of } => format!("{have} of {of} skills"),
        Status::Missing => "not installed".into(),
    }
}

fn pick(skill: &str) -> Result<&'static Pack, String> {
    skills::get(skill).ok_or_else(|| format!("Unknown skill pack \"{skill}\". See list_skills."))
}

/// The way to install `pack` into `targets`: the one named, else its first
/// that reaches all of them.
fn method_for(
    pack: &Pack,
    given: Option<&str>,
    targets: &[&skills::Target],
) -> Result<Method, String> {
    let ways = || {
        pack.methods
            .iter()
            .map(|m| m.id())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match given {
        Some(id) => pack
            .method(id.trim())
            .ok_or_else(|| format!("{} installs with: {}.", pack.id, ways())),
        None => pack
            .methods
            .iter()
            .copied()
            .find(|m| targets.iter().all(|t| m.reaches(t)))
            .ok_or_else(|| {
                format!(
                    "None of {}'s ways ({}) reaches all of those agents: a plugin installs only into Claude Code. Install into fewer agents at a time.",
                    pack.id,
                    ways()
                )
            }),
    }
}

impl Sawit {
    pub async fn list_skills_tool(&self) -> CallToolResult {
        let paths = self.paths.clone();
        let available = package_manager::detect().await;
        let text = tri!(
            blocking(move || {
                let report = skills::report();
                let last = package_manager::remembered(&paths);
                let runners: Vec<&package_manager::Available> =
                    package_manager::usable(&available, Use::Run);
                let mut out = vec![format!(
                    "Agents found: {}",
                    if report.targets.is_empty() {
                        "none".to_string()
                    } else {
                        report
                            .targets
                            .iter()
                            .map(|t| format!("{} ({})", t.label, tilde(&t.dir)))
                            .collect::<Vec<_>>()
                            .join("; ")
                    }
                )];
                out.push(format!(
                    "Package managers for the skills CLI: {}{}",
                    if runners.is_empty() {
                        "none (install Node.js, Bun or pnpm)".to_string()
                    } else {
                        runners
                            .iter()
                            .map(|a| format!("{} ({})", a.pm.id(), a.pm.runner()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    last.map(|pm| format!("; picked last: {}", pm.id()))
                        .unwrap_or_default()
                ));
                for pack in skills::PACKS {
                    out.push(String::new());
                    out.push(format!(
                        "{} ({}) by {} · {}",
                        pack.id,
                        pack.name,
                        pack.by,
                        pack.category.name()
                    ));
                    out.push(format!("  {}", pack.about));
                    out.push(format!("  skills: {}", pack.skills.join(", ")));
                    out.push(format!("  source: {} (docs: {})", pack.repo, pack.home));
                    for method in pack.methods {
                        if method.uses_package_manager() {
                            let pms: Vec<PackageManager> = if runners.is_empty() {
                                vec![PackageManager::Npm]
                            } else {
                                runners.iter().map(|a| a.pm).collect()
                            };
                            for pm in pms {
                                out.push(format!(
                                    "  {} ({}): {}",
                                    method.id(),
                                    pm.id(),
                                    skills::sample(pack, *method, pm)
                                ));
                            }
                        } else {
                            let only = if matches!(method, Method::Plugin { .. }) {
                                " (Claude Code profiles only)"
                            } else {
                                ""
                            };
                            out.push(format!(
                                "  {}{only}: {}",
                                method.id(),
                                skills::sample(pack, *method, PackageManager::Npm)
                            ));
                        }
                    }
                    if let Some(note) = pack.note {
                        out.push(format!("  note: {note}"));
                    }
                    let installed: Vec<String> = (0..report.targets.len())
                        .filter_map(|t| {
                            let status = report.status(pack, t);
                            status.any().then(|| {
                                format!("{} ({})", report.targets[t].label, status_text(status))
                            })
                        })
                        .collect();
                    out.push(format!(
                        "  in: {}",
                        if installed.is_empty() {
                            "no agent yet".into()
                        } else {
                            installed.join("; ")
                        }
                    ));
                }
                out.join("\n")
            })
            .await
        );
        ok(text)
    }

    pub async fn install_skill_tool(&self, a: InstallSkillArgs) -> CallToolResult {
        if !a.confirm {
            return fail(
                "Refused: installing downloads the skills and runs the skills CLI or Claude Code's plugin install. Pass confirm=true.",
            );
        }
        let pack = tri!(pick(&a.skill));
        let wanted = match a.package_manager.as_deref() {
            None => None,
            Some(name) => Some(tri!(PackageManager::parse(name).ok_or_else(|| format!(
                "\"{name}\" isn't a package manager: npm, pnpm, bun or yarn."
            )))),
        };
        let report = tri!(blocking(skills::report).await);
        let picked = tri!(skills::find_targets(&report.targets, &a.agents));
        if picked.is_empty() {
            return fail("Name the agents to install into: see list_skills.");
        }
        let targets: Vec<&skills::Target> = picked.iter().map(|&t| &report.targets[t]).collect();
        let method = tri!(method_for(pack, a.method.as_deref(), &targets));
        let available = package_manager::detect().await;
        let pm = if method.uses_package_manager() {
            let usable = package_manager::usable(&available, Use::Run);
            match package_manager::choose(
                &available,
                wanted,
                package_manager::remembered(&self.paths),
                Use::Run,
            ) {
                Some(pm) if wanted.is_none_or(|w| w == pm) => pm,
                Some(_) | None if usable.is_empty() => {
                    return fail(
                        "The skills CLI runs with npx, pnpm, bunx or yarn dlx, and none is installed. Install Node.js first (install_dev_tools with \"node\").",
                    );
                }
                _ => {
                    return fail(format!(
                        "{} isn't installed here (or, for Yarn 1, has no dlx). Installed: {}.",
                        wanted.map_or("That", |w| w.id()),
                        usable
                            .iter()
                            .map(|a| a.pm.id())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        } else {
            PackageManager::Npm
        };
        let all_there = picked
            .iter()
            .all(|&t| matches!(report.status(pack, t), Status::Installed { .. }));
        let action = if all_there {
            Action::Update
        } else {
            Action::Install
        };
        let places: Vec<skills::Place> = picked
            .iter()
            .map(|&t| (&report.targets[t], &report.installed[t]))
            .collect();
        let lines = tri!(skills::script(
            pack,
            method,
            action,
            &places,
            pm,
            &skills::Env::current()
        ));
        let result = skills::run(pack, &lines, |_| {}).await;
        let after = tri!(blocking(skills::report).await);
        let states: Vec<String> = picked
            .iter()
            .map(|&t| {
                format!(
                    "{}: {}",
                    report.targets[t].label,
                    status_text(after.status(pack, t))
                )
            })
            .collect();
        let ran = format!("Ran:\n{}", lines.join("\n"));
        match result {
            Ok(()) => ok(format!(
                "{} {} ({}). {}.\n{ran}\nStart a new session of those agents to pick it up.",
                if action == Action::Update {
                    "Updated"
                } else {
                    "Installed"
                },
                pack.id,
                method.label(),
                states.join("; ")
            )),
            Err(e) => fail(format!("{} failed: {e}\n{ran}", pack.id)),
        }
    }

    pub async fn uninstall_skill_tool(&self, a: UninstallSkillArgs) -> CallToolResult {
        if a.confirm != a.skill {
            return fail(format!(
                "Refused: to remove “{}”, pass confirm=\"{}\".",
                a.skill, a.skill
            ));
        }
        let pack = tri!(pick(&a.skill));
        let report = tri!(blocking(skills::report).await);
        let picked = tri!(skills::find_targets(&report.targets, &a.agents));
        if picked.is_empty() {
            return fail("Name the agents to remove it from: see list_skills.");
        }
        let places: Vec<skills::Place> = picked
            .iter()
            .map(|&t| (&report.targets[t], &report.installed[t]))
            .collect();
        let lines = match skills::script(
            pack,
            pack.methods[0],
            Action::Remove,
            &places,
            PackageManager::Npm,
            &skills::Env::current(),
        ) {
            Ok(lines) => lines,
            // Nothing of it there: nothing to do, not a failure.
            Err(why) => return ok(why),
        };
        let result = skills::run(pack, &lines, |_| {}).await;
        let ran = format!("Ran:\n{}", lines.join("\n"));
        match result {
            Ok(()) => ok(format!(
                "Removed {} from {}.\n{ran}",
                pack.id,
                picked
                    .iter()
                    .map(|&t| report.targets[t].label.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
            Err(e) => fail(format!("Removing {} failed: {e}\n{ran}", pack.id)),
        }
    }
}

#[tool_router(router = skills_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List the agent skill packs of Sawit's Skills page (Superpowers, Interface Skills, unslop, Animate, MCP Apps, Cloudflare's cf CLI, …): who publishes each, its skills, its source, the exact install command for each way it installs (the skills CLI per package manager: npx, pnpm dlx, bunx, yarn dlx; a Claude Code plugin; a git clone and copy), and which agents on this Mac have it (Claude Code per profile, Codex, Cursor, …, by skills folder). Works without the app"
    )]
    async fn list_skills(&self) -> CallToolResult {
        self.list_skills_tool().await
    }

    #[tool(
        description = "Install (or update, when every agent given has it) a skill pack from list_skills into the agents given, the way its source documents: the skills CLI run with the package manager picked (package_manager: npm, pnpm, bun or yarn), a Claude Code plugin (Claude Code profiles only), or a git clone and copy. Downloads from the internet: needs confirm=true. Returns the commands it ran and each agent's state after. Works without the app"
    )]
    async fn install_skill(&self, Parameters(a): Parameters<InstallSkillArgs>) -> CallToolResult {
        self.install_skill_tool(a).await
    }

    #[tool(
        description = "Remove a skill pack from the agents given, as the Skills page does: uninstalls its Claude Code plugin where it's installed and deletes its skill folders (or links) from those agents' skills folders, nothing else. DESTRUCTIVE: `confirm` must equal `skill`. Works without the app"
    )]
    async fn uninstall_skill(
        &self,
        Parameters(a): Parameters<UninstallSkillArgs>,
    ) -> CallToolResult {
        self.uninstall_skill_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(result: &CallToolResult) -> String {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    }

    fn sawit() -> Sawit {
        Sawit::new(sawit_core::instances::AppPaths {
            root: std::env::temp_dir(),
        })
    }

    /// Nothing is read, let alone run, until the agent confirms and names a pack of the catalog
    /// and a package manager there is.
    #[tokio::test]
    async fn installs_and_removals_refuse_before_anything_runs() {
        let sawit = sawit();
        let install = |skill: &str, pm: Option<&str>, confirm: bool| {
            sawit.install_skill_tool(InstallSkillArgs {
                skill: skill.into(),
                agents: vec!["Claude Code".into()],
                method: None,
                package_manager: pm.map(String::from),
                confirm,
            })
        };
        let r = install("superpowers", None, false).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("confirm=true"), "{}", said(&r));
        let r = install("sawit-palsu", None, true).await;
        assert!(said(&r).contains("Unknown skill pack"), "{}", said(&r));
        let r = install("cf-cli", Some("deno"), true).await;
        assert!(said(&r).contains("isn't a package manager"), "{}", said(&r));
        let r = sawit
            .uninstall_skill_tool(UninstallSkillArgs {
                skill: "superpowers".into(),
                agents: vec!["Claude Code".into()],
                confirm: "yes".into(),
            })
            .await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("confirm=\"superpowers\""), "{}", said(&r));
    }

    /// The way is the one named when the pack has it, else its first that reaches every agent:
    /// a plugin never goes to an agent other than Claude Code.
    #[test]
    fn the_way_reaches_every_agent_given() {
        let claude = skills::Target {
            label: "Claude Code".into(),
            clients: vec![],
            dir: "/h/.claude/skills".into(),
            claude: Some(skills::ClaudeHome {
                dir: "/h/.claude".into(),
                profile_dir: None,
            }),
            cli_agent: Some("claude-code"),
        };
        let cursor = skills::Target {
            label: "Cursor".into(),
            clients: vec![],
            dir: "/h/.cursor/skills".into(),
            claude: None,
            cli_agent: Some("cursor"),
        };
        let mcp_apps = skills::get("mcp-apps").unwrap();
        assert_eq!(
            method_for(mcp_apps, None, &[&claude]).unwrap().id(),
            "plugin"
        );
        assert_eq!(
            method_for(mcp_apps, None, &[&claude, &cursor])
                .unwrap()
                .id(),
            "skills-cli"
        );
        assert_eq!(
            method_for(mcp_apps, Some("skills-cli"), &[&claude])
                .unwrap()
                .id(),
            "skills-cli"
        );
        assert!(method_for(mcp_apps, Some("copy"), &[&claude]).is_err());
        let superpowers = skills::get("superpowers").unwrap();
        assert!(method_for(superpowers, None, &[&cursor]).is_err());
    }

    #[test]
    fn args_parse() {
        let a: InstallSkillArgs = serde_json::from_str(
            r#"{"skill":"cf-cli","agents":["codex"],"package_manager":"bun","confirm":true}"#,
        )
        .unwrap();
        assert_eq!(a.package_manager.as_deref(), Some("bun"));
        assert!(a.method.is_none());
        assert!(
            serde_json::from_str::<UninstallSkillArgs>(r#"{"skill":"x","agents":[]}"#).is_err()
        );
    }
}
