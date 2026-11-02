//! The ops an agent sends the browser tab (over `app_control`'s `Command::Browser`), and what one
//! gets while the tab has no page to run it.

use serde::{Deserialize, Serialize};

/// What an agent's op that doesn't open the browser gets while the project has none open.
pub const NOT_OPEN: &str = "The browser isn't open. Open a page with browser_navigate.";

/// What a page op gets while the page has no agent to run it: it is still loading, or isn't a
/// web page. Waiting for the page waits this out.
pub const NOT_READY: &str = "The page isn't ready yet (it may still be loading, or isn't a web page). Try again after browser_wait_for.";

/// How the error of a page op that WebKit couldn't hand to the page starts, the reason after it.
/// Waiting for the page waits this out too.
pub const UNREACHABLE: &str = "The page couldn't be reached";

/// An element: a ref from the last snapshot (`e12`) or a CSS selector.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Target {
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

impl Target {
    pub fn is_empty(&self) -> bool {
        self.reference.is_none() && self.selector.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrowserOp {
    Navigate {
        url: String,
    },
    Back,
    Forward,
    Reload,
    /// url, title, loading.
    State,
    /// The page as an outline with element refs.
    Snapshot,
    Click {
        #[serde(flatten)]
        target: Target,
        #[serde(default)]
        double: bool,
        /// `left` (default) or `right`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        button: Option<String>,
    },
    Hover {
        #[serde(flatten)]
        target: Target,
    },
    /// Into `target`, else the focused element.
    Type {
        #[serde(flatten)]
        target: Target,
        text: String,
        #[serde(default)]
        clear: bool,
        #[serde(default)]
        submit: bool,
    },
    /// A key (`Enter`, `Tab`, `a`) with modifiers (`Meta+a`) to the focused element.
    Press {
        key: String,
    },
    Select {
        #[serde(flatten)]
        target: Target,
        values: Vec<String>,
    },
    /// Scrolls `target` (or the page) by dx/dy; with a target and no
    /// deltas, scrolls it into view.
    Scroll {
        #[serde(flatten)]
        target: Target,
        #[serde(default)]
        dx: f64,
        #[serde(default)]
        dy: f64,
    },
    /// Whether `text` is on the page / `selector` matches (for waits).
    Query {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },
    /// A JS expression, or a function body when it uses `return`.
    Evaluate {
        script: String,
    },
    /// An `evaluate` whose script returned a promise answers `{"pending": id}`;
    /// this asks whether that promise has settled.
    Settled {
        id: u64,
    },
    /// The visible page, or one element's box. Answers `png_base64`.
    Screenshot {
        #[serde(flatten)]
        target: Target,
    },
    Console {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
        #[serde(default)]
        clear: bool,
    },
    Network {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(default)]
        failed_only: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
        #[serde(default)]
        clear: bool,
    },
    /// Opens Safari's Web Inspector on the page.
    Devtools,
    /// Sizes the page like a device (`device`) or `width` × `height`, or
    /// back to filling the tab (`device: "responsive"`).
    Resize {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
    },
    /// The annotations the user made, as markdown; `clear` removes them
    /// after the answer is built.
    Annotations {
        #[serde(default)]
        clear: bool,
    },
    /// An element's computed styles; `format` is `json` (default, with
    /// contrast), `css`, `tailwind` or `html`.
    Inspect {
        #[serde(flatten)]
        target: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },
    /// The page's design scan and derived tokens; `part` is `summary`
    /// (default), `colors`, `fonts`, `assets`, `tokens` or `variables`.
    Design {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<String>,
    },
    /// Starts writing the design export folder in the background (or joins
    /// the one running).
    ExportDesign {
        #[serde(default)]
        include_videos: bool,
        /// Replace what an earlier export left in the folder; without it, a
        /// folder that holds anything is refused, and nothing starts.
        #[serde(default)]
        replace: bool,
    },
    /// How the last design export is doing.
    ExportStatus,
    /// Opens the side panel (`annotate` or `design`, on its `tab`: inspect, colors, fonts,
    /// assets or system), or closes it (`panel` left out). For scripts and visual checks:
    /// agents read annotations and design through their own ops, panel or not.
    Panel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        panel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab: Option<String>,
    },
}

impl BrowserOp {
    /// Answered by the app itself rather than the page agent.
    pub fn is_native(&self) -> bool {
        matches!(
            self,
            Self::Navigate { .. }
                | Self::Back
                | Self::Forward
                | Self::Reload
                | Self::State
                | Self::Screenshot { .. }
                | Self::Console { .. }
                | Self::Network { .. }
                | Self::Devtools
                | Self::Resize { .. }
                | Self::Annotations { .. }
                | Self::Inspect { .. }
                | Self::Design { .. }
                | Self::ExportDesign { .. }
                | Self::ExportStatus
                | Self::Panel { .. }
        )
    }

    /// Brings the browser tab to the front (others work in the background).
    pub fn shows_tab(&self) -> bool {
        matches!(
            self,
            Self::Navigate { .. } | Self::Screenshot { .. } | Self::Resize { .. }
        )
    }
}
