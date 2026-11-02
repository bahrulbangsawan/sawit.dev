//! The config files each agent reads, in its order, highest priority first: where each one is
//! and what it holds.

use super::{Base, Entry, Layer};
use crate::mcp_clients::Client;

pub(super) const CLAUDE_CODE: &[Entry] = &[
    (
        Layer::Managed,
        Base::Root,
        "/Library/Application Support/ClaudeCode/managed-settings.json",
        "Settings from your organization",
    ),
    (
        Layer::Managed,
        Base::Root,
        "/Library/Application Support/ClaudeCode/managed-mcp.json",
        "MCP servers from your organization",
    ),
    (
        Layer::Managed,
        Base::Root,
        "/Library/Application Support/ClaudeCode/CLAUDE.md",
        "Instructions from your organization",
    ),
    (
        Layer::ProjectLocal,
        Base::Project,
        ".claude/settings.local.json",
        "Your settings for this project",
    ),
    (
        Layer::ProjectLocal,
        Base::Project,
        "CLAUDE.local.md",
        "Your instructions for this project",
    ),
    (
        Layer::Project,
        Base::Project,
        ".claude/settings.json",
        "Settings shared with the team",
    ),
    (
        Layer::Project,
        Base::Project,
        ".mcp.json",
        "MCP servers shared with the team",
    ),
    (
        Layer::Project,
        Base::Project,
        "CLAUDE.md",
        "Instructions shared with the team",
    ),
    (
        Layer::Project,
        Base::Project,
        ".claude/CLAUDE.md",
        "Instructions shared with the team",
    ),
    (
        Layer::User,
        Base::Dir,
        "settings.json",
        "Settings for every project",
    ),
    (
        Layer::User,
        Base::Dir,
        "CLAUDE.md",
        "Instructions for every project",
    ),
    (
        Layer::User,
        Base::Dir,
        "keybindings.json",
        "Keyboard shortcuts",
    ),
    (
        Layer::Global,
        Base::Dir,
        ".claude.json",
        "Account, MCP servers and /config state",
    ),
];

pub(super) const CODEX: &[Entry] = &[
    (
        Layer::Managed,
        Base::Root,
        "/etc/codex/managed_config.toml",
        "Settings from your organization",
    ),
    (
        Layer::Managed,
        Base::Root,
        "/etc/codex/requirements.toml",
        "Limits from your organization",
    ),
    (
        Layer::Project,
        Base::Project,
        ".codex/config.toml",
        "Settings for this project, once trusted",
    ),
    (
        Layer::Project,
        Base::Project,
        "AGENTS.override.md",
        "Instructions, used instead of AGENTS.md",
    ),
    (
        Layer::Project,
        Base::Project,
        "AGENTS.md",
        "Instructions shared with the team",
    ),
    (
        Layer::User,
        Base::Dir,
        "config.toml",
        "Settings, profiles and MCP servers",
    ),
    (
        Layer::User,
        Base::Dir,
        "AGENTS.override.md",
        "Instructions, used instead of AGENTS.md",
    ),
    (
        Layer::User,
        Base::Dir,
        "AGENTS.md",
        "Instructions for every project",
    ),
    (Layer::User, Base::Dir, "hooks.json", "Hooks"),
    (
        Layer::System,
        Base::Root,
        "/etc/codex/config.toml",
        "Settings for every user",
    ),
];

/// Agents without profiles: `(client, entries)`.
pub(super) const OTHERS: &[(Client, &[Entry])] = &[
    (
        Client::Gemini,
        &[
            (
                Layer::Managed,
                Base::Root,
                "/Library/Application Support/GeminiCli/settings.json",
                "System settings; override everything",
            ),
            (
                Layer::Project,
                Base::Project,
                ".gemini/settings.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "GEMINI.md",
                "Instructions shared with the team",
            ),
            (
                Layer::Project,
                Base::Project,
                ".gemini/.env",
                "Environment for this project",
            ),
            (
                Layer::User,
                Base::Home,
                ".gemini/settings.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".gemini/GEMINI.md",
                "Instructions for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".gemini/.env",
                "Environment for every project",
            ),
            (
                Layer::System,
                Base::Root,
                "/Library/Application Support/GeminiCli/system-defaults.json",
                "Defaults for every user",
            ),
        ],
    ),
    (
        Client::Qwen,
        &[
            (
                Layer::Project,
                Base::Project,
                ".qwen/settings.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "QWEN.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".qwen/settings.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".qwen/QWEN.md",
                "Instructions for every project",
            ),
        ],
    ),
    (
        Client::OpenCode,
        &[
            (
                Layer::Project,
                Base::Project,
                "opencode.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "opencode.jsonc",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "AGENTS.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/opencode/opencode.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/opencode/opencode.jsonc",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/opencode/AGENTS.md",
                "Instructions for every project",
            ),
        ],
    ),
    (
        Client::Amp,
        &[
            (
                Layer::Project,
                Base::Project,
                "AGENTS.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/amp/settings.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/amp/AGENTS.md",
                "Instructions for every project",
            ),
        ],
    ),
    (
        Client::CopilotCli,
        &[
            (
                Layer::Project,
                Base::Project,
                ".github/copilot-instructions.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".copilot/config.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".copilot/mcp-config.json",
                "MCP servers",
            ),
        ],
    ),
    (
        Client::Cursor,
        &[
            (
                Layer::Project,
                Base::Project,
                ".cursor/mcp.json",
                "MCP servers for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                ".cursor/cli.json",
                "CLI permissions for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                ".cursorrules",
                "Rules shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".cursor/mcp.json",
                "MCP servers for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".cursor/cli-config.json",
                "CLI settings",
            ),
        ],
    ),
    (
        Client::Windsurf,
        &[
            (
                Layer::Project,
                Base::Project,
                ".windsurfrules",
                "Rules shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".codeium/windsurf/mcp_config.json",
                "MCP servers",
            ),
            (
                Layer::User,
                Base::Home,
                ".codeium/windsurf/memories/global_rules.md",
                "Rules for every project",
            ),
        ],
    ),
    (
        Client::Kiro,
        &[
            (
                Layer::Project,
                Base::Project,
                ".kiro/settings/mcp.json",
                "MCP servers for this project",
            ),
            (
                Layer::User,
                Base::Home,
                ".kiro/settings/mcp.json",
                "MCP servers for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".kiro/settings/cli.json",
                "CLI settings",
            ),
        ],
    ),
    (
        Client::Goose,
        &[
            (
                Layer::Project,
                Base::Project,
                ".goosehints",
                "Hints shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/goose/config.yaml",
                "Settings and extensions",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/goose/.goosehints",
                "Hints for every project",
            ),
        ],
    ),
    (
        Client::Crush,
        &[
            (
                Layer::Project,
                Base::Project,
                ".crush.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "crush.json",
                "Settings for this project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/crush/crush.json",
                "Settings for every project",
            ),
        ],
    ),
    (
        Client::ClaudeDesktop,
        &[(
            Layer::User,
            Base::Home,
            "Library/Application Support/Claude/claude_desktop_config.json",
            "MCP servers and settings",
        )],
    ),
];
