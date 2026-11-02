//! The commands that install, update or remove a pack in the chosen agents' folders, and
//! running them.

use std::path::{Path, PathBuf};

use super::backups::backup;
use super::bundled::{self, Held};
use super::{ClaudeHome, Installed, Method, Pack, Target};
use crate::instances::AppPaths;
use crate::package_manager::{PackageManager, quote};
use crate::{CoreError, dev_tools};

/// How `method` installs `pack`, for a list: its commands, with `<agent>`
/// where the chosen agents go, and for a skill Sawit ships the folder in
/// `env`'s data folder it is copied from.
pub fn sample(pack: &Pack, method: Method, pm: PackageManager, env: &Env) -> String {
    match method {
        Method::SkillsCli { source, all } => {
            let mut args = vec!["skills", "add", source];
            if all {
                args.extend(["--skill", "*"]);
            } else {
                for skill in pack.skills {
                    args.extend(["--skill", skill]);
                }
            }
            format!("{} -g -a <agent> -y", pm.run_line(&args))
        }
        Method::Plugin {
            repo,
            marketplace,
            plugin,
        } => format!(
            "claude plugin marketplace add {repo} && claude plugin install {plugin}@{marketplace}"
        ),
        Method::Copy { url, path } => {
            let name = url.rsplit('/').next().unwrap_or("repo");
            format!(
                "git clone --depth 1 {url} && cp -R {name}/{}/<skill> <agent's skills folder>/",
                path.trim_matches('/')
            )
        }
        Method::Bundled { name } => format!(
            "cp -R {} <agent's skills folder>/",
            env.path(&env.bundled.join(name))
        ),
    }
}

/// What a script does to a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    /// Fetches the latest, as an install over what's there does: a plugin
    /// installed already is updated, a folder copied again.
    Update,
    Remove,
}

impl Action {
    pub fn verb(self) -> &'static str {
        match self {
            Action::Install => "Install",
            Action::Update => "Update",
            Action::Remove => "Remove",
        }
    }
}

/// What the commands depend on besides the pack and its targets.
#[derive(Debug, Clone)]
pub struct Env {
    /// The home folder: paths in it are written `"$HOME/…"`.
    pub home: PathBuf,
    /// This process has `CLAUDE_CONFIG_DIR` set, so a command for the default
    /// Claude Code profile must unset it.
    pub claude_config_dir_set: bool,
    /// Where the skills Sawit ships are written out for an install to copy,
    /// one folder each: `bundled-skills` in Sawit's data folder.
    pub bundled: PathBuf,
}

impl Env {
    /// This process's, with Sawit's data folder (`SAWIT_HOME`, else its
    /// default).
    pub fn current() -> Env {
        Env::with_data(&AppPaths::default_macos())
    }

    /// This process's, with the Sawit data folder `paths`.
    pub fn with_data(paths: &AppPaths) -> Env {
        Env {
            home: PathBuf::from(std::env::var("HOME").unwrap_or_default()),
            claude_config_dir_set: std::env::var_os("CLAUDE_CONFIG_DIR").is_some(),
            bundled: paths.bundled_skills_dir(),
        }
    }

    /// `path` for the shell, `"$HOME/…"` when it's in the home folder.
    pub(super) fn path(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) if !self.home.as_os_str().is_empty() => {
                let rest = rest.to_string_lossy();
                if rest.contains(['"', '$', '`', '\\']) {
                    quote(&path.to_string_lossy())
                } else if rest.is_empty() {
                    "\"$HOME\"".into()
                } else {
                    format!("\"$HOME/{rest}\"")
                }
            }
            _ => quote(&path.to_string_lossy()),
        }
    }

    /// What a Claude Code command for `claude` runs with: the profile's
    /// `CLAUDE_CONFIG_DIR`, or for the default profile the variable unset
    /// when this process has it.
    fn claude_prefix(&self, claude: &ClaudeHome) -> String {
        match &claude.profile_dir {
            Some(dir) => format!("CLAUDE_CONFIG_DIR={} ", self.path(dir)),
            None if self.claude_config_dir_set => "env -u CLAUDE_CONFIG_DIR ".into(),
            None => String::new(),
        }
    }
}

