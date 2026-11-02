//! The Skills page: the catalog of agent skill packs (from their official
//! sources), the agents' skills folders on this Mac and which packs each has,
//! and installing, updating or removing a pack there, through the `skills`
//! CLI (with the package manager picked), a Claude Code plugin, a copy from a
//! git clone, or a copy of a skill Sawit ships (Built In).

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
        description = "How to install, from the pack's ways in list_skills: \"skills-cli\" (npx skills add), \"plugin\" (Claude Code plugin; Claude Code profiles only), \"copy\" (git clone and copy) or \"built-in\" (a skill Sawit ships, copied from its data folder: no download). Default: the pack's first way that reaches every agent given"
    )]
    pub method: Option<String>,
    #[schemars(
        description = "For the skills CLI: the package manager that runs it: \"npm\" (npx -y), \"pnpm\" (pnpm dlx), \"bun\" (bunx) or \"yarn\" (yarn dlx, Yarn 2+). Default: the one picked last in Sawit, else the first installed"
    )]
    pub package_manager: Option<String>,
    #[schemars(
        description = "Must be true: installing replaces the pack's skill folders in those agents' folders and, unless the pack is built in, downloads the skills (and runs the skills CLI or Claude Code's plugin install) from the internet"
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
        Status::Differs => {
            "differs from this Sawit's version: edited, or an update is available".into()
        }
        Status::Partial { have, of } => format!("{have} of {of} skills"),
        Status::Missing => "not installed".into(),
    }
}

/// The lines of a run, as having run or, when it failed before the shell started on them (the
/// pack's lock, writing the skill out), as not run.
fn commands(lines: &[String], started: bool) -> String {
    let label = if started { "Ran" } else { "Not run" };
    format!("{label}:\n{}", lines.join("\n"))
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
    /// What the commands are written for: this process's home folder, and this server's data
    /// folder, where a skill Sawit ships is written out.
    fn skills_env(&self) -> skills::Env {
        skills::Env::with_data(&self.paths)
    }

    pub async fn list_skills_tool(&self) -> CallToolResult {
        let (paths, env) = (self.paths.clone(), self.skills_env());
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
                                    skills::sample(pack, *method, pm, &env)
                                ));
                            }
                        } else {
                            let only = match method {
                                Method::Plugin { .. } => " (Claude Code profiles only)",
                                Method::Bundled { .. } => " (ships inside Sawit, no download)",
                                _ => "",
                            };
                            out.push(format!(
                                "  {}{only}: {}",
                                method.id(),
                                skills::sample(pack, *method, PackageManager::Npm, &env)
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
                "Refused: installing replaces the pack's skill folders in the agents' folders and, unless the pack is built in, downloads the skills and runs the skills CLI or Claude Code's plugin install. Pass confirm=true.",
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
        let all_there = picked.iter().all(|&t| report.status(pack, t).full());
        let action = if all_there {
            Action::Update
        } else {
            Action::Install
        };
        let places: Vec<skills::Place> = picked
            .iter()
            .map(|&t| (&report.targets[t], &report.installed[t]))
            .collect();
        let env = self.skills_env();
        let lines = tri!(skills::script(pack, method, action, &places, pm, &env));
        let kept: Vec<String> = skills::kept(pack, &places, &env)
            .iter()
            .map(|b| tilde(b))
            .collect();
        let result = skills::run(pack, action, &lines, &env, |_| {}).await;
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
        match result {
            Ok(()) => ok(format!(
                "{} {} ({}). {}.{}\n{}\nStart a new session of those agents to pick it up.",
                if action == Action::Update {
                    "Updated"
                } else {
                    "Installed"
                },
                pack.id,
                method.label(),
                states.join("; "),
                match kept.is_empty() {
                    true => String::new(),
                    false => format!(
                        " The copy that differed from this Sawit's version is kept beside it: {}.",
                        kept.join(", ")
                    ),
                },
                commands(&lines, true)
            )),
            Err(e) => fail(format!(
                "{} failed: {e}\n{}",
                pack.id,
                commands(&lines, e.started)
            )),
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
        let (env, remove) = (self.skills_env(), Action::Remove);
        let npm = PackageManager::Npm;
        let lines = match skills::script(pack, pack.methods[0], remove, &places, npm, &env) {
            Ok(lines) => lines,
            // Nothing of it there: nothing to do, not a failure.
            Err(why) => return ok(why),
        };
        // Where it had something to remove: not where only another skill of its name is.
        let from: Vec<&str> = places
            .iter()
            .filter(|place| skills::removes_from(pack, place))
            .map(|(t, _)| t.label.as_str())
            .collect();
        let result = skills::run(pack, remove, &lines, &env, |_| {}).await;
        match result {
            Ok(()) => ok(format!(
                "Removed {} from {}.\n{}",
                pack.id,
                from.join("; "),
                commands(&lines, true)
            )),
            Err(e) => fail(format!(
                "Removing {} failed: {e}\n{}",
                pack.id,
                commands(&lines, e.started)
            )),
        }
    }
}

#[tool_router(router = skills_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List the agent skill packs of Sawit's Skills page (Sawit's own Canvas Design, Superpowers, shadcn's improve, Interface Skills, unslop, Animate, MCP Apps, Cloudflare's cf CLI, …): who publishes each, its skills, its source, the exact install command for each way it installs (the skills CLI per package manager: npx, pnpm dlx, bunx, yarn dlx; a Claude Code plugin; a git clone and copy; built in: a copy of the skill Sawit ships), and which agents on this Mac have it (Claude Code per profile, Codex, Cursor, …, by skills folder). Works without the app"
    )]
    async fn list_skills(&self) -> CallToolResult {
        self.list_skills_tool().await
    }

    #[tool(
        description = "Install (or update, when every agent given has it) a skill pack from list_skills into the agents given, the way its source documents: the skills CLI run with the package manager picked (package_manager: npm, pnpm, bun or yarn), a Claude Code plugin (Claude Code profiles only), a git clone and copy, or built in for a skill Sawit ships (sawit-canvas-design: written out into Sawit's data folder and copied from there, installing this Sawit's version with no download; a copy there that differs is kept in Sawit's data folder (skill-backups/<agent>/<skill>, out of the agents' reach), and a folder of that name that isn't Sawit's is left alone). Replaces the pack's skill folders and, unless the pack is built in, downloads from the internet: needs confirm=true. Returns the commands it ran and each agent's state after. Works without the app"
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
mod tests;
