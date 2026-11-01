//! Agent skills: the Skills page's catalog (skill packs from their official
//! sources), the skills folders of the agents on this Mac, which packs each
//! folder has, and the exact commands that install, update or remove a pack
//! there. A pack installs the way its source documents: the `skills` CLI
//! (`npx skills add owner/repo`, run with the package manager the user
//! picks), a Claude Code plugin from a marketplace (`claude plugin
//! marketplace add` and `claude plugin install`), or a copy of the skill's
//! folder from a git clone. Removing deletes the pack's skill folders from
//! the chosen agents' folders (`rm -rf` of `<folder>/<skill>`, a link or a
//! copy), or uninstalls its plugin.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::mcp_clients::{self, Client};
use crate::package_manager::{PackageManager, quote};

/// What a pack is for: the page's category menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Design,
    Workflow,
    Platform,
    Media,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Design,
        Category::Workflow,
        Category::Platform,
        Category::Media,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Design => "Design & UI",
            Category::Workflow => "Workflow",
            Category::Platform => "Platforms & SDKs",
            Category::Media => "Video & Animation",
        }
    }
}

/// How a pack installs, as its source documents it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The `skills` CLI (skills.sh): `npx skills add <source>`, run with the
    /// package manager picked, globally (`-g`) for the chosen agents (`-a`),
    /// without questions (`-y`). `all`: every skill of the source (`--skill
    /// '*'`), else the pack's skills by name.
    SkillsCli { source: &'static str, all: bool },
    /// A Claude Code plugin: the marketplace (a GitHub `owner/repo`, added once
    /// per Claude Code profile under the name its `marketplace.json` gives),
    /// then `<plugin>@<marketplace>`.
    Plugin {
        repo: &'static str,
        marketplace: &'static str,
        plugin: &'static str,
    },
    /// A shallow clone of `url`, then each skill's folder under `path` copied
    /// into the agents' skills folders.
    Copy {
        url: &'static str,
        path: &'static str,
    },
}

impl Method {
    /// Its name for tools and the CLI.
    pub fn id(self) -> &'static str {
        match self {
            Method::SkillsCli { .. } => "skills-cli",
            Method::Plugin { .. } => "plugin",
            Method::Copy { .. } => "copy",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Method::SkillsCli { .. } => "skills CLI",
            Method::Plugin { .. } => "Claude Code plugin",
            Method::Copy { .. } => "Git clone & copy",
        }
    }

    /// Whether it runs through a JavaScript package manager (the choice the
    /// page offers).
    pub fn uses_package_manager(self) -> bool {
        matches!(self, Method::SkillsCli { .. })
    }

    /// Whether it can install into `target`: a plugin only into a Claude Code
    /// profile, the `skills` CLI only into the folders it knows.
    pub fn reaches(self, target: &Target) -> bool {
        match self {
            Method::SkillsCli { .. } => target.cli_agent.is_some(),
            Method::Plugin { .. } => target.claude.is_some(),
            Method::Copy { .. } => true,
        }
    }

    /// `name@marketplace`, for a plugin.
    pub fn plugin_key(self) -> Option<String> {
        match self {
            Method::Plugin {
                marketplace,
                plugin,
                ..
            } => Some(format!("{plugin}@{marketplace}")),
            _ => None,
        }
    }
}

/// A pack of skills from one source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pack {
    pub id: &'static str,
    pub name: &'static str,
    /// Who publishes it.
    pub by: &'static str,
    pub about: &'static str,
    pub category: Category,
    /// The skill folders it installs (each has a `SKILL.md`).
    pub skills: &'static [&'static str],
    /// The page that documents it and how to install it.
    pub home: &'static str,
    /// Its source repository.
    pub repo: &'static str,
    /// How it installs, the preferred way first.
    pub methods: &'static [Method],
    /// A requirement or caveat.
    pub note: Option<&'static str>,
}

impl Pack {
    pub fn method(&self, id: &str) -> Option<Method> {
        self.methods.iter().copied().find(|m| m.id() == id)
    }

    /// Its plugin, when it is one.
    pub fn plugin(&self) -> Option<Method> {
        self.methods
            .iter()
            .copied()
            .find(|m| matches!(m, Method::Plugin { .. }))
    }
}

const SUPERPOWERS: &[&str] = &[
    "brainstorming",
    "diagnosing-superpowers",
    "dispatching-parallel-agents",
    "executing-plans",
    "finishing-a-development-branch",
    "receiving-code-review",
    "requesting-code-review",
    "subagent-driven-development",
    "systematic-debugging",
    "test-driven-development",
    "using-git-worktrees",
    "using-superpowers",
    "verification-before-completion",
    "writing-plans",
    "writing-skills",
];

const CLOUDFLARE: &[&str] = &[
    "agents-sdk",
    "basin",
    "cf-cli",
    "cloudflare",
    "cloudflare-email-service",
    "cloudflare-one",
    "cloudflare-one-migrations",
    "durable-objects",
    "gateway-workers",
    "k2",
    "nextjs-on-cloudflare",
    "sandbox-migrate-to-next",
    "sandbox-next",
    "sandbox-stable",
    "turnstile-spin",
    "web-perf",
    "workers-best-practices",
    "workers-profiling",
    "wrangler",
];