/// One target of a script, with what it has.
pub type Place<'a> = (&'a Target, &'a Installed);

/// The shell script that does `action` with `pack` in `places`, one command a
/// line: what the page shows before it runs, and what runs. `method` is how
/// to install or update (a removal reads what's there instead); `pm` runs the
/// `skills` CLI. Fails when nothing applies, saying why.
pub fn script(
    pack: &Pack,
    method: Method,
    action: Action,
    places: &[Place],
    pm: PackageManager,
    env: &Env,
) -> Result<Vec<String>, String> {
    if places.is_empty() {
        return Err("Pick at least one agent.".into());
    }
    if action == Action::Remove {
        return remove_script(pack, places, env);
    }
    if !pack.methods.contains(&method) {
        return Err(format!(
            "{} doesn't install with the {}.",
            pack.name,
            method.label()
        ));
    }
    if let Some((t, _)) = places.iter().find(|(t, _)| !method.reaches(t)) {
        return Err(match method {
            Method::Plugin { .. } => format!(
                "{} isn't a Claude Code profile: a plugin installs only into Claude Code.",
                t.label
            ),
            _ => format!("The {} can't install into {}.", method.label(), t.label),
        });
    }
    Ok(match method {
        Method::SkillsCli { source, all } => skills_cli_lines(pack, source, all, places, pm, env),
        Method::Plugin {
            repo,
            marketplace,
            plugin,
        } => {
            let key = format!("{plugin}@{marketplace}");
            let mut lines = Vec::new();
            for (target, installed) in places {
                let Some(claude) = &target.claude else {
                    continue;
                };
                let run = format!("{}claude plugin", env.claude_prefix(claude));
                // A marketplace added before is read again, for the latest.
                if installed.marketplaces.contains(marketplace) {
                    lines.push(format!("{run} marketplace update {}", quote(marketplace)));
                } else {
                    lines.push(format!("{run} marketplace add {}", quote(repo)));
                }
                if installed.plugins.contains(&key) {
                    lines.push(format!("{run} update {}", quote(&key)));
                } else {
                    lines.push(format!("{run} install {}", quote(&key)));
                }
            }
            lines
        }
        Method::Copy { url, path } => {
            let name = url.rsplit('/').next().unwrap_or("repo");
            let clone = format!("\"$tmp/{name}\"");
            let mut lines = vec![
                "tmp=\"$(mktemp -d)\"".to_string(),
                "trap 'rm -rf \"$tmp\"' EXIT".to_string(),
                format!("git clone --depth 1 {} {clone}", quote(url)),
            ];
            let path = path.trim_matches('/');
            let from: Vec<(&str, String)> = pack
                .skills
                .iter()
                .map(|skill| (*skill, format!("\"$tmp/{name}/{path}/{skill}\"")))
                .collect();
            lines.extend(copy_lines(&from, places, env));
            lines
        }
        Method::Bundled { name } => bundled_lines(name, places, env)?,
    })
}

