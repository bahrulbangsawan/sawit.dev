//! Which coding agent (Claude Code, Codex, …) runs in a terminal, told from its
//! foreground process ([`detect`]). Many agents are Node, Bun or Python
//! scripts, so the process's own name says `node`: what the script is comes
//! from its arguments, its executable's path or the package it was run from. A
//! terminal title the agent sets ("✳ Claude Code") is the fallback, for an
//! agent that runs on a server over SSH or under a launcher this doesn't know.

mod command;
mod known;
mod process;

pub use command::command_name;
use command::{by_command, by_install_path, by_package, is_launcher, launched};
use known::{KNOWN, Known};
pub use process::{ProcessArgs, parse_procargs, process_args, process_name};

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

#[cfg(test)]
mod tests;
