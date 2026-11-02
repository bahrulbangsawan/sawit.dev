//! The AI clients Sawit installs into, and finding them (and their profiles) on this Mac.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    // Agents and CLIs
    ClaudeCode,
    Codex,
    Gemini,
    Qwen,
    OpenCode,
    Amp,
    CopilotCli,
    FactoryDroid,
    Kiro,
    Cline,
    Auggie,
    AmazonQ,
    Junie,
    Warp,
    Goose,
    Crush,
    // Editors and IDEs
    ClaudeDesktop,
    Cursor,
    Windsurf,
    VsCode,
    VsCodeInsiders,
    Zed,
    Antigravity,
    Trae,
    RooCode,
    KiloCode,
    Continue,
    LmStudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Coding agents and CLIs.
    Agent,
    /// Editors, IDEs and desktop apps.
    Editor,
}

impl Client {
    pub const ALL: [Client; 28] = [
        Client::ClaudeCode,
        Client::Codex,
        Client::Gemini,
        Client::Qwen,
        Client::OpenCode,
        Client::Amp,
        Client::CopilotCli,
        Client::FactoryDroid,
        Client::Kiro,
        Client::Cline,
        Client::Auggie,
        Client::AmazonQ,
        Client::Junie,
        Client::Warp,
        Client::Goose,
        Client::Crush,
        Client::ClaudeDesktop,
        Client::Cursor,
        Client::Windsurf,
        Client::VsCode,
        Client::VsCodeInsiders,
        Client::Zed,
        Client::Antigravity,
        Client::Trae,
        Client::RooCode,
        Client::KiloCode,
        Client::Continue,
        Client::LmStudio,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::Codex => "Codex",
            Client::Gemini => "Gemini CLI",
            Client::Qwen => "Qwen Code",
            Client::OpenCode => "opencode",
            Client::Amp => "Amp",
            Client::CopilotCli => "GitHub Copilot CLI",
            Client::FactoryDroid => "Factory Droid",
            Client::Kiro => "Kiro",
            Client::Cline => "Cline",
            Client::Auggie => "Augment (Auggie)",
            Client::AmazonQ => "Amazon Q CLI",
            Client::Junie => "JetBrains Junie",
            Client::Warp => "Warp",
            Client::Goose => "Goose",
            Client::Crush => "Crush",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Cursor => "Cursor",
            Client::Windsurf => "Windsurf / Devin",
            Client::VsCode => "VS Code",
            Client::VsCodeInsiders => "VS Code Insiders",
            Client::Zed => "Zed",
            Client::Antigravity => "Antigravity",
            Client::Trae => "Trae",
            Client::RooCode => "Roo Code",
            Client::KiloCode => "Kilo Code",
            Client::Continue => "Continue",
            Client::LmStudio => "LM Studio",
        }
    }

    pub fn category(self) -> Category {
        match self {
            Client::ClaudeDesktop
            | Client::Cursor
            | Client::Windsurf
            | Client::VsCode
            | Client::VsCodeInsiders
            | Client::Zed
            | Client::Antigravity
            | Client::Trae
            | Client::RooCode
            | Client::KiloCode
            | Client::Continue
            | Client::LmStudio => Category::Editor,
            _ => Category::Agent,
        }
    }

    /// Whether it's an app that starts with launchd's `PATH`
    /// (`/usr/bin:/bin:/usr/sbin:/sbin`) rather than the login shell's:
    /// Claude Desktop and LM Studio. VS Code, its forks and extensions, and
    /// Zed read the login shell's environment; agents run from a terminal
    /// have its.
    pub fn without_shell_path(self) -> bool {
        matches!(self, Client::ClaudeDesktop | Client::LmStudio)
    }

    /// Whether Sawit can edit the config (JSON/TOML). YAML and script
    /// configs get a snippet to paste instead.
    pub fn editable(self) -> bool {
        !matches!(self, Client::Goose | Client::Continue | Client::Crush)
    }

    /// The environment variable that selects a profile folder, for clients
    /// that have profiles.
    pub fn profile_var(self) -> Option<&'static str> {
        match self {
            Client::ClaudeCode => Some("CLAUDE_CONFIG_DIR"),
            Client::Codex => Some("CODEX_HOME"),
            _ => None,
        }
    }

    /// The client's own CLI, used instead of editing files when installed.
    pub fn cli(self) -> Option<&'static str> {
        match self {
            Client::ClaudeCode => Some("claude"),
            Client::Codex => Some("codex"),
            _ => None,
        }
    }
}

/// One place a server can be installed: a client, and for Claude Code and
/// Codex one of its profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub client: Client,
    /// "default", or the profile folder's suffix (`.claude-work` → "work").
    pub profile: String,
    pub config: PathBuf,
    /// The folder `profile_var` must point at; `None` for the default
    /// profile, which must run with the variable unset.
    pub profile_dir: Option<PathBuf>,
}

fn target(client: Client, profile: &str, config: PathBuf, profile_dir: Option<PathBuf>) -> Target {
    Target {
        client,
        profile: profile.into(),
        config,
        profile_dir,
    }
}

/// Folders in `home` named `<prefix>-<profile>` that contain `marker`.
pub(crate) fn profiles(home: &Path, prefix: &str, marker: &str) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let profile = name.strip_prefix(prefix)?.strip_prefix('-')?.to_string();
            let dir = e.path();
            (dir.is_dir() && dir.join(marker).exists()).then_some((profile, dir))
        })
        .collect();
    found.sort();
    found
}

