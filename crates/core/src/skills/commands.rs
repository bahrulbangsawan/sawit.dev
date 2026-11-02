//! The commands that install, update or remove a pack in the chosen agents' folders, and
//! running them.

use std::path::{Path, PathBuf};

use super::{ClaudeHome, Installed, Method, Pack, Target};
use crate::package_manager::{PackageManager, quote};

/// How `method` installs `pack`, for a list: its commands, with `<agent>`
/// where the chosen agents go.
pub fn sample(pack: &Pack, method: Method, pm: PackageManager) -> String {
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
}

impl Env {
    /// This process's.
    pub fn current() -> Env {
        Env {
            home: PathBuf::from(std::env::var("HOME").unwrap_or_default()),
            claude_config_dir_set: std::env::var_os("CLAUDE_CONFIG_DIR").is_some(),
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
            for (target, _) in places {
                let dir = env.path(&target.dir);
                lines.push(format!("mkdir -p {dir}"));
                for skill in pack.skills {
                    let into = env.path(&target.dir.join(skill));
                    // What was there goes first, so an update leaves no old files.
                    lines.push(format!("rm -rf {into}"));
                    lines.push(format!(
                        "cp -R \"$tmp/{name}/{path}/{skill}\" {dir}/",
                        path = path.trim_matches('/')
                    ));
                }
            }
            lines
        }
    })
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
/// deleted from the folder.
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
            let at = target.dir.join(skill);
            // A link whose folder is gone is there too.
            if installed.folder.contains(*skill) || at.symlink_metadata().is_ok() {
                lines.push(format!("rm -rf {}", env.path(&at)));
            }
        }
    }
    if lines.is_empty() {
        let names = places
            .iter()
            .map(|(t, _)| t.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
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

/// Runs `lines` (from [`script`]) for `pack` in the login shell, as the Dev
/// Tools page runs an installer, sending each line of output to `on_line`.
/// One script per pack at a time, from the page, an agent or `sawit`.
pub async fn run(
    pack: &Pack,
    lines: &[String],
    on_line: impl Fn(String) + Send + Sync,
) -> crate::Result<()> {
    crate::dev_tools::run_shell(
        &format!("skill-{}", pack.id),
        pack.name,
        &lines.join("\n"),
        on_line,
    )
    .await
}