/// Copies the skill `name` Sawit ships from the folder [`run`] writes it out into first, into
/// each place: the lines stop before anything is removed when that folder isn't there (copied and
/// run without Sawit); a copy that differs from this Sawit's version is kept in Sawit's data folder
/// ([`backup`]), an older one replaced; a folder of that name that isn't Sawit's is never touched,
/// so that place fails.
fn bundled_lines(name: &str, places: &[Place], env: &Env) -> Result<Vec<String>, String> {
    if let Some((target, _)) = places
        .iter()
        .find(|(_, i)| i.bundled.get(name) == Some(&Held::Other))
    {
        return Err(format!(
            "{} has a {name} folder that isn't Sawit's skill (its SKILL.md names another, or none), and Sawit leaves it alone: move it away to install there.",
            target.label
        ));
    }
    let from = env.path(&env.bundled.join(name));
    // Its messages are in double quotes, holding nothing the shell reads there.
    let missing = "is missing: Sawit writes the skill out there as it installs it, so install it from Sawit (its Skills page, or install_skill).";
    let mut lines = vec![format!(
        "[ -d {from} ] || {{ echo {from} \"{missing}\" >&2; exit 1; }}"
    )];
    for (target, installed) in places {
        let dir = env.path(&target.dir);
        let at = env.path(&target.dir.join(name));
        lines.push(format!("mkdir -p {dir}"));
        if installed.bundled.get(name) == Some(&Held::Differs) {
            let kept = backup(env, target, name);
            let parent = env.path(kept.parent().unwrap_or(&kept));
            let bak = env.path(&kept);
            lines.push(format!("rm -rf {bak}"));
            lines.push(format!("mkdir -p {parent}"));
            lines.push(format!("mv {at} {bak}"));
            let kept = "Kept the copy that differs from this Sawit's version as";
            lines.push(format!("echo \"{kept}\" {bak}"));
        } else {
            lines.push(format!("rm -rf {at}"));
        }
        lines.push(format!("cp -R {from} {dir}/"));
    }
    Ok(lines)
}

/// Copies each skill folder of `from` (the skill, and its folder as the shell reads it) into the
/// skills folder of every place, replacing what was there.
fn copy_lines(from: &[(&str, String)], places: &[Place], env: &Env) -> Vec<String> {
    let mut lines = Vec::new();
    for (target, _) in places {
        let dir = env.path(&target.dir);
        lines.push(format!("mkdir -p {dir}"));
        for (skill, folder) in from {
            // What was there goes first, so an update leaves no old files.
            lines.push(format!("rm -rf {}", env.path(&target.dir.join(skill))));
            lines.push(format!("cp -R {folder} {dir}/"));
        }
    }
    lines
}

/// `skills add` for `places`: one run for the default Claude Code profile and
/// every other agent together, and one per other Claude Code profile, whose
/// `CLAUDE_CONFIG_DIR` tells the CLI where its folder is.
fn skills_cli_lines(
    pack: &Pack,
    source: &str,
    all: bool,
    places: &[Place],
    pm: PackageManager,
    env: &Env,
) -> Vec<String> {
    let mut picks: Vec<&str> = Vec::new();
    if all {
        picks.extend(["--skill", "*"]);
    } else {
        for skill in pack.skills {
            picks.extend(["--skill", skill]);
        }
    }
    let line = |prefix: String, agents: &[&str]| {
        let mut args = vec!["skills", "add", source];
        args.extend(&picks);
        args.push("-g");
        for agent in agents {
            args.extend(["-a", agent]);
        }
        args.push("-y");
        format!("{prefix}{}", pm.run_line(&args))
    };
    let mut lines = Vec::new();
    let mut shared: Vec<&str> = Vec::new();
    let mut prefix = String::new();
    for (target, _) in places {
        let Some(agent) = target.cli_agent else {
            continue;
        };
        match &target.claude {
            Some(claude) if claude.profile_dir.is_some() => {
                lines.push(line(env.claude_prefix(claude), &[agent]));
            }
            Some(claude) => {
                prefix = env.claude_prefix(claude);
                if !shared.contains(&agent) {
                    shared.push(agent);
                }
            }
            None => {
                if !shared.contains(&agent) {
                    shared.push(agent);
                }
            }
        }
    }
    if !shared.is_empty() {
        lines.insert(0, line(prefix, &shared));
    }
    lines
}

