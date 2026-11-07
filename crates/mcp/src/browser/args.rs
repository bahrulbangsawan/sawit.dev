//! The arguments of the `browser_*` tools, as an agent sends them.

use rmcp::schemars;

/// Which browser an op goes to: the project's (by its folder; default the one the agent works
/// in), and one of its tabs.
#[derive(Debug, Default, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct At {
    pub project: Option<String>,
    #[schemars(
        description = "A browser tab, by id, title or address (browser_tabs lists them); default the tab on show. A tab not on show is woken off screen: the user's tab and keyboard stay as they are"
    )]
    pub tab: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NavigateArgs {
    #[schemars(
        description = "Address to open: a URL, localhost:5173, or words to search. Omit (with no action) to open the project's running dev server"
    )]
    pub url: Option<String>,
    #[schemars(description = "back, forward or reload instead of a url")]
    pub action: Option<String>,
    #[schemars(description = "How long to wait for the page to load, in ms (default 15000)")]
    pub timeout_ms: Option<u64>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ElementArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Element ref from browser_snapshot, e.g. e12"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ClickArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Element ref from browser_snapshot, e.g. e12"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "Double-click")]
    #[serde(default)]
    pub double: bool,
    #[schemars(description = "left (default) or right")]
    pub button: Option<String>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TypeArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Field ref from browser_snapshot; default the focused element"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    pub text: String,
    #[schemars(description = "Empty the field first")]
    #[serde(default)]
    pub clear: bool,
    #[schemars(description = "Press Enter afterwards (submits a form)")]
    #[serde(default)]
    pub submit: bool,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct KeyArgs {
    #[schemars(
        description = "Enter, Tab, Shift+Tab, Escape, Backspace, Delete, Space, ArrowDown, PageDown, Home, End, Meta+a, or one character. Meta+Shift+. is Sawit's: it opens or closes the Annotate panel"
    )]
    pub key: String,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SelectArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "The <select>'s ref from browser_snapshot"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "Option values or visible labels (several for a multiple select)")]
    pub values: Vec<String>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScrollArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Element to scroll (or, with no dx/dy, to bring into view); default the page"
    )]
    pub reference: Option<String>,
    pub selector: Option<String>,
    #[schemars(description = "Pixels right (negative: left)")]
    pub dx: Option<f64>,
    #[schemars(description = "Pixels down (negative: up)")]
    pub dy: Option<f64>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WaitArgs {
    #[schemars(description = "Wait until this text is on the page (any case)")]
    pub text: Option<String>,
    #[schemars(description = "Wait until this text is gone")]
    pub text_gone: Option<String>,
    #[schemars(description = "Wait until a visible element matches this CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "Or just wait this many ms")]
    pub time_ms: Option<u64>,
    #[schemars(description = "Give up after this many ms (default and most 30000)")]
    pub timeout_ms: Option<u64>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ScreenshotArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Capture only this element (ref from browser_snapshot)"
    )]
    pub reference: Option<String>,
    pub selector: Option<String>,
    #[schemars(
        description = "Absolute path (or ~/…) to write the PNG to; default Sawit's cache folder"
    )]
    pub output_path: Option<String>,
    #[schemars(
        description = "While Annotate's page tools (mesurer) are on the page: hide (default) leaves them out; drawings keeps their drawings, guides, layout guides and comment pins, without the toolbar and panels; show keeps everything"
    )]
    pub mesurer: Option<String>,
    #[schemars(description = "The whole scrollable page, not just the part on show")]
    #[serde(default)]
    pub full_page: bool,
    #[schemars(
        description = "Lay the page out as this device for the shot (any browser_resize preset, e.g. iPhone 15, iPad Air, MacBook Air, Full HD); the tab's own size comes back after"
    )]
    pub device: Option<String>,
    #[schemars(
        description = "Several devices in one call: one image per device, e.g. [\"iPhone 15\", \"iPad Pro 11″\", \"Laptop\", \"Desktop\"]"
    )]
    #[serde(default)]
    pub devices: Vec<String>,
    #[schemars(description = "Or a width in CSS px (with height)")]
    pub width: Option<u32>,
    #[schemars(description = "And a height in CSS px")]
    pub height: Option<u32>,
    #[schemars(
        description = "Pixels per CSS px, 1 to 3; default the device's own (3 for phones, 2 for tablets and laptops, 1 for desktops)"
    )]
    pub scale: Option<f64>,
    #[schemars(description = "png (default), jpeg, or pdf (full page only, vector)")]
    pub format: Option<String>,
    #[schemars(description = "JPEG quality, 0 to 1 (default 0.9)")]
    pub quality: Option<f64>,
    #[schemars(description = "Only this box of the page: [x, y, width, height] in CSS px")]
    pub clip: Option<[f64; 4]>,
    #[serde(flatten)]
    pub at: At,
}