/// The catalog. Each pack's commands are the ones its source documents
/// (README, docs page); see `home` and `repo`.
pub const PACKS: &[Pack] = &[
    Pack {
        id: "interfaces",
        name: "Interface Skills",
        by: "Jakub Krehel",
        about: "Reviews and polish for product interfaces (UI, typography, layout, colour, writing, accessibility), every state of a component, variants to pick from, and technical teardown illustrations.",
        category: Category::Design,
        skills: &[
            "better-accessibility",
            "better-colors",
            "better-interface",
            "better-layout",
            "better-typography",
            "better-ui",
            "better-writing",
            "break",
            "build-design",
            "explain-interface",
            "interface-review",
            "state-machine",
            "teardown",
            "variant",
        ],
        home: "https://jakub.kr/skills",
        repo: "https://github.com/jakubkrehel/skills",
        methods: &[Method::SkillsCli {
            source: "jakubkrehel/skills",
            all: true,
        }],
        note: None,
    },
    Pack {
        id: "make-interfaces-feel-better",
        name: "Make Interfaces Feel Better",
        by: "Jakub Krehel",
        about: "The small details that make an interface feel better; the lighter skill that came before the better-* collection.",
        category: Category::Design,
        skills: &["make-interfaces-feel-better"],
        home: "https://jakub.kr/skills",
        repo: "https://github.com/jakubkrehel/make-interfaces-feel-better",
        methods: &[Method::SkillsCli {
            source: "jakubkrehel/make-interfaces-feel-better",
            all: true,
        }],
        note: None,
    },
    Pack {
        id: "unslop",
        name: "unslop",
        by: "Damir Sokolovsky",
        about: "Turns vibecoded, AI-looking UI into a designed interface: audits the slop, sets a direction from your product, then fixes the screen pass by pass.",
        category: Category::Design,
        skills: &["unslop"],
        home: "https://github.com/heydamir/unslop",
        repo: "https://github.com/heydamir/unslop",
        methods: &[Method::Copy {
            url: "https://github.com/heydamir/unslop",
            path: "skills",
        }],
        note: Some(
            "Live-site snapshots and story videos need Playwright with Chromium, and ffmpeg.",
        ),
    },
    Pack {
        id: "animate",
        name: "Animate",
        by: "cth9191",
        about: "Short procedural animations in any style as single-file canvas videos, built through story, look and storyboard check-ins.",
        category: Category::Media,
        skills: &["animate"],
        home: "https://github.com/cth9191/animate",
        repo: "https://github.com/cth9191/animate",
        methods: &[
            Method::Plugin {
                repo: "cth9191/animate",
                marketplace: "animate",
                plugin: "animate",
            },
            Method::Copy {
                url: "https://github.com/cth9191/animate",
                path: "plugins/animate/skills",
            },
        ],
        note: Some("A Claude Code skill. Needs Node 18+, Playwright with Chromium, and ffmpeg."),
    },
    Pack {
        id: "superpowers",
        name: "Superpowers",
        by: "Jesse Vincent",
        about: "A software development method as skills: brainstorming, plans, test-driven development, systematic debugging, subagents, code review and git worktrees.",
        category: Category::Workflow,
        skills: SUPERPOWERS,
        home: "https://github.com/obra/superpowers",
        repo: "https://github.com/obra/superpowers",
        methods: &[Method::Plugin {
            repo: "anthropics/claude-plugins-official",
            marketplace: "claude-plugins-official",
            plugin: "superpowers",
        }],
        note: None,
    },
    Pack {
        id: "improve",
        name: "improve",
        by: "shadcn",
        about: "Audits a codebase with your most capable model and writes self-contained plans for cheaper models to execute: findings ranked by leverage, plans with verification gates, and an executor it dispatches and reviews. It never edits your code itself.",
        category: Category::Workflow,
        skills: &["improve"],
        home: "https://github.com/shadcn/improve",
        repo: "https://github.com/shadcn/improve",
        methods: &[
            // The install its README documents.
            Method::SkillsCli {
                source: "shadcn/improve",
                all: true,
            },
            // Its .claude-plugin/marketplace.json, added for `/plugin marketplace add
            // shadcn/improve` and `/plugin install improve@improve` (shadcn/improve#5, #12).
            Method::Plugin {
                repo: "shadcn/improve",
                marketplace: "improve",
                plugin: "improve",
            },
        ],
        note: None,
    },
    Pack {
        id: "mcp-apps",
        name: "MCP Apps",
        by: "Model Context Protocol",
        about: "Build MCP Apps, interactive UI inside the chat: create one from scratch, add UI to a server's tools, convert a web app, or migrate an OpenAI App.",
        category: Category::Platform,
        skills: &[
            "add-app-to-server",
            "convert-web-app",
            "create-mcp-app",
            "migrate-oai-app",
        ],
        home: "https://modelcontextprotocol.io/extensions/apps/build",
        repo: "https://github.com/modelcontextprotocol/ext-apps",
        methods: &[
            Method::Plugin {
                repo: "modelcontextprotocol/ext-apps",
                // Its marketplace.json's name, which `plugin install` takes.
                marketplace: "mcp-apps",
                plugin: "mcp-apps",
            },
            Method::SkillsCli {
                source: "modelcontextprotocol/ext-apps",
                all: true,
            },
        ],
        note: None,
    },
    Pack {
        id: "cf-cli",
        name: "cf CLI",
        by: "Cloudflare",
        about: "Cloudflare's cf CLI for the public API and Workers projects: sign-in, finding commands with cf cli search, deploys, DNS, D1, KV and R2.",
        category: Category::Platform,
        skills: &["cf-cli"],
        home: "https://developers.cloudflare.com/cf/agents/",
        repo: "https://github.com/cloudflare/skills",
        methods: &[Method::SkillsCli {
            source: "cloudflare/skills",
            all: false,
        }],
        note: Some("Needs the cf CLI: install it from the Dev Tools page."),
    },
    Pack {
        id: "cloudflare",
        name: "Cloudflare Skills",
        by: "Cloudflare",
        about: "Every Cloudflare skill: Workers, Durable Objects, the Agents SDK, Sandbox, Wrangler and cf, Zero Trust, email, Turnstile and web performance.",
        category: Category::Platform,
        skills: CLOUDFLARE,
        home: "https://developers.cloudflare.com/agent-setup/claude-code/",
        repo: "https://github.com/cloudflare/skills",
        methods: &[
            Method::Plugin {
                repo: "cloudflare/skills",
                marketplace: "cloudflare",
                plugin: "cloudflare",
            },
            Method::SkillsCli {
                source: "cloudflare/skills",
                all: true,
            },
        ],
        note: Some("The plugin also adds Cloudflare's MCP server to Claude Code."),
    },
];

