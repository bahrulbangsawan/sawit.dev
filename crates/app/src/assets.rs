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
        MoveVertical
    ]
);

/// Extra icons first, then the default `Assets` (registering an asset source
/// replaces the default one rather than adding to it).
pub struct AppAssets;

macro_rules! agent_logos {
    ($($name:literal),* $(,)?) => {
        /// AI client logos for the MCP page (see assets/agent_logos/SOURCES.md).
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
    "figma-color",
    "geminicli-color",
    "githubcopilot",
    "goose",
    "junie-color",
    "kilocode",
    "kiro-color",
    "lmstudio",
    "meta-color",
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

fn tool_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("tool_logos/")?.strip_suffix(".svg")?;
    TOOL_LOGOS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

fn agent_logo(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("agent_logos/")?.strip_suffix(".svg")?;
    AGENT_LOGOS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}

/// The Sawit mark (title bar).
pub const LOGO_MARK: &str = "logo/sawit-mark.svg";

/// The herdr logo (terminal dock), from https://herdr.dev/assets/logo.svg.
pub const HERDR_LOGO: &str = "logo/herdr.svg";

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(svg) = agent_logo(path).or_else(|| tool_logo(path)) {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        if let Some(svg) = crate::file_icons::asset(path) {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        if path == LOGO_MARK {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../assets/logo/sawit-mark.svg"
            ))));
        }
        if path == HERDR_LOGO {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../assets/logo/herdr.svg"
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
