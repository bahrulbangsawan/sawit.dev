//! Brand logos embedded for the app's pages: AI clients, dev tools, databases
//! and New Project stacks.

macro_rules! agent_logos {
    ($($name:literal),* $(,)?) => {
        /// AI client logos for the MCP page and the terminal's agents (see
        /// assets/agent_logos/SOURCES.md).
        const AGENT_LOGOS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../../assets/agent_logos/", $name, ".svg")))),*
        ];
    };
}

agent_logos!(
    "amp-color",
    "antigravity-color",
    "aws",
    "claude-color",
    "claudecode-color",
    "cline",
    "cloudflare-color",
    "codex-color",
    "cursor",
    "exa-color",
    "factory-color",
    "figma-color",
    "geminicli-color",
    "github",
    "githubcopilot",
    "goose",
    "junie-color",
    "kilocode",
    "kiro-color",
    "lmstudio",
    "meta-color",
    "omp-color",
    "opencode",
    "qwen-color",
    "roocode",
    "trae-color",
    "warp",
    "windsurf",
    "zed",
);

macro_rules! tool_logos {
    ($($name:literal),* $(,)?) => {
        /// Logos for the Dev Tools page (see assets/tool_logos/SOURCES.md).
        const TOOL_LOGOS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../../assets/tool_logos/", $name, ".svg")))),*
        ];
    };
}

tool_logos!(
    "bun",
    "cap-color",
    "deno",
    "factory-color",
    "ffmpeg-color",
    "flyio",
    "github",
    "herdr-color",
    "homebrew-color",
    "hyperframes-color",
    "minio-color",
    "nodejs-color",
    "npm-color",
    "nvm-color",
    "omp-color",
    "pnpm-color",
    "python-color",
    "rust",
    "starship-color",
    "uv-color",
);

/// Tool logos that are an app's own icon, where it publishes no vector logo
/// (see assets/tool_logos/SOURCES.md).
const TOOL_ICONS: &[(&str, &[u8])] = &[
    (
        "puremac",
        include_bytes!("../../../../assets/tool_logos/puremac.png"),
    ),
    (
        "voxl",
        include_bytes!("../../../../assets/tool_logos/voxl.png"),
    ),
];

/// Database logos for the database sidebar, its add menu, the SQL editor and the welcome
/// screen (see assets/db_logos/SOURCES.md and `crate::db::logos`).
const DB_LOGOS: &[(&str, &[u8])] = &[
    (
        "postgresql-color",
        include_bytes!("../../../../assets/db_logos/postgresql-color.svg"),
    ),
    ("d1", include_bytes!("../../../../assets/db_logos/d1.svg")),
    (
        "sqlite-color",
        include_bytes!("../../../../assets/db_logos/sqlite-color.svg"),
    ),
];

pub(super) fn db_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("db_logos/")?.strip_suffix(".svg")?;
    DB_LOGOS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

pub(super) fn tool_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("tool_logos/")?;
    if let Some(name) = name.strip_suffix(".png") {
        return TOOL_ICONS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b);
    }
    let name = name.strip_suffix(".svg")?;
    TOOL_LOGOS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

pub(super) fn agent_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("agent_logos/")?.strip_suffix(".svg")?;
    AGENT_LOGOS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}

macro_rules! stack_logos {
    ($($name:literal),* $(,)?) => {
        /// Logos for the New Project page's stack cards (see assets/stack_logos/SOURCES.md).
        const STACK_LOGOS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../../assets/stack_logos/", $name, ".svg")))),*
        ];
    };
}

stack_logos!(
    "astro",
    "betterauth",
    "biome",
    "bun",
    "clerk",
    "cloudflare",
    "cloudflareworkers",
    "convex",
    "docker",
    "drizzle",
    "expo",
    "express",
    "fastify",
    "git",
    "hono",
    "lefthook",
    "modelcontextprotocol",
    "mongodb",
    "mongoose",
    "mysql",
    "neon",
    "nextdotjs",
    "nodedotjs",
    "npm",
    "nuxt",
    "nx",
    "oxc",
    "planetscale",
    "pnpm",
    "postgresql",
    "prisma",
    "pwa",
    "reactrouter",
    "solid",
    "sqlite",
    "supabase",
    "svelte",
    "tanstack",
    "tauri",
    "trpc",
    "turborepo",
    "turso",
    "vercel",
    "wxt",
);

pub(super) fn stack_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("stack_logos/")?.strip_suffix(".svg")?;
    STACK_LOGOS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}
