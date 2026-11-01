//! The arguments of the `browser_*` tools, as an agent sends them.

use rmcp::schemars;

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
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    pub project: Option<String>,
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
    pub project: Option<String>,
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
    pub project: Option<String>,
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
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct KeyArgs {
    #[schemars(
        description = "Enter, Tab, Shift+Tab, Escape, Backspace, Delete, Space, ArrowDown, PageDown, Home, End, Meta+a, or one character. Meta+Shift+. is Sawit's: it opens or closes the Annotate panel"
    )]
    pub key: String,
    pub project: Option<String>,
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
    pub project: Option<String>,
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
    pub project: Option<String>,
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
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
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
    pub project: Option<String>,
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
    pub project: Option<String>,
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
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EvaluateArgs {
    #[schemars(
        description = "A JS expression (document.title), or a function body with return; a returned promise is awaited"
    )]
    pub script: String,
    #[schemars(description = "How long to await a promise, in ms (default 10000)")]
    pub timeout_ms: Option<u64>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AnnotationsArgs {
    #[schemars(description = "Remove the annotations after reading them")]
    #[serde(default)]
    pub clear: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResizeArgs {
    #[schemars(
        description = "iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop, or responsive to fill the tab"
    )]
    pub device: Option<String>,
    #[schemars(description = "Viewport width in CSS px (with height)")]
    pub width: Option<u32>,
    #[schemars(description = "Viewport height in CSS px (with width)")]
    pub height: Option<u32>,
    pub project: Option<String>,
}