/// Every installed client (and profile) on this Mac: [`discover`] in the
/// home folder, with this process's `CLAUDE_CONFIG_DIR`.
pub fn targets() -> Vec<Target> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    discover(&home, claude_dir)
}

/// Every installed client (and profile) under `home`. `claude_config_dir`
/// is the app's `CLAUDE_CONFIG_DIR`, included when it lives elsewhere.
pub fn discover(home: &Path, claude_config_dir: Option<PathBuf>) -> Vec<Target> {
    let mut out = Vec::new();
    if home.join(".claude.json").exists() || home.join(".claude").is_dir() {
        out.push(target(
            Client::ClaudeCode,
            "default",
            home.join(".claude.json"),
            None,
        ));
    }
    for (profile, dir) in profiles(home, ".claude", ".claude.json") {
        out.push(target(
            Client::ClaudeCode,
            &profile,
            dir.join(".claude.json"),
            Some(dir),
        ));
    }
    if let Some(dir) = claude_config_dir
        && dir.join(".claude.json").exists()
    {
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let known = out.iter().any(|t| {
            t.profile_dir
                .as_deref()
                .is_some_and(|d| canonical(d) == canonical(&dir))
        });
        if !known {
            let profile = dir
                .file_name()
                .map(|n| n.to_string_lossy().trim_start_matches('.').to_string())
                .unwrap_or_else(|| "CLAUDE_CONFIG_DIR".into());
            out.push(target(
                Client::ClaudeCode,
                &profile,
                dir.join(".claude.json"),
                Some(dir),
            ));
        }
    }
    let desktop = home.join("Library/Application Support/Claude");
    if desktop.is_dir() {
        out.push(target(
            Client::ClaudeDesktop,
            "default",
            desktop.join("claude_desktop_config.json"),
            None,
        ));
    }
    if home.join(".codex").is_dir() {
        out.push(target(
            Client::Codex,
            "default",
            home.join(".codex/config.toml"),
            None,
        ));
    }
    for (profile, dir) in profiles(home, ".codex", "config.toml") {
        out.push(target(
            Client::Codex,
            &profile,
            dir.join("config.toml"),
            Some(dir),
        ));
    }
    // (client, folder that shows it's installed, config file in it). Paths
    // checked against each tool's documentation.
    let documented: [(Client, &str, &str); 19] = [
        (Client::Gemini, ".gemini", "settings.json"),
        (Client::Qwen, ".qwen", "settings.json"),
        (Client::OpenCode, ".config/opencode", "opencode.json"),
        (Client::Amp, ".config/amp", "settings.json"),
        (Client::CopilotCli, ".copilot", "mcp-config.json"),
        (Client::FactoryDroid, ".factory", "mcp.json"),
        (Client::Kiro, ".kiro", "settings/mcp.json"),
        (
            Client::Cline,
            ".cline",
            "data/settings/cline_mcp_settings.json",
        ),
        (Client::Auggie, ".augment", "settings.json"),
        (Client::Junie, ".junie", "mcp/mcp.json"),
        (Client::Warp, ".warp", ".mcp.json"),
        (Client::Goose, ".config/goose", "config.yaml"),
        (Client::Crush, ".config/crush", "crushrc"),
        (Client::Cursor, ".cursor", "mcp.json"),
        (Client::Windsurf, ".config/devin", "mcp_config.json"),
        (Client::Zed, ".config/zed", "settings.json"),
        (Client::Antigravity, ".gemini/config", "mcp_config.json"),
        (Client::KiloCode, ".config/kilo", "kilo.json"),
        (Client::Continue, ".continue", "config.yaml"),
    ];
    let documented_mac: [(Client, &str, &str); 2] = [
        (
            Client::VsCode,
            "Library/Application Support/Code/User",
            "mcp.json",
        ),
        (
            Client::VsCodeInsiders,
            "Library/Application Support/Code - Insiders/User",
            "mcp.json",
        ),
    ];
    for (client, dir, file) in documented.into_iter().chain(documented_mac) {
        let dir = home.join(dir);
        if dir.is_dir() {
            out.push(target(client, "default", dir.join(file), None));
        }
    }
    // Unconfirmed or older locations: offered only when the file itself is
    // there, so a wrong guess never creates a stray file.
    let existing: [(Client, &str); 7] = [
        (Client::LmStudio, ".lmstudio/mcp.json"),
        (Client::AmazonQ, ".aws/amazonq/mcp.json"),
        (
            Client::Trae,
            "Library/Application Support/Trae/User/mcp.json",
        ),
        (
            Client::RooCode,
            "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json",
        ),
        (
            Client::Cline,
            "Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json",
        ),
        (Client::Windsurf, ".codeium/windsurf/mcp_config.json"),
        (Client::Antigravity, ".gemini/antigravity/mcp_config.json"),
    ];
    for (client, file) in existing {
        let file = home.join(file);
        if file.is_file() && !out.iter().any(|t| t.config == file) {
            let profile = if out.iter().any(|t| t.client == client) {
                "legacy"
            } else {
                "default"
            };
            out.push(target(client, profile, file, None));
        }
    }
    // LM Studio's folder is documented even if mcp.json isn't there yet.
    if home.join(".lmstudio").is_dir() && !out.iter().any(|t| t.client == Client::LmStudio) {
        out.push(target(
            Client::LmStudio,
            "default",
            home.join(".lmstudio/mcp.json"),
            None,
        ));
    }
    out
}
