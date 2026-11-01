//! Which coding agent (Claude Code, Codex, …) runs in a terminal, told from its
//! foreground process ([`detect`]). Many agents are Node, Bun or Python
//! scripts, so the process's own name says `node`: what the script is comes
//! from its arguments, its executable's path or the package it was run from. A
//! terminal title the agent sets ("✳ Claude Code") is the fallback, for an
//! agent that runs on a server over SSH or under a launcher this doesn't know.

/// A coding agent a terminal can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    GeminiCli,
    Opencode,
    Amp,
    CursorAgent,
    Copilot,
    Goose,
    Qwen,
    Kiro,
    Junie,
    Aider,
    Crush,
    Droid,
    Warp,
    Cline,
    KiloCode,
    OhMyPi,
}

/// How an agent is known: by the command that runs it, the package it is
/// installed from, and the titles it sets.
struct Known {
    agent: AgentKind,
    id: &'static str,
    name: &'static str,
    /// Its command names, as `command_name` leaves them.
    commands: &'static [&'static str],
    /// npm or PyPI packages it is run or installed from.
    packages: &'static [&'static str],
    /// How a title it sets starts, after any spinner or mark. Each is matched
    /// with its case, so a shell's lower-case title of a command another tool
    /// shares a name with (`goose up`, of the migration tool) doesn't count.
    titles: &'static [&'static str],
}

const KNOWN: &[Known] = &[
    Known {
        agent: AgentKind::ClaudeCode,
        id: "claude-code",
        name: "Claude Code",
        commands: &["claude", "claude-code"],
        packages: &["@anthropic-ai/claude-code"],
        titles: &["Claude Code", "claude"],
    },
    Known {
        agent: AgentKind::Codex,
        id: "codex",
        name: "Codex",
        commands: &["codex"],
        packages: &["@openai/codex"],
        titles: &["Codex", "codex"],
    },
    Known {
        agent: AgentKind::GeminiCli,
        id: "gemini",
        name: "Gemini CLI",
        commands: &["gemini", "gemini-cli"],
        packages: &["@google/gemini-cli"],
        titles: &["Gemini CLI", "gemini"],
    },
    Known {
        agent: AgentKind::Opencode,
        id: "opencode",
        name: "OpenCode",
        commands: &["opencode"],
        packages: &["opencode-ai"],
        titles: &["OpenCode", "opencode"],
    },
    Known {
        agent: AgentKind::Amp,
        id: "amp",
        name: "Amp",
        commands: &["amp"],
        packages: &["@sourcegraph/amp"],
        titles: &["Amp"],
    },
    Known {
        agent: AgentKind::CursorAgent,
        id: "cursor",
        name: "Cursor Agent",
        commands: &["cursor-agent"],
        packages: &[],
        titles: &["Cursor Agent", "cursor-agent"],
    },
    Known {
        agent: AgentKind::Copilot,
        id: "copilot",
        name: "GitHub Copilot",
        commands: &["copilot"],
        packages: &["@github/copilot"],
        titles: &["GitHub Copilot"],
    },
    Known {
        agent: AgentKind::Goose,
        id: "goose",
        name: "Goose",
        commands: &["goose"],
        packages: &[],
        titles: &["Goose"],
    },
    Known {
        agent: AgentKind::Qwen,
        id: "qwen",
        name: "Qwen Code",
        commands: &["qwen", "qwen-code"],
        packages: &["@qwen-code/qwen-code"],
        titles: &["Qwen Code", "qwen"],
    },
    Known {
        agent: AgentKind::Kiro,
        id: "kiro",
        name: "Kiro",
        commands: &["kiro-cli", "kiro-cli-chat"],
        packages: &[],
        titles: &["Kiro", "kiro-cli"],
    },
    Known {
        agent: AgentKind::Junie,
        id: "junie",
        name: "Junie",
        commands: &["junie"],
        packages: &["@jetbrains/junie"],
        titles: &["Junie", "junie"],
    },
    Known {
        agent: AgentKind::Aider,
        id: "aider",
        name: "Aider",
        commands: &["aider"],
        packages: &["aider-chat", "aider-install"],
        titles: &["Aider", "aider"],
    },
    Known {
        agent: AgentKind::Crush,
        id: "crush",
        name: "Crush",
        commands: &["crush"],
        packages: &["@charmland/crush"],
        titles: &["Crush", "crush"],
    },
    Known {
        agent: AgentKind::Droid,
        id: "droid",
        name: "Factory Droid",
        commands: &["droid"],
        packages: &["droid", "@factory/cli"],
        titles: &["Factory Droid", "Droid", "droid"],
    },
    Known {
        agent: AgentKind::Warp,
        id: "warp",
        name: "Warp AgentKind",
        commands: &["warp", "warp-preview", "oz", "oz-preview"],
        packages: &[],
        titles: &["Warp AgentKind"],
    },
    Known {
        agent: AgentKind::Cline,
        id: "cline",
        name: "Cline",
        commands: &["cline"],
        packages: &["cline"],
        titles: &["Cline", "cline"],
    },
    Known {
        agent: AgentKind::KiloCode,
        id: "kilo-code",
        name: "Kilo Code",
        commands: &["kilocode"],
        packages: &["@kilocode/cli"],
        titles: &["Kilo Code", "kilocode"],
    },
    Known {
        agent: AgentKind::OhMyPi,
        id: "omp",
        name: "oh-my-pi",
        commands: &["omp"],
        packages: &[],
        titles: &["oh-my-pi"],
    },
];

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

