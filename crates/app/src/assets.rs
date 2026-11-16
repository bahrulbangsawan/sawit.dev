//! App assets: gpui-component's default icon set plus a few extra Lucide
//! icons embedded individually (the full catalog would add ~1 MiB).

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

mod logos;

use logos::{agent_logo, db_logo, stack_logo, tool_logo};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Database,
        Server,
        // The browser's SEO panel: its toolbar switch, check states and a missing share image.
        // (CircleMinus and CircleDashed: listed with the catalogs below.)
        SearchCheck,
        // The browser's capture menu and bar.
        Ratio,
        Video,
        Square,
        ImageOff,
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
        // The Accounts page: the page icon and Import from Claude Pool.
        Users,
        FolderInput,
        // The find bar's options and Replace All.
        WholeWord,
        Regex,
        ReplaceAll,
        // The footer's Pages menu and the schema diagram's Auto Layout; GitHub repository rows.
        LayoutGrid,
        Archive,
        GitFork,
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
        Locate,
        LocateFixed, // Follow agent (B4.1)
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
        // The native canvas's layers panel (src/board/layers_panel/).
        LockOpen,
        Group,
        Component,
        Diamond,
        // The pictures of the canvas's choices (src/board/option_icons/).
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
        // The browser's Annotate panel (src/browser/annotate/).
        Camera,
        // The Dev Tools and MCP catalogs (src/catalog/, tools_page/, mcp_page/).
        CircleArrowUp,
        Stethoscope,
        CircleDashed,
        CircleMinus,
        Clock,
        SearchX,
        // The Skills page (src/skills_page/).
        Sparkles,
        Workflow,
        Blocks,
        Clapperboard,
        // The New Project page (src/new_project_page/).
        FolderPlus,
        ListChecks,
        // Activity icons on the tabs: needs attention.
        BellDot,
        // The Accounts page's tabs, sections and settings rows.
        ChartColumn,
        Gauge,
        Share2,
        Power,
        Wrench,
        // Menus and floating bars (B16, src/menu_icons.rs, src/board/menus/icons.rs).
        AppWindow,
        ArrowLeftRight,
        ArrowLeftToLine,
        ArrowRightToLine,
        BringToFront,
        ChevronsDownUp,
        ClipboardCopy,
        ClipboardPaste,
        Cloud,
        Crop,
        FilePlus,
        FlipHorizontal2,
        FlipVertical2,
        GitBranchPlus,
        MousePointerClick,
        MoveRight,
        PinOff,
        Pipette,
        Rows2,
        Scissors,
        Send,
        SendToBack,
        SquareMinus,
        SquarePen,
        Timer,
        // The Pomodoro pill's Skip.
        SkipForward,
        Ungroup,
        Variable,
        Vote,
        // The pool's views (Overview, Detailed, Compare, Timeline, Analytics, Wallet).
        List,
        CalendarClock,
        TrendingUp,
        Wallet,
        GitCompare,
        Flame,
    ]
);

/// Extra icons first, then the default `Assets` (registering an asset source
/// replaces the default one rather than adding to it).
pub struct AppAssets;

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

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource as _;
    use gpui_kit::assets::IconName;

    use super::AppAssets;

    /// Every icon the app's code names (`Lucide::…` or `IconName::…`) is one the app embeds: the
    /// default set or the extras above. One that isn't draws nothing, and an icon-only button
    /// with nothing drawn is a button nobody can see.
    #[test]
    fn every_icon_the_app_names_is_embedded() {
        let mut names = std::collections::BTreeSet::new();
        let mut dirs = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs")
                    // macOS's `._` metadata files beside sources on a non-Apple disk (exFAT).
                    && !entry.file_name().to_string_lossy().starts_with("._")
                {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for prefix in ["Lucide::", "IconName::"] {
                        for (at, _) in text.match_indices(prefix) {
                            let name: String = text[at + prefix.len()..]
                                .chars()
                                .take_while(|c| c.is_ascii_alphanumeric())
                                .collect();
                            if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                                names.insert(name);
                            }
                        }
                    }
                }
            }
        }
        assert!(
            names.len() > 50,
            "found the icons the code names: {}",
            names.len()
        );
        let missing: Vec<_> = names
            .iter()
            .filter(|name| {
                let icon = IconName::ALL.iter().find(|i| format!("{i:?}") == **name);
                icon.is_some_and(|i| !matches!(AppAssets.load(&i.path()), Ok(Some(_))))
            })
            .collect();
        assert!(missing.is_empty(), "named but not embedded: {missing:?}");
    }
}
