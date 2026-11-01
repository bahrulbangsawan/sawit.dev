//! The agents this knows, each with the commands, packages and titles it is known by.

use super::AgentKind;

/// How an agent is known: by the command that runs it, the package it is
/// installed from, and the titles it sets.
pub(super) struct Known {
    pub(super) agent: AgentKind,
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    /// Its command names, as `command_name` leaves them.
    pub(super) commands: &'static [&'static str],
    /// npm or PyPI packages it is run or installed from.
    pub(super) packages: &'static [&'static str],
    /// How a title it sets starts, after any spinner or mark. Each is matched
    /// with its case, so a shell's lower-case title of a command another tool
    /// shares a name with (`goose up`, of the migration tool) doesn't count.
    pub(super) titles: &'static [&'static str],
}

pub(super) const KNOWN: &[Known] = &[
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