pub fn get(id: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.id == id)
}

/// A Claude Code profile: its config folder (`~/.claude`, or the profile's
/// `CLAUDE_CONFIG_DIR`), where its skills, plugins and marketplaces live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeHome {
    pub dir: PathBuf,
    /// The folder `CLAUDE_CONFIG_DIR` names; `None` for the default profile,
    /// whose commands run with the variable unset.
    pub profile_dir: Option<PathBuf>,
}

/// One skills folder and the agents that read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// "Claude Code", "Claude Code · work", "Codex, Zed".
    pub label: String,
    pub clients: Vec<Client>,
    /// The folder the skills go in, one folder per skill.
    pub dir: PathBuf,
    /// For a Claude Code profile, where its plugins live.
    pub claude: Option<ClaudeHome>,
    /// The `skills` CLI's name for an agent that installs here (`-a`).
    pub cli_agent: Option<&'static str>,
}

/// Each client's global skills folder (from the home folder) and the `skills`
/// CLI's name for it, as the CLI's table of agents and the agents' docs give
/// them. Clients without skills (Claude Desktop, LM Studio, Amazon Q) have
/// none.
fn skills_folder(client: Client) -> Option<(&'static str, &'static str)> {
    Some(match client {
        Client::Codex => (".agents/skills", "codex"),
        Client::Cline => (".agents/skills", "cline"),
        Client::Warp => (".agents/skills", "warp"),
        Client::Zed => (".agents/skills", "zed"),
        Client::Cursor => (".cursor/skills", "cursor"),
        Client::Gemini => (".gemini/skills", "gemini-cli"),
        Client::CopilotCli | Client::VsCode | Client::VsCodeInsiders => {
            (".copilot/skills", "github-copilot")
        }
        Client::OpenCode => (".config/opencode/skills", "opencode"),
        Client::Amp => (".config/agents/skills", "amp"),
        Client::FactoryDroid => (".factory/skills", "droid"),
        Client::Kiro => (".kiro/skills", "kiro-cli"),
        Client::Goose => (".config/goose/skills", "goose"),
        Client::Windsurf => (".codeium/windsurf/skills", "windsurf"),
        Client::Qwen => (".qwen/skills", "qwen-code"),
        Client::Antigravity => (".gemini/antigravity/skills", "antigravity"),
        Client::Junie => (".junie/skills", "junie"),
        Client::Crush => (".config/crush/skills", "crush"),
        Client::KiloCode => (".kilo/skills", "kilo"),
        Client::Continue => (".continue/skills", "continue"),
        Client::Auggie => (".augment/skills", "augment"),
        Client::Trae => (".trae/skills", "trae"),
        Client::RooCode => (".roo/skills", "roo"),
        Client::ClaudeCode | Client::ClaudeDesktop | Client::LmStudio | Client::AmazonQ => {
            return None;
        }
    })
}

/// The skills folders of the agents on this Mac ([`mcp_clients::targets`]).
pub fn targets() -> Vec<Target> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    targets_for(&home, mcp_clients::targets())
}

/// The skills folders of `clients` under `home`: each Claude Code profile's
/// own, then one per folder the other agents share (Codex, Cline, Warp and
/// Zed all read `~/.agents/skills`). `~/.agents/skills` and Codex's older
/// `~/.codex/skills` are there too when they exist.
pub fn targets_for(home: &Path, clients: Vec<mcp_clients::Target>) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    for t in &clients {
        if t.client != Client::ClaudeCode {
            continue;
        }
        let dir = t
            .profile_dir
            .clone()
            .unwrap_or_else(|| home.join(".claude"));
        let label = if t.profile_dir.is_none() {
            "Claude Code".to_string()
        } else {
            format!("Claude Code · {}", t.profile)
        };
        out.push(Target {
            label,
            clients: vec![Client::ClaudeCode],
            dir: dir.join("skills"),
            claude: Some(ClaudeHome {
                dir,
                profile_dir: t.profile_dir.clone(),
            }),
            cli_agent: Some("claude-code"),
        });
    }
    let mut add = |client: Option<Client>, folder: &str, agent: Option<&'static str>| {
        let dir = home.join(folder);
        match out.iter_mut().find(|t| t.dir == dir) {
            Some(t) => {
                if let Some(client) = client
                    && !t.clients.contains(&client)
                {
                    t.clients.push(client);
                }
                if t.cli_agent.is_none() {
                    t.cli_agent = agent;
                }
            }
            None => out.push(Target {
                label: String::new(),
                clients: client.into_iter().collect(),
                dir,
                claude: None,
                cli_agent: agent,
            }),
        }
    };
    for client in Client::ALL {
        // A client's default profile: the skills CLI has no other.
        let found = clients
            .iter()
            .any(|t| t.client == client && t.profile_dir.is_none());
        if found && let Some((folder, agent)) = skills_folder(client) {
            add(Some(client), folder, Some(agent));
        }
    }
    if home.join(".agents/skills").is_dir() {
        add(None, ".agents/skills", Some("codex"));
    }
    if home.join(".codex/skills").is_dir() {
        add(None, ".codex/skills", None);
    }
    for t in out.iter_mut().filter(|t| t.label.is_empty()) {
        let names: Vec<&str> = t.clients.iter().map(|c| c.name()).collect();
        t.label = if names.is_empty() {
            match t.dir.strip_prefix(home) {
                Ok(rest) if rest == Path::new(".codex/skills") => "Codex (older folder)".into(),
                _ => "Shared agents folder".into(),
            }
        } else {
            names.join(", ")
        };
    }
    out
}

