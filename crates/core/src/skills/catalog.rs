//! The catalog: each pack, the skills it brings, and how its source documents installing it.

use super::{Category, Method, Pack};

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

/// Where the skill Sawit ships lives in its repository.
const CANVAS_DESIGN_HOME: &str =
    "https://github.com/bahrulbangsawan/sawit.dev/tree/main/skills/sawit-canvas-design";

/// The catalog. Each pack's commands are the ones its source documents
/// (README, docs page); see `home` and `repo`.
pub const PACKS: &[Pack] = &[
    // Not `canvas-design`: Anthropic's skills (and their example-skills plugin) have a skill of
    // that name, and a folder of it is theirs.
    Pack {
        id: "sawit-canvas-design",
        name: "Canvas Design",
        by: "Sawit",
        about: "Sawit's design standard for interfaces made on its canvas: layout, type, colour, spacing, copy, components, states, accessibility, motion and polish, with a checklist to run before a design is done.",
        category: Category::Design,
        skills: &["sawit-canvas-design"],
        home: CANVAS_DESIGN_HOME,
        repo: CANVAS_DESIGN_HOME,
        methods: &[Method::Bundled {
            name: "sawit-canvas-design",
        }],
        note: Some("Ships inside Sawit: installs this Sawit's version, offline."),
    },
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
