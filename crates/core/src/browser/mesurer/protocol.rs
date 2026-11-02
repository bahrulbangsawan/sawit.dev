//! What the glue posts to the app (`page/mesurer/bridge.js`): one JSON message per post,
//! `{"kind": …, "payload": …, "url": …}`, answered with a JSON value or an error. The app's side of
//! each kind belongs to one lane ([`Lane`]), so the lanes add handlers without touching the
//! dispatch.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A message from mesurer's world. `kind` names it, `payload` carries its fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum FromMesurer {
    /// mesurer's tree is in its shadow root, `mount_ms` after `SawitMesurer.mount` began.
    #[serde(rename = "ready")]
    Ready {
        version: String,
        url: String,
        mount_ms: f64,
    },
    /// A line for Sawit's log (`info`, `warn`, `error`).
    #[serde(rename = "log")]
    Log { level: String, text: String },
    /// mesurer's settings (`MesurerStoredSettings`), kept app-wide.
    #[serde(rename = "store.settings")]
    StoreSettings(Value),
    /// The page's origin's workspace (`PagedWorkspaceStore`: session chrome and `pages`).
    #[serde(rename = "store.workspace")]
    StoreWorkspace(Value),
    /// "Clear workspace" or "Use defaults" in mesurer's settings.
    #[serde(rename = "store.clear")]
    StoreClear { what: ClearWhat },
    /// Text for the pasteboard (selectors, colours, comments for an agent).
    #[serde(rename = "clipboard.text")]
    ClipboardText { text: String },
    /// A PNG for the pasteboard (a region screenshot). Lane 1.
    #[serde(rename = "clipboard.image")]
    ClipboardImage { png_base64: String },
    /// A download (a screenshot or a recording). Lane 1; replies `{path}`.
    #[serde(rename = "file.save")]
    FileSave {
        name: String,
        mime: String,
        base64: String,
    },
    /// The visible page, as `captureVisibleTab` gives it. Lane 1.
    #[serde(rename = "capture.visible")]
    CaptureVisible {},
    /// One frame of the page for a recording. Lane 1.
    #[serde(rename = "capture.frame")]
    CaptureFrame {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quality: Option<f64>,
    },
    /// `EyeDropper.open()`: sample a colour on screen. Lane 1.
    #[serde(rename = "color.sample")]
    ColorSample {},
    /// The sampling was aborted. Lane 1.
    #[serde(rename = "color.cancel")]
    ColorCancel {},
    /// "Copy for agent": the comments as mesurer wrote them. Lane 2.
    #[serde(rename = "comments.copied")]
    CommentsCopied { text: String },
    /// "Resolve all". Lane 2.
    #[serde(rename = "comments.resolved")]
    CommentsResolved {},
    /// mesurer's tool and chrome, as they change. Lane 3.
    #[serde(rename = "ui.state")]
    UiState(Value),
    /// A link mesurer opens (Settings' website): the default browser. Lane 3.
    #[serde(rename = "link.open")]
    LinkOpen { url: String },
}

/// What `store.clear` clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClearWhat {
    Workspace,
    Settings,
}

/// Who answers a kind in the app: the foundation (`browser/mesurer/`), or a lane's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// ready, log, store.*, clipboard.text.
    Foundation,
    /// clipboard.image, file.save, capture.*, color.* (`browser/mesurer/capture.rs`).
    Capture,
    /// comments.* (`browser/mesurer/agents.rs`).
    Agents,
    /// ui.state, link.open (`browser/mesurer/keys.rs`).
    Keys,
}

impl FromMesurer {
    /// The message's `kind`, as the glue posts it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "ready",
            Self::Log { .. } => "log",
            Self::StoreSettings(_) => "store.settings",
            Self::StoreWorkspace(_) => "store.workspace",
            Self::StoreClear { .. } => "store.clear",
            Self::ClipboardText { .. } => "clipboard.text",
            Self::ClipboardImage { .. } => "clipboard.image",
            Self::FileSave { .. } => "file.save",
            Self::CaptureVisible {} => "capture.visible",
            Self::CaptureFrame { .. } => "capture.frame",
            Self::ColorSample {} => "color.sample",
            Self::ColorCancel {} => "color.cancel",
            Self::CommentsCopied { .. } => "comments.copied",
            Self::CommentsResolved {} => "comments.resolved",
            Self::UiState(_) => "ui.state",
            Self::LinkOpen { .. } => "link.open",
        }
    }

    /// Who answers it.
    pub fn lane(&self) -> Lane {
        match self {
            Self::Ready { .. }
            | Self::Log { .. }
            | Self::StoreSettings(_)
            | Self::StoreWorkspace(_)
            | Self::StoreClear { .. }
            | Self::ClipboardText { .. } => Lane::Foundation,
            Self::ClipboardImage { .. }
            | Self::FileSave { .. }
            | Self::CaptureVisible {}
            | Self::CaptureFrame { .. }
            | Self::ColorSample {}
            | Self::ColorCancel {} => Lane::Capture,
            Self::CommentsCopied { .. } | Self::CommentsResolved {} => Lane::Agents,
            Self::UiState(_) | Self::LinkOpen { .. } => Lane::Keys,
        }
    }
}

/// A message with the address of the page that posted it.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub message: FromMesurer,
    /// `location.href` when it was posted; none from an older glue.
    pub url: Option<String>,
}

/// Reads a posted message and the page it came from.
pub fn parse_envelope(body: &str) -> Result<Envelope, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("mesurer sent a bad message: {e}"))?;
    let url = value.get("url").and_then(Value::as_str).map(str::to_string);
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut message = serde_json::Map::new();
    message.insert(
        "kind".into(),
        value.get("kind").cloned().unwrap_or(Value::Null),
    );
    if let Some(payload) = value.get("payload") {
        message.insert("payload".into(), payload.clone());
    }
    let message = serde_json::from_value(Value::Object(message))
        .map_err(|e| format!("mesurer sent a bad “{kind}” message: {e}"))?;
    Ok(Envelope { message, url })
}

/// Reads a posted message.
pub fn parse(body: &str) -> Result<FromMesurer, String> {
    parse_envelope(body).map(|e| e.message)
}