/// What one target has: the skills in its folder, and for a Claude Code
/// profile the plugins installed for the user, their skills and the
/// marketplaces it knows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    /// Skill folders with a `SKILL.md` (links followed).
    pub folder: HashSet<String>,
    /// Skills the profile's plugins bring.
    pub from_plugins: HashSet<String>,
    /// `name@marketplace` of every plugin installed for the user.
    pub plugins: HashSet<String>,
    /// The marketplaces the profile has added.
    pub marketplaces: HashSet<String>,
}

impl Installed {
    pub fn has(&self, skill: &str) -> bool {
        self.folder.contains(skill) || self.from_plugins.contains(skill)
    }
}

/// The skill folders in `dir`: each folder (or link to one) with a `SKILL.md`.
fn skills_in(dir: &Path) -> HashSet<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("SKILL.md").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Reads what `target` has, from its folder and, for a Claude Code profile,
/// `plugins/installed_plugins.json` and `plugins/known_marketplaces.json`.
pub fn read_installed(target: &Target) -> Installed {
    let mut installed = Installed {
        folder: skills_in(&target.dir),
        ..Default::default()
    };
    let Some(claude) = &target.claude else {
        return installed;
    };
    let plugins = claude.dir.join("plugins");
    let json = |file: &str| -> Option<Value> {
        serde_json::from_str(&std::fs::read_to_string(plugins.join(file)).ok()?).ok()
    };
    if let Some(Value::Object(known)) = json("known_marketplaces.json") {
        installed.marketplaces = known.keys().cloned().collect();
    }
    if let Some(list) = json("installed_plugins.json")
        .as_ref()
        .and_then(|v| v.get("plugins"))
        .and_then(Value::as_object)
    {
        for (key, installs) in list {
            // A plugin installed for one project only isn't the user's.
            let user = installs
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i.get("scope").and_then(Value::as_str) == Some("user"));
            let mut any = false;
            for install in user {
                any = true;
                if let Some(path) = install.get("installPath").and_then(Value::as_str) {
                    installed
                        .from_plugins
                        .extend(skills_in(&Path::new(path).join("skills")));
                }
            }
            if any {
                installed.plugins.insert(key.clone());
            }
        }
    }
    installed
}

/// A pack's state in one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Every skill of the pack is there; `plugin` when its plugin brings them.
    Installed {
        plugin: bool,
    },
    /// Some of its skills are there.
    Partial {
        have: usize,
        of: usize,
    },
    Missing,
}

impl Status {
    /// Installed, or partly.
    pub fn any(self) -> bool {
        self != Status::Missing
    }
}

/// `pack`'s state in a target that has `installed`.
pub fn status(pack: &Pack, installed: &Installed) -> Status {
    if pack
        .plugin()
        .and_then(|m| m.plugin_key())
        .is_some_and(|key| installed.plugins.contains(&key))
    {
        return Status::Installed { plugin: true };
    }
    let have = pack.skills.iter().filter(|s| installed.has(s)).count();
    match have {
        0 => Status::Missing,
        n if n == pack.skills.len() => Status::Installed { plugin: false },
        n => Status::Partial {
            have: n,
            of: pack.skills.len(),
        },
    }
}

/// The Skills page's list, as `list_skills` and `sawit list skills` show it.
#[derive(Debug, Clone)]
pub struct Report {
    pub targets: Vec<Target>,
    /// What each target has, by its place in `targets`.
    pub installed: Vec<Installed>,
}

impl Report {
    pub fn status(&self, pack: &Pack, target: usize) -> Status {
        self.installed
            .get(target)
            .map_or(Status::Missing, |i| status(pack, i))
    }
}

/// The [`Report`] for this Mac.
pub fn report() -> Report {
    let targets = targets();
    let installed = targets.iter().map(read_installed).collect();
    Report { targets, installed }
}