/// Removes `pack` from `places` as it is there: its plugin uninstalled from a
/// Claude Code profile that has it, and each of its skill folders (or links)
/// deleted from the folder; a skill Sawit ships only where its SKILL.md is
/// Sawit's, or a link to nothing is there.
fn remove_script(pack: &Pack, places: &[Place], env: &Env) -> Result<Vec<String>, String> {
    let key = pack.plugin().and_then(|m| m.plugin_key());
    let mut lines = Vec::new();
    for (target, installed) in places {
        if let (Some(claude), Some(key)) = (&target.claude, &key)
            && installed.plugins.contains(key)
        {
            lines.push(format!(
                "{}claude plugin uninstall {}",
                env.claude_prefix(claude),
                quote(key)
            ));
        }
        for skill in pack.skills {
            if removes(pack, skill, target, installed) {
                lines.push(format!("rm -rf {}", env.path(&target.dir.join(skill))));
            }
        }
    }
    if lines.is_empty() {
        let names = places
            .iter()
            .map(|(t, _)| t.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        // A folder of its name that another skill has.
        if let Some(name) = pack.bundled()
            && let Some((t, _)) = places
                .iter()
                .find(|(_, i)| i.bundled.get(name) == Some(&Held::Other))
        {
            return Err(format!(
                "The {name} folder in {} isn't Sawit's skill (its SKILL.md names another, or none): Sawit leaves it alone, so there's nothing to remove.",
                t.label
            ));
        }
        // Its skills are there, but another pack's plugin brings them.
        if places
            .iter()
            .any(|(_, i)| pack.skills.iter().any(|s| i.from_plugins.contains(*s)))
        {
            return Err(format!(
                "{}'s skills come from another pack's Claude Code plugin in {names}: remove that pack instead.",
                pack.name
            ));
        }
        return Err(format!(
            "{} isn't in {names}: nothing to remove.",
            pack.name
        ));
    }
    Ok(lines)
}

/// Why [`run`] failed, and whether its lines had started: taking the pack's lock and writing
/// the skill out come before any of them runs.
#[derive(Debug)]
pub struct Failure {
    pub error: CoreError,
    /// The shell started on the lines.
    pub started: bool,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

/// Whether removing `pack` does anything in `place`: uninstalls its plugin there, or deletes a
/// skill folder of its own ([`removes`]).
pub fn removes_from(pack: &Pack, place: &Place) -> bool {
    let (target, installed) = *place;
    let key = pack.plugin().and_then(|m| m.plugin_key());
    let plugin = target.claude.is_some() && key.is_some_and(|k| installed.plugins.contains(&k));
    plugin
        || pack
            .skills
            .iter()
            .any(|s| removes(pack, s, target, installed))
}

/// Whether a removal of `pack` deletes its `skill` from `target`'s folder: a folder or link of
/// that name (a link whose folder is gone too); for a skill Sawit ships, only a copy whose
/// SKILL.md is Sawit's, or a link to nothing.
fn removes(pack: &Pack, skill: &str, target: &Target, installed: &Installed) -> bool {
    let at = target.dir.join(skill);
    let dangling = at.symlink_metadata().is_ok() && at.metadata().is_err();
    match pack.bundled() {
        Some(_) => installed
            .bundled
            .get(skill)
            .map_or(dangling, |held| *held != Held::Other),
        None => installed.folder.contains(skill) || at.symlink_metadata().is_ok(),
    }
}

/// Runs `lines` (from [`script`]) that do `action` with `pack` in the login
/// shell, as the Dev Tools page runs an installer, sending each line of output
/// to `on_line`. One script per pack at a time, from the page, an agent or
/// `sawit`: the pack's lock is taken first and held through the script.
/// Installing or updating a skill Sawit ships writes it out into
/// `env.bundled` under that lock ([`bundled::write_out`]), where the lines
/// copy it from, so no other run rewrites it while they do.
pub async fn run(
    pack: &Pack,
    action: Action,
    lines: &[String],
    env: &Env,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<(), Failure> {
    let before = |error| Failure {
        error,
        started: false,
    };
    let lock = dev_tools::lock(&format!("skill-{}", pack.id), pack.name).map_err(before)?;
    if let Some(name) = pack.bundled()
        && action != Action::Remove
    {
        bundled::write_out(name, &env.bundled).map_err(|e| {
            before(CoreError::Config(format!(
                "Sawit couldn't write out {} into {}: {e}",
                pack.name,
                env.bundled.display()
            )))
        })?;
    }
    dev_tools::run_shell_holding(&lock, pack.name, &lines.join("\n"), on_line)
        .await
        .map_err(|error| Failure {
            error,
            started: true,
        })
}