impl AgentKind {
    /// Every agent this knows, in a stable order.
    pub fn all() -> impl Iterator<Item = AgentKind> {
        KNOWN.iter().map(|k| k.agent)
    }

    fn known(&self) -> &'static Known {
        KNOWN
            .iter()
            .find(|k| k.agent == *self)
            .expect("every agent is in KNOWN")
    }

    /// A stable id for the agent (`claude-code`), as `app_state` reports it.
    /// Where the Dev Tools catalog has the agent, it is the catalog's id.
    pub fn id(&self) -> &'static str {
        self.known().id
    }

    /// The agent's name, as people know it.
    pub fn name(&self) -> &'static str {
        self.known().name
    }

    /// The command a user types to start the agent (`claude`).
    pub fn executable(&self) -> &'static str {
        self.known().commands[0]
    }

    /// The agent's logo, by its file name in `assets/agent_logos` without the
    /// `.svg`. A name ending in `-color` is drawn in its own colours; the others
    /// are one colour, drawn in the text's. `None` for an agent with no logo
    /// there, which a generic glyph stands for.
    pub fn logo(&self) -> Option<&'static str> {
        use AgentKind::*;
        Some(match self {
            ClaudeCode => "claudecode-color",
            Codex => "codex-color",
            GeminiCli => "geminicli-color",
            Opencode => "opencode",
            Amp => "amp-color",
            CursorAgent => "cursor",
            Copilot => "githubcopilot",
            Goose => "goose",
            Qwen => "qwen-color",
            Kiro => "kiro-color",
            Junie => "junie-color",
            Droid => "factory-color",
            Warp => "warp",
            Cline => "cline",
            KiloCode => "kilocode",
            OhMyPi => "omp-color",
            Aider | Crush => return None,
        })
    }

    /// The agent a terminal's foreground job runs ([`detect`]).
    pub fn detect(process: Option<&ProcessArgs>, title: Option<&str>) -> Option<AgentKind> {
        detect(process, title)
    }

    /// The agent run by a process whose executable is at `exec_path` and
    /// whose arguments are `argv` (`argv[0]` included). By the name it was run
    /// as or its executable's, then by where it is installed (an agent's own
    /// `versions` folder, its npm package), then, for an interpreter or a
    /// package runner, by what that runs.
    pub fn from_process(exec_path: &str, argv: &[String]) -> Option<AgentKind> {
        let args = argv.get(1..).unwrap_or_default();
        let program = argv.first().map(String::as_str).unwrap_or(exec_path);
        for word in [program, exec_path] {
            if let Some(agent) = by_command(word, args) {
                return Some(agent);
            }
        }
        if let Some(agent) = by_install_path(exec_path) {
            return Some(agent);
        }
        if !is_launcher(program) && !is_launcher(exec_path) {
            return None;
        }
        let launcher = if is_launcher(program) {
            program
        } else {
            exec_path
        };
        let (target, rest) = launched(launcher, args)?;
        by_command(target, rest)
            .or_else(|| by_install_path(target))
            .or_else(|| by_package(target))
    }

    /// The agent a terminal title names: one Claude Code sets (`✳ Claude
    /// Code`), or a shell's title for a command it runs (`codex`). Only a
    /// title that starts with the agent's name counts, after any spinner or
    /// mark in front.
    pub fn from_title(title: &str) -> Option<AgentKind> {
        let title = title.trim_start_matches(|c: char| !c.is_alphanumeric());
        KNOWN.iter().find_map(|k| {
            k.titles
                .iter()
                .any(|name| {
                    title.strip_prefix(name).is_some_and(|rest| {
                        rest.chars()
                            .next()
                            .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '-' | '_' | '.'))
                    })
                })
                .then_some(k.agent)
        })
    }
}

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
fn by_command(word: &str, args: &[String]) -> Option<AgentKind> {
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
fn by_package(spec: &str) -> Option<AgentKind> {
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
fn by_install_path(path: &str) -> Option<AgentKind> {
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

fn is_launcher(word: &str) -> bool {
    let name = command_name(word);
    LAUNCHERS.contains(&name.as_str()) || name.starts_with("python") || name.starts_with("pypy")
}

/// What a launcher given `args` runs, and the arguments it runs it with:
/// past its options (and their values), `NAME=value` settings and a
/// runner's subcommand words, through any launcher it runs in turn (`npx`
/// is `node` running npx). Python's `-m aider` names it as a module. `None`
/// when the launcher runs code given inline. `launcher` is the program given
/// `args`: `env -u NAME` takes a value where Python's `-u` is a flag.
fn launched<'a>(launcher: &str, args: &'a [String]) -> Option<(&'a str, &'a [String])> {
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

/// The agent a terminal's foreground job runs: told from its process
/// (`process`, read with [`process_args`]), else from `title`, a title the job
/// set. Pass a title only when it is the job's own: one left over from an
/// agent that has exited would name it still.
pub fn detect(process: Option<&ProcessArgs>, title: Option<&str>) -> Option<AgentKind> {
    process
        .and_then(|p| AgentKind::from_process(&p.exec_path, &p.argv))
        .or_else(|| title.and_then(AgentKind::from_title))
}

/// "Claude Code", "Claude Code and Codex", "Claude Code, Codex and Gemini
/// CLI": `agents`' names as a sentence says them.
pub fn names_phrase(agents: &[AgentKind]) -> String {
    let names: Vec<&str> = agents.iter().map(|a| a.name()).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A process's executable and its arguments (`argv[0]` included), as the OS
/// keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessArgs {
    pub exec_path: String,
    pub argv: Vec<String>,
}

/// Reads `KERN_PROCARGS2`'s layout: the argument count, the executable's
/// path, padding NULs, then the arguments, each NUL-ended (the environment
/// follows, and is left alone).
pub fn parse_procargs(buf: &[u8]) -> Option<ProcessArgs> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let rest = &buf[4..];
    let end = rest.iter().position(|&b| b == 0)?;
    let exec_path = String::from_utf8_lossy(&rest[..end]).into_owned();
    let mut at = end;
    while rest.get(at) == Some(&0) {
        at += 1;
    }
    let mut argv = Vec::new();
    for _ in 0..argc.max(0) {
        let tail = rest.get(at..)?;
        let len = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        argv.push(String::from_utf8_lossy(&tail[..len]).into_owned());
        at += len + 1;
    }
    Some(ProcessArgs { exec_path, argv })
}

/// The executable and arguments of process `pid`. Only the user's own
/// processes can be read; `None` for others, for one that has ended, and off
/// macOS.
pub fn process_args(pid: u32) -> Option<ProcessArgs> {
    #[cfg(target_os = "macos")]
    {
        let mut max: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `max` is a c_int, and `size` says so.
        let ok = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut max as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok != 0 || max <= 0 {
            return None;
        }
        let mut buf = vec![0u8; max as usize];
        let mut size = buf.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
        // SAFETY: `buf` holds `size` bytes, the most the kernel writes.
        let ok = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok != 0 {
            return None;
        }
        buf.truncate(size);
        parse_procargs(&buf)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// The name the OS gives process `pid` (its executable's, cut to 16
/// characters on macOS). `None` when it can't be read, as off macOS.
pub fn process_name(pid: u32) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let mut name = [0u8; 64];
        // SAFETY: `name` holds the length given.
        let len = unsafe {
            libc::proc_name(
                pid as libc::c_int,
                name.as_mut_ptr().cast(),
                name.len() as u32,
            )
        };
        if len > 0 {
            return Some(String::from_utf8_lossy(&name[..len as usize]).into_owned());
        }
    }
    let _ = pid;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn agent(exec: &str, words: &[&str]) -> Option<AgentKind> {
        AgentKind::from_process(exec, &argv(words))
    }

    #[test]
    fn a_native_binary_is_known_by_its_name() {
        assert_eq!(
            agent("/opt/homebrew/bin/codex", &["codex"]),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            agent("/usr/local/bin/goose", &["goose"]),
            Some(AgentKind::Goose)
        );
        assert_eq!(
            agent("/Users/me/.local/bin/cursor-agent", &["cursor-agent"]),
            Some(AgentKind::CursorAgent)
        );
        assert_eq!(
            agent("/Users/me/.local/bin/kiro-cli", &["kiro-cli", "chat"]),
            Some(AgentKind::Kiro)
        );
        assert_eq!(agent("/usr/bin/vim", &["vim", "notes.md"]), None);
    }

    /// The shell runs a command by the path it found, so argv[0] may be a full path.
    #[test]
    fn a_path_is_known_by_its_last_part() {
        assert_eq!(
            agent(
                "/Users/me/.local/share/claude/versions/2.0.14",
                &["/Users/me/.local/bin/claude", "--resume"],
            ),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            agent(
                "/opt/homebrew/bin/opencode",
                &["/opt/homebrew/bin/opencode"]
            ),
            Some(AgentKind::Opencode)
        );
    }

    /// Claude Code's native install runs a binary named after its version, from a `versions`
    /// folder of its own: run under another name, the folder still says what it is.
    #[test]
    fn a_version_named_binary_is_known_by_its_folder_or_suffix() {
        assert_eq!(
            agent("/Users/me/.local/share/claude/versions/2.0.14", &["2.0.14"]),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            agent(
                "/tmp/codex-aarch64-apple-darwin",
                &["codex-aarch64-apple-darwin"]
            ),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            agent("/tmp/claude-2.0.14", &["claude-2.0.14"]),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            agent("/tmp/gemini@0.9.0", &["gemini@0.9.0"]),
            Some(AgentKind::GeminiCli)
        );
        assert_eq!(command_name("/x/Codex-X86_64-Apple-Darwin"), "codex");
        assert_eq!(command_name("kiro-cli"), "kiro-cli");
        assert_eq!(command_name("cursor-agent"), "cursor-agent");
    }

    /// An npm-installed agent is a Node script: the process is `node`, and the script it runs
    /// (by the bin link the shell found, or inside the package) names the agent.
    #[test]
    fn a_node_script_is_known_by_the_script() {
        assert_eq!(
            agent(
                "/opt/homebrew/Cellar/node/24.1.0/bin/node",
                &["node", "/opt/homebrew/bin/claude"],
            ),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            agent(
                "/opt/homebrew/bin/node",
                &[
                    "node",
                    "--no-warnings",
                    "--enable-source-maps",
                    "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
                ],
            ),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            agent(
                "/opt/homebrew/bin/node",
                &[
                    "node",
                    "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                ],
            ),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            agent(
                "/Users/me/.bun/bin/bun",
                &[
                    "bun",
                    "/Users/me/.bun/install/global/node_modules/@google/gemini-cli/dist/index.js"
                ],
            ),
            Some(AgentKind::GeminiCli)
        );
        // A script's own arguments never name what runs.
        assert_eq!(
            agent("/opt/homebrew/bin/node", &["node", "server.js", "codex"]),
            None
        );
        assert_eq!(agent("/opt/homebrew/bin/node", &["node", "-e", "1"]), None);
        assert_eq!(agent("/opt/homebrew/bin/node", &["node"]), None);
    }

    /// Cursor's agent is a script that starts its own bundled `node` from a versions folder.
    #[test]
    fn a_bundled_runtime_is_known_by_its_folder() {
        assert_eq!(
            agent(
                "/Users/me/.local/share/cursor-agent/versions/2025.10.02-bd871ac/node",
                &[
                    "/Users/me/.local/share/cursor-agent/versions/2025.10.02-bd871ac/node",
                    "/Users/me/.local/share/cursor-agent/versions/2025.10.02-bd871ac/index.js",
                ],
            ),
            Some(AgentKind::CursorAgent)
        );
    }

    #[test]
    fn a_package_runner_is_known_by_the_package() {
        let node = "/opt/homebrew/bin/node";
        assert_eq!(
            agent(
                node,
                &[
                    "node",
                    "/opt/homebrew/bin/npx",
                    "-y",
                    "@anthropic-ai/claude-code@latest"
                ]
            ),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            agent(
                node,
                &["node", "/opt/homebrew/bin/pnpm", "dlx", "@openai/codex"]
            ),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            agent(
                "/Users/me/.local/bin/uvx",
                &["uvx", "--from", "aider-chat", "aider"]
            ),
            Some(AgentKind::Aider)
        );
        assert_eq!(
            agent(
                "/opt/homebrew/bin/deno",
                &["deno", "run", "-A", "npm:@google/gemini-cli"]
            ),
            Some(AgentKind::GeminiCli)
        );
        assert_eq!(
            agent("/usr/bin/caffeinate", &["caffeinate", "-i", "claude"]),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            agent("/usr/bin/env", &["env", "DEBUG=1", "codex", "--full-auto"]),
            Some(AgentKind::Codex)
        );
    }

    /// Aider is Python: the process is the interpreter, running its entry script or module.
    #[test]
    fn a_python_script_is_known_by_the_script_or_module() {
        assert_eq!(
            agent(
                "/opt/homebrew/Cellar/python@3.12/3.12.7/Frameworks/Python.framework/Versions/3.12/Resources/Python.app/Contents/MacOS/Python",
                &[
                    "/Users/me/.local/pipx/venvs/aider-chat/bin/python",
                    "/Users/me/.local/bin/aider"
                ],
            ),
            Some(AgentKind::Aider)
        );
        assert_eq!(
            agent(
                "/usr/bin/python3",
                &["python3", "-m", "aider", "--model", "x"]
            ),
            Some(AgentKind::Aider)
        );
        // Python's -u is a flag, env's takes the name it unsets.
        assert_eq!(
            agent(
                "/usr/bin/python3",
                &["python3", "-u", "/Users/me/.local/bin/aider"]
            ),
            Some(AgentKind::Aider)
        );
        assert_eq!(
            agent("/usr/bin/env", &["env", "-u", "TMUX", "claude"]),
            Some(AgentKind::ClaudeCode)
        );
    }

    /// The process decides; a title the job set only stands in when the process says nothing.
    #[test]
    fn detect_takes_the_process_before_the_title() {
        let ssh = ProcessArgs {
            exec_path: "/usr/bin/ssh".into(),
            argv: argv(&["ssh", "devbox"]),
        };
        let codex = ProcessArgs {
            exec_path: "/opt/homebrew/bin/codex".into(),
            argv: argv(&["codex"]),
        };
        assert_eq!(
            detect(Some(&ssh), Some("✳ Claude Code")),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(detect(Some(&ssh), Some("devbox")), None);
        assert_eq!(detect(Some(&ssh), None), None);
        assert_eq!(
            detect(Some(&codex), Some("✳ Claude Code")),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            AgentKind::detect(None, Some("✳ Claude Code")),
            Some(AgentKind::ClaudeCode)
        );
    }

    /// Every logo named is a file in assets/agent_logos, and each agent's command is one it is
    /// known by.
    #[test]
    fn logos_and_executables_are_real() {
        let logos = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/agent_logos");
        for kind in AgentKind::all() {
            if let Some(logo) = kind.logo() {
                let file = std::path::Path::new(logos).join(format!("{logo}.svg"));
                assert!(file.is_file(), "{} has no {}", kind.name(), file.display());
            }
            let exe = kind.executable();
            assert_eq!(by_command(exe, &[]), Some(kind), "{exe}");
        }
        assert_eq!(AgentKind::ClaudeCode.executable(), "claude");
        assert_eq!(AgentKind::GeminiCli.executable(), "gemini");
        assert_eq!(AgentKind::ClaudeCode.logo(), Some("claudecode-color"));
    }

    /// Commands another tool shares: the migration tool `goose` and AWS's `copilot` always take
    /// a subcommand of their own, the agents a flag or nothing.
    #[test]
    fn a_shared_command_name_needs_the_agents_own_start() {
        assert_eq!(
            agent("/opt/homebrew/bin/goose", &["goose", "session"]),
            Some(AgentKind::Goose)
        );
        assert_eq!(
            agent("/opt/homebrew/bin/goose", &["goose", "postgres", "x", "up"]),
            None
        );
        assert_eq!(agent("/opt/homebrew/bin/goose", &["goose", "up"]), None);
        assert_eq!(
            agent("/opt/homebrew/bin/copilot", &["copilot"]),
            Some(AgentKind::Copilot)
        );
        assert_eq!(
            agent(
                "/opt/homebrew/bin/copilot",
                &["copilot", "--allow-all-tools"]
            ),
            Some(AgentKind::Copilot)
        );
        assert_eq!(
            agent("/opt/homebrew/bin/copilot", &["copilot", "svc", "deploy"]),
            None
        );
    }

    #[test]
    fn every_requested_agent_is_known_by_its_command() {
        for (command, expected) in [
            ("claude", AgentKind::ClaudeCode),
            ("codex", AgentKind::Codex),
            ("gemini", AgentKind::GeminiCli),
            ("opencode", AgentKind::Opencode),
            ("amp", AgentKind::Amp),
            ("cursor-agent", AgentKind::CursorAgent),
            ("copilot", AgentKind::Copilot),
            ("goose", AgentKind::Goose),
            ("qwen", AgentKind::Qwen),
            ("kiro-cli", AgentKind::Kiro),
            ("junie", AgentKind::Junie),
            ("aider", AgentKind::Aider),
            ("crush", AgentKind::Crush),
            ("droid", AgentKind::Droid),
            ("warp", AgentKind::Warp),
            ("cline", AgentKind::Cline),
            ("kilocode", AgentKind::KiloCode),
            ("omp", AgentKind::OhMyPi),
        ] {
            assert_eq!(agent(&format!("/x/{command}"), &[command]), Some(expected));
        }
    }

    #[test]
    fn ids_and_names_are_unique() {
        let all: Vec<AgentKind> = AgentKind::all().collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.id(), b.id());
                assert_ne!(a.name(), b.name());
            }
        }
        assert_eq!(AgentKind::ClaudeCode.id(), "claude-code");
    }

    #[test]
    fn a_title_names_the_agent_only_at_its_start() {
        assert_eq!(
            AgentKind::from_title("✳ Claude Code"),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            AgentKind::from_title("⠂ Claude Code"),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            AgentKind::from_title("codex --full-auto"),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            AgentKind::from_title("Gemini CLI (sawit.dev)"),
            Some(AgentKind::GeminiCli)
        );
        assert_eq!(AgentKind::from_title("~/projects/codex"), None);
        assert_eq!(AgentKind::from_title("vim codex.rs"), None);
        assert_eq!(AgentKind::from_title("codex-rs"), None);
        assert_eq!(AgentKind::from_title("Ampersand"), None);
        assert_eq!(
            AgentKind::from_title("goose up"),
            None,
            "the migration tool"
        );
        assert_eq!(AgentKind::from_title("zsh"), None);
        assert_eq!(AgentKind::from_title(""), None);
    }

    #[test]
    fn names_read_as_a_sentence() {
        use AgentKind::*;
        assert_eq!(names_phrase(&[]), "");
        assert_eq!(names_phrase(&[ClaudeCode]), "Claude Code");
        assert_eq!(names_phrase(&[ClaudeCode, Codex]), "Claude Code and Codex");
        assert_eq!(
            names_phrase(&[ClaudeCode, Codex, GeminiCli]),
            "Claude Code, Codex and Gemini CLI"
        );
    }

    #[test]
    fn procargs_give_the_executable_and_arguments() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/opt/homebrew/bin/node\0\0\0\0");
        buf.extend_from_slice(b"node\0/opt/homebrew/bin/claude\0");
        buf.extend_from_slice(b"HOME=/Users/me\0");
        assert_eq!(
            parse_procargs(&buf),
            Some(ProcessArgs {
                exec_path: "/opt/homebrew/bin/node".into(),
                argv: argv(&["node", "/opt/homebrew/bin/claude"]),
            })
        );
        assert_eq!(parse_procargs(&[1, 0]), None);
    }

    /// This test's own process can be read, as a terminal's foreground job is.
    #[cfg(target_os = "macos")]
    #[test]
    fn reads_a_live_process() {
        let me = process_args(std::process::id()).expect("its own arguments");
        assert!(!me.argv.is_empty());
        assert!(me.exec_path.starts_with('/'), "{}", me.exec_path);
        assert!(process_name(std::process::id()).is_some());
    }
}