impl ScreenshotArgs {
    /// Whether any of the sized-capture options is asked for.
    pub fn sized(&self) -> bool {
        self.full_page
            || self.device.is_some()
            || !self.devices.is_empty()
            || self.width.is_some()
            || self.height.is_some()
            || self.scale.is_some()
            || self.format.is_some()
            || self.quality.is_some()
            || self.clip.is_some()
    }
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct RecordArgs {
    #[schemars(
        description = "view (default): the part on show; full_page: a scroll through the whole page, top to bottom; element: one element (ref or selector); region: a clip"
    )]
    pub mode: Option<String>,
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "element mode: the element's ref from browser_snapshot"
    )]
    pub reference: Option<String>,
    #[schemars(description = "element mode: or a CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "region mode: [x, y, width, height] in CSS px")]
    pub clip: Option<[f64; 4]>,
    #[schemars(
        description = "Lay the page out as this device while recording (any browser_resize preset)"
    )]
    pub device: Option<String>,
    #[schemars(description = "Or a width in CSS px (with height)")]
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[schemars(
        description = "Output resolution by its short side: 360p, 480p, 720p, 1080p, 1440p or 2160p; the page's shape is kept"
    )]
    pub resolution: Option<String>,
    #[schemars(description = "Or the widest the video may be, in pixels")]
    pub max_width: Option<u32>,
    #[schemars(description = "15, 24, 30 (default) or 60; the rate achieved is reported")]
    pub fps: Option<u32>,
    #[schemars(
        description = "mp4 (H.264, default), mov (HEVC, smaller, and above 4096 px) or gif (short clips)"
    )]
    pub format: Option<String>,
    #[schemars(description = "full_page: scroll speed in CSS px a second")]
    pub scroll_speed: Option<f64>,
    #[schemars(description = "full_page: or how long the scroll takes, in seconds")]
    pub duration: Option<f64>,
    #[schemars(description = "Stops on its own after this many seconds (default 30, at most 300)")]
    pub max_seconds: Option<u32>,
    #[schemars(
        description = "Absolute path (or ~/…) for the file; default the project's screenshot folder"
    )]
    pub output_path: Option<String>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct MesurerArgs {
    #[schemars(
        description = "state (default): what the page tools have on the page; open or close: Annotate with its page tools; set: change them; clear: remove what is on the page"
    )]
    pub action: Option<String>,
    #[schemars(description = "state: include each comment thread's HTML preview of its element")]
    #[serde(default)]
    pub full: bool,
    #[schemars(description = "set: X-ray (every element's box outlined) on or off")]
    pub xray: Option<bool>,
    #[schemars(description = "set: rulers on or off")]
    pub rulers: Option<bool>,
    #[schemars(
        description = "set: the tool in hand: none, select (inspect), selection, guides, arrows, pen, text or comments"
    )]
    pub tool: Option<String>,
    #[schemars(description = "set: the toolbar minimized, or shown")]
    pub minimized: Option<bool>,
    #[schemars(
        description = "set: the page's guides, in place of the ones there: CSS px from the viewport's left (vertical) or top (horizontal)"
    )]
    pub guides: Option<Vec<GuideArg>>,
    #[schemars(
        description = "set: the page's layout guides, in place of the ones there: columns, rows or a pixel grid"
    )]
    pub layout_guides: Option<Vec<LayoutGuideArg>>,
    #[schemars(
        description = "clear: drawings (arrows, pen strokes, text notes), measurements, guides, layout_guides, comments, or all; on the page on show"
    )]
    pub what: Option<String>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct GuideArg {
    #[schemars(description = "vertical (default) or horizontal")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<String>,
    #[schemars(description = "CSS px from the viewport's left or top")]
    pub position: f64,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct LayoutGuideArg {
    #[schemars(description = "columns (default), rows or grid")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[schemars(description = "How many columns or rows, 1 to 24 (default 5)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[schemars(
        description = "Each column's or row's width in px, used with align min, center or max (with stretch they fill the page); a grid's square (default 72)"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[schemars(description = "Px between columns or rows (default 20)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gutter: Option<f64>,
    #[schemars(description = "Px from the edge (default 0)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    #[schemars(description = "stretch (default), min, center or max")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[schemars(description = "A CSS colour (default #FF0000)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[schemars(description = "0 to 1 (default 0.1)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[schemars(description = "Shown (default true)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ConsoleArgs {
    #[schemars(description = "error, warning, info, log or debug; default all")]
    pub level: Option<String>,
    #[schemars(description = "Only messages containing this text (any case)")]
    pub pattern: Option<String>,
    #[schemars(description = "Newest messages to return (default 100)")]
    pub limit: Option<usize>,
    #[schemars(
        description = "Empty the tab's console log after reading: every message kept so far, not only those returned, and with them the page's console errors that the user's Annotate notes add when copied or sent to a terminal"
    )]
    #[serde(default)]
    pub clear: bool,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NetworkArgs {
    #[schemars(description = "Only URLs containing this text (any case)")]
    pub pattern: Option<String>,
    #[schemars(description = "Only failed requests (status 400+ or no response)")]
    #[serde(default)]
    pub failed_only: bool,
    #[schemars(description = "Newest requests to return (default 100)")]
    pub limit: Option<usize>,
    #[schemars(
        description = "Empty the tab's network log after reading: every request kept so far, not only those returned, so the failed-request count browser_navigate answers starts again from zero"
    )]
    #[serde(default)]
    pub clear: bool,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EvaluateArgs {
    #[schemars(
        description = "A JS expression (document.title), or a function body with return; a returned promise is awaited"
    )]
    pub script: String,
    #[schemars(description = "How long to await a promise, in ms (default 10000)")]
    pub timeout_ms: Option<u64>,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AnnotationsArgs {
    #[schemars(description = "Remove the annotations after reading them")]
    #[serde(default)]
    pub clear: bool,
    #[serde(flatten)]
    pub at: At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResizeArgs {
    #[schemars(
        description = "A device preset (iPhone SE, iPhone 15, iPhone 15 Pro Max, Pixel 8, Galaxy S, Small Android, iPad mini, iPad Air, iPad Pro 11″, iPad Pro 13″, Android tablet, Laptop, Laptop 1366, MacBook Air, MacBook Pro 14″, Laptop 1536, Desktop, Full HD, QHD, 4K), or responsive to fill the tab"
    )]
    pub device: Option<String>,
    #[schemars(description = "Viewport width in CSS px (with height)")]
    pub width: Option<u32>,
    #[schemars(description = "Viewport height in CSS px (with width)")]
    pub height: Option<u32>,
    #[serde(flatten)]
    pub at: At,
}
