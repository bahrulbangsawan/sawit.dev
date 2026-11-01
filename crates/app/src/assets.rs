//! App assets: gpui-component's default icon set plus a few extra Lucide
//! icons embedded individually (the full catalog would add ~1 MiB).

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Database,
        Server,
        Terminal,
        FileCode,
        Download,
        Table,
        X,
        Save,
        Circle,
        Eye,
        Lock,
        ChevronsLeft,
        ChevronsRight,
        Pin,
        PanelTop,
        WandSparkles,
        Image,
        FileText,
        FileSpreadsheet,
        Upload,
        Eraser,
        Film,
        Minus,
        Play,
        Pause,
        Volume2,
        VolumeX,
        Code,
        Columns2,
        File,
        Plug,
        Copy,
        Bot,
        MessageSquareText,
        Star,
        Check,
        CopyPlus,
        Trash,
        Tag,
        ScrollText,
        Waypoints,
        Radio,
        ZoomIn,
        ZoomOut,
        SquareCheck,
        Zap,
        Unplug,
        GitBranch,
        GitGraph,
        GitCommitHorizontal,
        GitMerge,
        GitPullRequest,
        LogIn,
        LogOut,
        CircleDot,
        MessageSquare,
        BookMarked,
        FileCog,
        FileTerminal,
        Pencil,
        UserPlus,
        Link,
        GitBranchMinus,
        Shapes,
        CircleStop,
        RotateCcw,
        Keyboard,
        ListFilter,
        TriangleAlert,
        Package,
        ListOrdered,
        Layers,
        ListTree,
        ShieldCheck,
        KeyRound,
        Funnel,
        FunnelX,
        ArrowUpDown,
        // The native canvas's tools and chrome (src/board).
        MousePointer2,
        Hand,
        Scaling,
        SquareDashed,
        Slice,
        Slash,
        ArrowUpRight,
        Triangle,
        PenTool,
        Type,
        PenLine,
        Highlighter,
        StickyNote,
        Spline,
        Stamp,
        Signature,
        ImagePlus,
        LayoutTemplate,
        // The native canvas's Design panel (src/board/design_panel).
        AlignStartVertical,
        AlignCenterVertical,
        AlignEndVertical,
        AlignStartHorizontal,
        AlignCenterHorizontal,
        AlignEndHorizontal,
        AlignHorizontalDistributeCenter,
        AlignVerticalDistributeCenter,
        Unlink,
        Contrast,
        SquareRoundCorner,
        Scan,
        Blend,
        EyeDashed,
        TextWrap,
        MoveHorizontal,
        MoveVertical,
        TextAlignStart,
        TextAlignCenter,
        TextAlignEnd,
        TextAlignJustify,
        ArrowUpToLine,
        FoldVertical,
        ArrowDownToLine,
        UnfoldVertical,
        UnfoldHorizontal,
        // The native canvas's layers panel (src/board/layers_panel.rs).
        LockOpen,
        Group,
        Component,
        Diamond,
        // The pictures of the canvas's choices (src/board/option_icons.rs).
        Diff,
        RulerDimensionLine,
        FoldHorizontal,
        ScanSearch,
        // The browser's device menu (src/browser/toolbar.rs).
        Smartphone,
        Tablet,
        Laptop,
        Monitor,
        MonitorSmartphone,
        Proportions,
        // The browser's Annotate panel (src/browser/annotate.rs).
        Camera,
        // The Dev Tools and MCP catalogs (src/catalog.rs, tools_page.rs, mcp_page.rs).
        CircleArrowUp,
        CircleDashed,
        CircleMinus,
        Clock,
        SearchX,
        // The Skills page (src/skills_page.rs).
        Sparkles,
        Workflow,
        Blocks,
        Clapperboard,
        // The New Project page (src/new_project_page.rs).
        FolderPlus,
        ListChecks
    ]
);

/// Extra icons first, then the default `Assets` (registering an asset source
/// replaces the default one rather than adding to it).
pub struct AppAssets;

macro_rules! agent_logos {
    ($($name:literal),* $(,)?) => {
        /// AI client logos for the MCP page and the terminal's agents (see
        /// assets/agent_logos/SOURCES.md).
        const AGENT_LOGOS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../assets/agent_logos/", $name, ".svg")))),*
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
            $(($name, include_bytes!(concat!("../../../assets/tool_logos/", $name, ".svg")))),*
        ];
    };
}

tool_logos!(
    "bun",
    "cap-color",
    "deno",
    "factory-color",
    "flyio",
    "homebrew-color",
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
const TOOL_ICONS: &[(&str, &[u8])] = &[(
    "puremac",
    include_bytes!("../../../assets/tool_logos/puremac.png"),
)];

/// Database logos for the database sidebar, its add menu, the SQL editor and the welcome
/// screen (see assets/db_logos/SOURCES.md and `crate::db::logos`).
const DB_LOGOS: &[(&str, &[u8])] = &[
    (
        "postgresql-color",
        include_bytes!("../../../assets/db_logos/postgresql-color.svg"),
    ),
    ("d1", include_bytes!("../../../assets/db_logos/d1.svg")),
    (
        "sqlite-color",
        include_bytes!("../../../assets/db_logos/sqlite-color.svg"),
    ),
];

fn db_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("db_logos/")?.strip_suffix(".svg")?;
    DB_LOGOS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

fn tool_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("tool_logos/")?;
    if let Some(name) = name.strip_suffix(".png") {
        return TOOL_ICONS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b);
    }
    let name = name.strip_suffix(".svg")?;
    TOOL_LOGOS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

fn agent_logo(path: &str) -> Option<&'static [u8]> {
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
            $(($name, include_bytes!(concat!("../../../assets/stack_logos/", $name, ".svg")))),*
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

fn stack_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("stack_logos/")?.strip_suffix(".svg")?;
    STACK_LOGOS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}

/// The Sawit mark (title bar).
pub const LOGO_MARK: &str = "logo/sawit-mark.svg";

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(svg) = agent_logo(path)
            .or_else(|| tool_logo(path))
            .or_else(|| db_logo(path))
            .or_else(|| stack_logo(path))
        {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        if let Some(svg) = crate::file_icons::asset(path) {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        if let Some(svg) = crate::board::option_icons::asset(path) {
            return Ok(Some(svg));
        }
        if path == LOGO_MARK {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../assets/logo/sawit-mark.svg"
            ))));
        }
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = ExtraIcons.list(path)?;
        all.extend(gpui_kit::assets::Assets.list(path)?);
        all.sort();
        all.dedup();
        Ok(all)
    }
}
