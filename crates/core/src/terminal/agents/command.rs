//! Telling an agent from what a process runs: its command's name, the package or folder it
//! was installed in, and what an interpreter, package runner or wrapper runs in turn.

use super::AgentKind;
use super::known::KNOWN;

/// Programs that run another program named in their arguments: interpreters,
/// package runners and wrappers such as `caffeinate`. What they run is the
/// first argument that isn't an option (or a subcommand word of a runner).
const LAUNCHERS: &[&str] = &[
    "node",
    "nodejs",
    "bun",
    "deno",
    "tsx",
    "ts-node",
    "npx",
    "bunx",
    "pnpx",
    "npm",
    "pnpm",
    "yarn",
    "uv",
    "uvx",
    "pipx",
    "ruby",
    "perl",
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "env",
    "arch",
    "caffeinate",
    "nice",
    "nohup",
    "time",
    "sudo",
];

/// Subcommand words of the package runners, which come before the package:
/// `npm exec`, `pnpm dlx`, `yarn dlx`, `uv tool run`, `pipx run`, `deno run`.
const RUNNER_WORDS: &[&str] = &["exec", "dlx", "x", "run", "tool", "--"];

/// Options of the launchers that take the next argument as their value.
const VALUE_OPTIONS: &[&str] = &[
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "-C",
    "--conditions",
    "--env-file",
    "-p",
    "--package",
    "--from",
    "--with",
    "--python",
    "--cwd",
    "--preload",
    "--config",
    "-W",
    "-X",
    "-n",
    "-t",
    "-w",
];

/// Options after which the launcher runs code given inline: nothing it runs
/// can be named.
const CODE_OPTIONS: &[&str] = &["-e", "--eval", "-c", "--call", "--print"];

/// Last parts of a binary's name that only say its version or platform:
/// `codex-aarch64-apple-darwin`, `claude-2.0.14`.
const PLATFORM_WORDS: &[&str] = &[
    "darwin",
    "macos",
    "apple",
    "linux",
    "gnu",
    "musl",
    "aarch64",
    "arm64",
    "x86_64",
    "x64",
    "amd64",
    "universal",
    "unknown",
];

/// The command a path or a name runs, as agents are known by it: its last
/// part, lower case, without a script's extension, an `@version`, or the
/// version and platform words a release binary's name ends in.
pub fn command_name(word: &str) -> String {
    let base = word.rsplit('/').next().unwrap_or(word);
    let mut name = base.to_ascii_lowercase();
    for ext in [".js", ".mjs", ".cjs", ".ts", ".py", ".sh", ".exe"] {
        if let Some(stem) = name.strip_suffix(ext) {
            name.truncate(stem.len());
            break;
        }
    }
    if let Some(at) = name.find('@').filter(|&at| at > 0) {
        name.truncate(at);
    }
    while let Some((head, tail)) = name.rsplit_once('-') {
        if head.is_empty() || !(is_version(tail) || PLATFORM_WORDS.contains(&tail)) {
            break;
        }
        name.truncate(head.len());
    }
    name
}

/// `2.0.14`, `v1`, `2025.09.18`: a version, not a name.
fn is_version(word: &str) -> bool {
    let digits = word.strip_prefix('v').unwrap_or(word);
    digits.starts_with(|c: char| c.is_ascii_digit())
        && digits
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
}

/// The agent whose command `word` runs, given the arguments it runs with.
/// Goose and GitHub Copilot share their command with other tools (the `goose`
/// database migration tool, AWS Copilot), which always take a subcommand of
/// their own: the agent is told apart by what it is started with.
pub(super) fn by_command(word: &str, args: &[String]) -> Option<AgentKind> {
    let name = command_name(word);
    let agent = KNOWN
        .iter()
        .find(|k| k.commands.contains(&name.as_str()))?
        .agent;
    let first = args.first().map(String::as_str);
    let ok = match agent {
        AgentKind::Goose => first.is_none_or(|a| {
            a.starts_with('-')
                || [
                    "session",
                    "s",
                    "run",
                    "web",
                    "configure",
                    "recipe",
                    "project",
                    "projects",
                    "schedule",
                    "mcp",
                    "acp",
                ]
                .contains(&a)
        }),
        AgentKind::Copilot => first.is_none_or(|a| a.starts_with('-')),
        _ => true,
    };
    ok.then_some(agent)
}

/// The agent an npm or PyPI package spec names: `@openai/codex`,
/// `@anthropic-ai/claude-code@latest`, `npm:@google/gemini-cli`, `aider-chat`.
pub(super) fn by_package(spec: &str) -> Option<AgentKind> {
    let spec = spec.strip_prefix("npm:").unwrap_or(spec);
    let spec = match spec[1.min(spec.len())..].find('@') {
        Some(at) => &spec[..at + 1],
        None => spec,
    };
    let spec = spec.to_ascii_lowercase();
    KNOWN
        .iter()
        .find(|k| k.packages.contains(&spec.as_str()))
        .map(|k| k.agent)
}

/// The agent a path is installed in: the package folder in
/// `node_modules/@openai/codex/…`, an agent's own `claude/versions/2.0.14` or
/// `cursor-agent/versions/…/node`, or a Homebrew `Cellar/gemini-cli/…`.
pub(super) fn by_install_path(path: &str) -> Option<AgentKind> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    for (i, part) in parts.iter().enumerate() {
        let next = parts.get(i + 1).copied();
        let found = match (*part, next) {
            ("node_modules", Some(scope)) if scope.starts_with('@') => parts
                .get(i + 2)
                .and_then(|name| by_package(&format!("{scope}/{name}"))),
            ("node_modules", Some(name)) => by_package(name),
            ("Cellar" | "Caskroom", Some(formula)) => by_command(formula, &[]),
            (name, Some("versions")) => by_command(name, &[]),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

pub(super) fn is_launcher(word: &str) -> bool {
    let name = command_name(word);
    LAUNCHERS.contains(&name.as_str()) || name.starts_with("python") || name.starts_with("pypy")
}

/// What a launcher given `args` runs, and the arguments it runs it with:
/// past its options (and their values), `NAME=value` settings and a
/// runner's subcommand words, through any launcher it runs in turn (`npx`
/// is `node` running npx). Python's `-m aider` names it as a module. `None`
/// when the launcher runs code given inline. `launcher` is the program given
/// `args`: `env -u NAME` takes a value where Python's `-u` is a flag.
pub(super) fn launched<'a>(launcher: &str, args: &'a [String]) -> Option<(&'a str, &'a [String])> {
    let mut launcher = command_name(launcher);
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if CODE_OPTIONS.contains(&arg) {
            return None;
        }
        if arg == "-m" {
            return args.get(i).map(|m| (m.as_str(), &args[i + 1..]));
        }
        let env_value = launcher == "env" && matches!(arg, "-u" | "--unset" | "-C" | "--chdir");
        if env_value || VALUE_OPTIONS.contains(&arg) {
            i += 1;
            continue;
        }
        if arg.starts_with('-') || RUNNER_WORDS.contains(&arg) {
            continue;
        }
        if arg.contains('=') {
            continue;
        }
        if is_launcher(arg) && by_command(arg, &[]).is_none() {
            launcher = command_name(arg);
            continue;
        }
        return Some((arg, &args[i..]));
    }
    None
}