/// The targets `wanted` names, by their place in `targets`: a target's label
/// ("Claude Code · work", "Codex, Zed"), one of its agents ("codex", "zed";
/// "Claude Code" is the default profile's), or its folder (`~/.cursor/skills`),
/// matched without case, spaces or punctuation ("claude-code-work").
pub fn find_targets(targets: &[Target], wanted: &[String]) -> Result<Vec<usize>, String> {
    let key = |s: &str| -> String {
        s.to_lowercase()
            .replace("profile", "")
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect()
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let mut out = Vec::new();
    for name in wanted {
        let path = match name.trim().strip_prefix("~/") {
            Some(rest) => Path::new(&home).join(rest),
            None => PathBuf::from(name.trim()),
        };
        let wanted_key = key(name);
        let found = targets
            .iter()
            .position(|t| key(&t.label) == wanted_key)
            .or_else(|| {
                targets.iter().position(|t| {
                    t.claude.as_ref().is_none_or(|c| c.profile_dir.is_none())
                        && t.clients.iter().any(|c| key(c.name()) == wanted_key)
                })
            })
            .or_else(|| {
                targets
                    .iter()
                    .position(|t| t.dir == path || t.dir == path.join("skills"))
            });
        match found {
            Some(ix) if !out.contains(&ix) => out.push(ix),
            Some(_) => {}
            None => {
                return Err(format!(
                    "No agent \"{name}\" on this Mac. The agents are: {}.",
                    targets
                        .iter()
                        .map(|t| t.label.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        }
    }
    Ok(out)
}

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
    fn path(&self, path: &Path) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/me")
    }

    fn env() -> Env {
        Env {
            home: home(),
            claude_config_dir_set: false,
        }
    }

    fn client(client: Client, profile_dir: Option<&str>) -> mcp_clients::Target {
        mcp_clients::Target {
            client,
            profile: profile_dir
                .map(|d| d.trim_start_matches("/Users/me/.claude-").to_string())
                .unwrap_or_else(|| "default".into()),
            config: PathBuf::from("/x"),
            profile_dir: profile_dir.map(PathBuf::from),
        }
    }

    fn claude(profile: Option<&str>) -> Target {
        let dir = profile.map_or(home().join(".claude"), PathBuf::from);
        Target {
            label: "Claude Code".into(),
            clients: vec![Client::ClaudeCode],
            dir: dir.join("skills"),
            claude: Some(ClaudeHome {
                dir,
                profile_dir: profile.map(PathBuf::from),
            }),
            cli_agent: Some("claude-code"),
        }
    }

    fn folder(dir: &str, agent: Option<&'static str>) -> Target {
        Target {
            label: dir.into(),
            clients: vec![],
            dir: home().join(dir),
            claude: None,
            cli_agent: agent,
        }
    }

    #[test]
    fn the_catalog_is_well_formed() {
        let mut ids: Vec<&str> = PACKS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PACKS.len());
        for pack in PACKS {
            assert!(
                !pack.skills.is_empty() && !pack.methods.is_empty(),
                "{}",
                pack.id
            );
            assert!(pack.home.starts_with("https://"), "{}", pack.id);
            assert!(pack.repo.starts_with("https://github.com/"), "{}", pack.id);
            // Skill names go into commands and paths.
            for skill in pack.skills {
                assert!(
                    skill
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                    "{skill}"
                );
            }
            assert_eq!(get(pack.id), Some(pack));
        }
    }

    /// The seven sources the user named are there, each installing the way its own docs say.
    #[test]
    fn the_named_sources_install_their_documented_way() {
        let methods = |id: &str| get(id).unwrap().methods.to_vec();
        assert_eq!(
            methods("interfaces"),
            [Method::SkillsCli {
                source: "jakubkrehel/skills",
                all: true
            }]
        );
        assert!(
            matches!(methods("unslop")[0], Method::Copy { url, path: "skills" } if url == "https://github.com/heydamir/unslop")
        );
        assert_eq!(
            get("animate")
                .unwrap()
                .plugin()
                .unwrap()
                .plugin_key()
                .as_deref(),
            Some("animate@animate")
        );
        assert_eq!(
            get("superpowers")
                .unwrap()
                .plugin()
                .unwrap()
                .plugin_key()
                .as_deref(),
            Some("superpowers@claude-plugins-official")
        );
        assert!(get("mcp-apps").unwrap().skills.contains(&"create-mcp-app"));
        assert_eq!(get("cf-cli").unwrap().skills, ["cf-cli"]);
        assert!(get("cloudflare").unwrap().skills.contains(&"cf-cli"));
        // shadcn/improve: its README's `npx skills add shadcn/improve` first, then the plugin its
        // marketplace.json names.
        let improve = get("improve").unwrap();
        assert_eq!(improve.skills, ["improve"]);
        assert_eq!(improve.category, Category::Workflow);
        assert_eq!(
            improve.methods[0],
            Method::SkillsCli {
                source: "shadcn/improve",
                all: true
            }
        );
        assert_eq!(
            improve.plugin().unwrap().plugin_key().as_deref(),
            Some("improve@improve")
        );
    }

    /// improve installs with the skills CLI into every agent picked, or as its plugin into a
    /// Claude Code profile; it counts as installed from its folder, a link to one, or its user
    /// plugin, and removing it takes out only its own folder and plugin.
    #[test]
    fn improve_installs_with_the_skills_cli_or_its_plugin() {
        let pack = get("improve").unwrap();
        let (cli, plugin) = (pack.methods[0], pack.methods[1]);
        let none = Installed::default();
        let (main, shared) = (claude(None), folder(".agents/skills", Some("codex")));
        assert_eq!(
            script(
                pack,
                cli,
                Action::Install,
                &[(&main, &none), (&shared, &none)],
                PackageManager::Npm,
                &env()
            )
            .unwrap(),
            ["npx -y skills add shadcn/improve --skill '*' -g -a claude-code -a codex -y"]
        );
        assert_eq!(
            sample(pack, cli, PackageManager::Pnpm),
            "pnpm dlx skills add shadcn/improve --skill '*' -g -a <agent> -y"
        );
        assert_eq!(
            script(
                pack,
                plugin,
                Action::Install,
                &[(&main, &none)],
                PackageManager::Npm,
                &env()
            )
            .unwrap(),
            [
                "claude plugin marketplace add shadcn/improve",
                "claude plugin install improve@improve",
            ]
        );
        let refused = script(
            pack,
            plugin,
            Action::Install,
            &[(&shared, &none)],
            PackageManager::Npm,
            &env(),
        )
        .unwrap_err();
        assert!(refused.contains("only into Claude Code"), "{refused}");

        // Installed: from the shared folder (the skills CLI's copy), through a link in Claude
        // Code's folder, and from the plugin's install path.
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        let skill_md = |root: &Path| {
            std::fs::create_dir_all(root.join("improve")).unwrap();
            std::fs::write(root.join("improve/SKILL.md"), "---\nname: improve\n---\n").unwrap();
        };
        skill_md(&h.join(".agents/skills"));
        let shared_dir = Target {
            dir: h.join(".agents/skills"),
            ..shared.clone()
        };
        assert_eq!(
            status(pack, &read_installed(&shared_dir)),
            Status::Installed { plugin: false }
        );
        let claude_dir = h.join(".claude");
        std::fs::create_dir_all(claude_dir.join("skills")).unwrap();
        std::os::unix::fs::symlink(
            h.join(".agents/skills/improve"),
            claude_dir.join("skills/improve"),
        )
        .unwrap();
        let claude_target = Target {
            dir: claude_dir.join("skills"),
            claude: Some(ClaudeHome {
                dir: claude_dir.clone(),
                profile_dir: None,
            }),
            ..claude(None)
        };
        assert_eq!(
            status(pack, &read_installed(&claude_target)),
            Status::Installed { plugin: false }
        );
        std::fs::remove_file(claude_dir.join("skills/improve")).unwrap();
        assert_eq!(
            status(pack, &read_installed(&claude_target)),
            Status::Missing
        );
        let cache = h.join(".claude/plugins/cache/improve/improve/1.0.0");
        skill_md(&cache.join("skills"));
        std::fs::write(
            claude_dir.join("plugins/installed_plugins.json"),
            serde_json::json!({"version": 2, "plugins": {
                "improve@improve": [{"scope": "user", "installPath": cache}]
            }})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            claude_dir.join("plugins/known_marketplaces.json"),
            r#"{"improve": {}}"#,
        )
        .unwrap();
        let installed = read_installed(&claude_target);
        assert!(installed.from_plugins.contains("improve"));
        assert_eq!(status(pack, &installed), Status::Installed { plugin: true });
        // An update reads its marketplace again and updates the plugin.
        assert_eq!(
            script(
                pack,
                plugin,
                Action::Update,
                &[(&claude_target, &installed)],
                PackageManager::Npm,
                &env()
            )
            .unwrap(),
            [
                "claude plugin marketplace update improve",
                "claude plugin update improve@improve",
            ]
        );
        // Removing: the plugin where it's installed, the folder where it's a folder.
        let env_h = Env {
            home: h.to_path_buf(),
            claude_config_dir_set: false,
        };
        assert_eq!(
            script(
                pack,
                cli,
                Action::Remove,
                &[(&claude_target, &installed)],
                PackageManager::Npm,
                &env_h
            )
            .unwrap(),
            ["claude plugin uninstall improve@improve"]
        );
        assert_eq!(
            script(
                pack,
                cli,
                Action::Remove,
                &[(&shared_dir, &read_installed(&shared_dir))],
                PackageManager::Npm,
                &env_h
            )
            .unwrap(),
            ["rm -rf \"$HOME/.agents/skills/improve\""]
        );
    }

    /// Every Claude Code profile has its own folder; agents that read the same folder share one
    /// target; clients without skills have none.
    #[test]
    fn targets_follow_the_agents_on_this_mac() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        std::fs::create_dir_all(h.join(".codex/skills")).unwrap();
        let clients = vec![
            client(Client::ClaudeCode, None),
            mcp_clients::Target {
                profile: "work".into(),
                profile_dir: Some(h.join(".claude-work")),
                ..client(Client::ClaudeCode, None)
            },
            client(Client::Codex, None),
            client(Client::Zed, None),
            client(Client::Cursor, None),
            client(Client::ClaudeDesktop, None),
        ];
        let targets = targets_for(h, clients);
        let labels: Vec<&str> = targets.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Claude Code",
                "Claude Code · work",
                "Codex, Zed",
                "Cursor",
                "Codex (older folder)"
            ]
        );
        assert_eq!(targets[0].dir, h.join(".claude/skills"));
        assert_eq!(targets[1].dir, h.join(".claude-work/skills"));
        assert_eq!(
            targets[1].claude.as_ref().unwrap().profile_dir.as_deref(),
            Some(h.join(".claude-work").as_path())
        );
        assert_eq!(targets[2].dir, h.join(".agents/skills"));
        assert_eq!(targets[2].cli_agent, Some("codex"));
        assert_eq!(targets[3].cli_agent, Some("cursor"));
        // The skills CLI doesn't write Codex's older folder; a copy can.
        assert_eq!(targets[4].cli_agent, None);
    }

    /// A pack is installed in a folder that has all its skills (links followed), partly when it
    /// has some, and in a Claude Code profile whose user plugins include it.
    #[test]
    fn what_a_folder_has_is_read_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        let skill = |root: &Path, name: &str| {
            std::fs::create_dir_all(root.join(name)).unwrap();
            std::fs::write(root.join(name).join("SKILL.md"), "---\nname: x\n---\n").unwrap();
        };
        let claude_dir = h.join(".claude");
        skill(&h.join(".agents/skills"), "cf-cli");
        std::fs::create_dir_all(claude_dir.join("skills")).unwrap();
        std::os::unix::fs::symlink(
            h.join(".agents/skills/cf-cli"),
            claude_dir.join("skills/cf-cli"),
        )
        .unwrap();
        // A folder without SKILL.md isn't a skill.
        std::fs::create_dir_all(claude_dir.join("skills/agents-sdk")).unwrap();
        let cache = h.join("cache/superpowers/7.0.0");
        skill(&cache.join("skills"), "brainstorming");
        std::fs::create_dir_all(claude_dir.join("plugins")).unwrap();
        std::fs::write(
            claude_dir.join("plugins/installed_plugins.json"),
            serde_json::json!({"version": 2, "plugins": {
                "superpowers@claude-plugins-official": [{"scope": "user", "installPath": cache}],
                "animate@animate": [{"scope": "project", "projectPath": "/p", "installPath": "/nope"}]
            }})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            claude_dir.join("plugins/known_marketplaces.json"),
            r#"{"claude-plugins-official": {}, "animate": {}}"#,
        )
        .unwrap();
        let target = Target {
            dir: claude_dir.join("skills"),
            claude: Some(ClaudeHome {
                dir: claude_dir.clone(),
                profile_dir: None,
            }),
            ..claude(None)
        };
        let installed = read_installed(&target);
        assert_eq!(installed.folder, HashSet::from(["cf-cli".to_string()]));
        assert!(installed.from_plugins.contains("brainstorming"));
        assert_eq!(
            installed.plugins,
            HashSet::from(["superpowers@claude-plugins-official".to_string()])
        );
        assert!(installed.marketplaces.contains("animate"));
        let status = |id| status(get(id).unwrap(), &installed);
        assert_eq!(status("cf-cli"), Status::Installed { plugin: false });
        assert_eq!(status("superpowers"), Status::Installed { plugin: true });
        // Installed for one project only.
        assert_eq!(status("animate"), Status::Missing);
        assert_eq!(status("cloudflare"), Status::Partial { have: 1, of: 19 });
    }

    /// The skills CLI runs with the package manager picked, globally, for the agents picked, without
    /// questions; each other Claude Code profile gets its own run with its CLAUDE_CONFIG_DIR.
    #[test]
    fn the_skills_cli_runs_with_the_package_manager_picked() {
        let none = Installed::default();
        let (main, work) = (claude(None), claude(Some("/Users/me/.claude-work")));
        let (shared, cursor) = (
            folder(".agents/skills", Some("codex")),
            folder(".cursor/skills", Some("cursor")),
        );
        let pack = get("interfaces").unwrap();
        let method = pack.methods[0];
        let lines = |pm, places: &[Place]| {
            script(pack, method, Action::Install, places, pm, &env()).unwrap()
        };
        assert_eq!(
            lines(PackageManager::Npm, &[(&main, &none), (&shared, &none)]),
            ["npx -y skills add jakubkrehel/skills --skill '*' -g -a claude-code -a codex -y"]
        );
        assert_eq!(
            lines(
                PackageManager::Bun,
                &[(&main, &none), (&work, &none), (&cursor, &none)]
            ),
            [
                "bunx skills add jakubkrehel/skills --skill '*' -g -a claude-code -a cursor -y",
                "CLAUDE_CONFIG_DIR=\"$HOME/.claude-work\" bunx skills add jakubkrehel/skills --skill '*' -g -a claude-code -y",
            ]
        );
        assert_eq!(
            lines(PackageManager::Pnpm, &[(&shared, &none)]),
            ["pnpm dlx skills add jakubkrehel/skills --skill '*' -g -a codex -y"]
        );
        // One skill of a bigger source, by name.
        let cf = get("cf-cli").unwrap();
        assert_eq!(
            script(
                cf,
                cf.methods[0],
                Action::Install,
                &[(&shared, &none)],
                PackageManager::Yarn,
                &env()
            )
            .unwrap(),
            ["yarn dlx skills add cloudflare/skills --skill cf-cli -g -a codex -y"]
        );
        // The app started with CLAUDE_CONFIG_DIR: the default profile's run unsets it.
        let set = Env {
            claude_config_dir_set: true,
            ..env()
        };
        assert_eq!(
            script(
                pack,
                method,
                Action::Install,
                &[(&main, &none)],
                PackageManager::Npm,
                &set
            )
            .unwrap(),
            [
                "env -u CLAUDE_CONFIG_DIR npx -y skills add jakubkrehel/skills --skill '*' -g -a claude-code -y"
            ]
        );
    }

    /// A plugin's marketplace is added once per profile, under its own name, and read again
    /// before an update; a plugin installed already is updated; a plugin can't go into another
    /// agent's folder.
    #[test]
    fn a_plugin_installs_into_each_claude_code_profile() {
        let none = Installed::default();
        let known = Installed {
            marketplaces: HashSet::from(["mcp-apps".to_string()]),
            plugins: HashSet::from(["mcp-apps@mcp-apps".to_string()]),
            ..Default::default()
        };
        let (main, work) = (claude(None), claude(Some("/Users/me/.claude-work")));
        let pack = get("mcp-apps").unwrap();
        let plugin = pack.plugin().unwrap();
        let run = |action, places: &[Place]| {
            script(pack, plugin, action, places, PackageManager::Npm, &env())
        };
        assert_eq!(
            run(Action::Install, &[(&main, &none)]).unwrap(),
            [
                "claude plugin marketplace add modelcontextprotocol/ext-apps",
                "claude plugin install mcp-apps@mcp-apps",
            ]
        );
        assert_eq!(
            run(Action::Update, &[(&work, &known)]).unwrap(),
            [
                "CLAUDE_CONFIG_DIR=\"$HOME/.claude-work\" claude plugin marketplace update mcp-apps",
                "CLAUDE_CONFIG_DIR=\"$HOME/.claude-work\" claude plugin update mcp-apps@mcp-apps",
            ]
        );
        let cursor = folder(".cursor/skills", Some("cursor"));
        let refused = run(Action::Install, &[(&cursor, &none)]).unwrap_err();
        assert!(refused.contains("only into Claude Code"), "{refused}");
        assert!(run(Action::Install, &[]).is_err());
        // A method the pack doesn't have.
        let other = get("superpowers").unwrap().methods[0];
        assert!(
            script(
                pack,
                other,
                Action::Install,
                &[(&main, &none)],
                PackageManager::Npm,
                &env()
            )
            .is_err()
        );
    }

    /// The documented clone and copy, into every folder picked, replacing what was there.
    #[test]
    fn a_copy_clones_once_and_copies_into_each_folder() {
        let none = Installed::default();
        let pack = get("unslop").unwrap();
        let shared = folder(".agents/skills", Some("codex"));
        let lines = script(
            pack,
            pack.methods[0],
            Action::Install,
            &[(&claude(None), &none), (&shared, &none)],
            PackageManager::Npm,
            &env(),
        )
        .unwrap();
        assert_eq!(
            lines,
            [
                "tmp=\"$(mktemp -d)\"",
                "trap 'rm -rf \"$tmp\"' EXIT",
                "git clone --depth 1 https://github.com/heydamir/unslop \"$tmp/unslop\"",
                "mkdir -p \"$HOME/.claude/skills\"",
                "rm -rf \"$HOME/.claude/skills/unslop\"",
                "cp -R \"$tmp/unslop/skills/unslop\" \"$HOME/.claude/skills\"/",
                "mkdir -p \"$HOME/.agents/skills\"",
                "rm -rf \"$HOME/.agents/skills/unslop\"",
                "cp -R \"$tmp/unslop/skills/unslop\" \"$HOME/.agents/skills\"/",
            ]
        );
    }

    /// Removing deletes only the pack's own skill folders where they are, and uninstalls its plugin
    /// where it's installed; with nothing there it says so instead.
    #[test]
    fn removing_deletes_only_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env {
            home: dir.path().to_path_buf(),
            claude_config_dir_set: false,
        };
        let mut main = claude(None);
        main.dir = dir.path().join(".claude/skills");
        let has = Installed {
            folder: HashSet::from(["create-mcp-app".to_string(), "unrelated".to_string()]),
            plugins: HashSet::from(["mcp-apps@mcp-apps".to_string()]),
            ..Default::default()
        };
        let pack = get("mcp-apps").unwrap();
        let lines = script(
            pack,
            pack.methods[0],
            Action::Remove,
            &[(&main, &has)],
            PackageManager::Npm,
            &env,
        )
        .unwrap();
        assert_eq!(
            lines,
            [
                "claude plugin uninstall mcp-apps@mcp-apps",
                "rm -rf \"$HOME/.claude/skills/create-mcp-app\"",
            ]
        );
        let nothing = script(
            pack,
            pack.methods[0],
            Action::Remove,
            &[(&main, &Installed::default())],
            PackageManager::Npm,
            &env,
        )
        .unwrap_err();
        assert!(nothing.contains("nothing to remove"), "{nothing}");
        // cf-cli there only through the Cloudflare plugin: removing cf-cli says to remove that.
        let through_plugin = Installed {
            from_plugins: HashSet::from(["cf-cli".to_string()]),
            plugins: HashSet::from(["cloudflare@cloudflare".to_string()]),
            ..Default::default()
        };
        let cf = get("cf-cli").unwrap();
        assert_eq!(
            status(cf, &through_plugin),
            Status::Installed { plugin: false }
        );
        let other = script(
            cf,
            cf.methods[0],
            Action::Remove,
            &[(&main, &through_plugin)],
            PackageManager::Npm,
            &env,
        )
        .unwrap_err();
        assert!(
            other.contains("another pack's Claude Code plugin"),
            "{other}"
        );
    }

    #[test]
    fn agents_are_found_by_label_agent_or_folder() {
        let mut work = claude(Some("/Users/me/.claude-work"));
        work.label = "Claude Code · work".into();
        let mut shared = folder(".agents/skills", Some("codex"));
        shared.label = "Codex, Zed".into();
        shared.clients = vec![Client::Codex, Client::Zed];
        let targets = [claude(None), work, shared];
        let find = |names: &[&str]| {
            find_targets(
                &targets,
                &names.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
            )
        };
        assert_eq!(find(&["Claude Code"]), Ok(vec![0]));
        assert_eq!(find(&["claude-code-work", "zed"]), Ok(vec![1, 2]));
        assert_eq!(find(&["Claude Code (profile work)"]), Ok(vec![1]));
        assert_eq!(find(&["/Users/me/.agents/skills", "codex"]), Ok(vec![2]));
        let unknown = find(&["cursor"]).unwrap_err();
        assert!(unknown.contains("Claude Code · work"), "{unknown}");
    }

    #[test]
    fn a_sample_shows_each_way_with_the_package_manager() {
        let cf = get("cf-cli").unwrap();
        assert_eq!(
            sample(cf, cf.methods[0], PackageManager::Bun),
            "bunx skills add cloudflare/skills --skill cf-cli -g -a <agent> -y"
        );
        let animate = get("animate").unwrap();
        assert_eq!(
            sample(animate, animate.methods[0], PackageManager::Npm),
            "claude plugin marketplace add cth9191/animate && claude plugin install animate@animate"
        );
        assert_eq!(
            sample(animate, animate.methods[1], PackageManager::Npm),
            "git clone --depth 1 https://github.com/cth9191/animate && cp -R animate/plugins/animate/skills/<skill> <agent's skills folder>/"
        );
    }

    #[test]
    fn paths_outside_home_are_quoted() {
        let e = env();
        assert_eq!(
            e.path(Path::new("/Users/me/.claude/skills")),
            "\"$HOME/.claude/skills\""
        );
        assert_eq!(e.path(Path::new("/opt/my skills")), "'/opt/my skills'");
        assert_eq!(e.path(Path::new("/Users/me/we$ird")), "'/Users/me/we$ird'");
    }

    /// Every script the catalog builds, each way, with each package manager, into every kind of
    /// agent (another profile, a folder outside the home folder with a space in it), is one the
    /// shell reads: checked with `sh -n`, which runs nothing.
    #[test]
    fn every_script_is_valid_shell() {
        let has_all = Installed {
            folder: PACKS
                .iter()
                .flat_map(|p| p.skills.iter().map(|s| s.to_string()))
                .collect(),
            plugins: PACKS
                .iter()
                .filter_map(|p| p.plugin()?.plugin_key())
                .collect(),
            ..Default::default()
        };
        let none = Installed::default();
        let (main, work) = (claude(None), claude(Some("/Users/me/.claude-work")));
        let odd = Target {
            dir: PathBuf::from("/opt/my skills"),
            ..folder(".x", Some("cursor"))
        };
        let set = Env {
            claude_config_dir_set: true,
            ..env()
        };
        let mut checked = 0;
        for pack in PACKS {
            for method in pack.methods {
                let places: Vec<Place> = [&main, &work, &odd]
                    .into_iter()
                    .filter(|t| method.reaches(t))
                    .map(|t| (t, &none))
                    .collect();
                for pm in PackageManager::ALL {
                    for action in [Action::Install, Action::Update] {
                        let lines = script(pack, *method, action, &places, pm, &set).unwrap();
                        let ok = std::process::Command::new("/bin/sh")
                            .args(["-n", "-c", &lines.join("\n")])
                            .status()
                            .unwrap()
                            .success();
                        assert!(ok, "{} {}: {lines:?}", pack.id, method.id());
                        checked += 1;
                    }
                }
            }
            let places: Vec<Place> = [&main, &work, &odd]
                .into_iter()
                .map(|t| (t, &has_all))
                .collect();
            let lines = script(
                pack,
                pack.methods[0],
                Action::Remove,
                &places,
                PackageManager::Npm,
                &set,
            )
            .unwrap();
            let ok = std::process::Command::new("/bin/sh")
                .args(["-n", "-c", &lines.join("\n")])
                .status()
                .unwrap()
                .success();
            assert!(ok, "remove {}: {lines:?}", pack.id);
        }
        assert!(checked > 50, "{checked}");
    }
}
