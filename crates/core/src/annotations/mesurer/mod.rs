//! What the Annotate panel and agents take from mesurer's workspace (`browser::mesurer`, a
//! `PagedWorkspaceStore` as JSON): its comment threads, as rows ([`threads_of`]); the rest of a
//! page's markup, as a section of the notes ([`markup`]); and the changes Sawit makes to a
//! workspace for an agent or the panel ([`edit`]), which go back to mesurer through
//! `SawitMesurer.apply`.

pub mod edit;
pub mod markup;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ElementDetails, Rect};

pub use markup::{MarkPoint, Near, PageMarkup};

/// One of mesurer's comment threads, as its Annotate row carries it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    /// mesurer's id for it, unique in its workspace.
    pub id: String,
    pub messages: Vec<ThreadMessage>,
    pub status: ThreadStatus,
    /// The workspace it is kept in, by mesurer's origin key (`http_localhost_3000`), and its page
    /// there (mesurer's page key, `/docs?b=1`).
    pub origin: String,
    pub page: String,
    /// Left in one of Responsive's previews, this size in CSS px, rather than on the page.
    #[serde(default)]
    pub preview: Option<(u32, u32)>,
    /// The frames its element is in, outermost first (mesurer's `framePath`); none on the page.
    #[serde(default)]
    pub frames: Vec<String>,
    /// mesurer's selector of its element (`target.selector`): a new one when the pin was moved.
    #[serde(default)]
    pub target: String,
}

impl Thread {
    pub fn resolved(&self) -> bool {
        self.status == ThreadStatus::Resolved
    }

    /// Whether its element is in the page's own document, where Sawit's page agent can read it.
    pub fn in_document(&self) -> bool {
        self.preview.is_none() && self.frames.is_empty()
    }

    /// The first message: what a folded row shows.
    pub fn first(&self) -> &str {
        self.messages.first().map_or("", |m| m.text.as_str())
    }
}

/// A message of a thread: the user's own words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub text: String,
    /// When it was written, ms since the epoch (mesurer's `createdAt`).
    pub created_at: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadStatus {
    #[default]
    Open,
    Resolved,
}

/// The comment threads of a page's artifacts (`MesurerPageArtifacts` JSON, `null` for none), each
/// with its element as mesurer captured it ([`element_of`]). `origin` and `page` say where they
/// are kept. A thread without an id or a message is left out.
pub fn threads_of(
    page_artifacts: &Value,
    origin: &str,
    page: &str,
) -> Vec<(Thread, ElementDetails)> {
    let Some(all) = page_artifacts["comments"].as_array() else {
        return Vec::new();
    };
    all.iter()
        .filter_map(|thread| {
            let id = thread["id"].as_str().filter(|id| !id.is_empty())?;
            let messages: Vec<ThreadMessage> = (thread["messages"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    Some(ThreadMessage {
                        text: m["text"].as_str()?.to_string(),
                        created_at: m["createdAt"].as_f64().unwrap_or_default(),
                    })
                })
                .collect();
            if messages.is_empty() {
                return None;
            }
            let target = &thread["target"];
            let preview = (target["preview"] == true)
                .then(|| viewport_of(target))
                .flatten();
            let frames = strings(&target["framePath"]);
            Some((
                Thread {
                    id: id.to_string(),
                    messages,
                    status: if thread["status"] == "resolved" {
                        ThreadStatus::Resolved
                    } else {
                        ThreadStatus::Open
                    },
                    origin: origin.to_string(),
                    page: page.to_string(),
                    preview,
                    frames,
                    target: target["selector"].as_str().unwrap_or_default().to_string(),
                },
                element_of(target),
            ))
        })
        .collect()
}

/// A thread's element as mesurer captured it when the comment was left (`CommentTarget`): its
/// selector, tag, id and classes, text, box in the viewport, attributes and a few styles. Sawit's
/// page agent tells more of an element on the page (`details`); this is what there is of one in
/// a preview or a frame, which it doesn't reach.
pub fn element_of(target: &Value) -> ElementDetails {
    let tag = target["tagName"].as_str().unwrap_or("element").to_string();
    let attributes: Vec<(String, String)> = target["attributes"]
        .as_object()
        .map(|all| {
            all.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let attribute = |name: &str| {
        attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    let id = attribute("id").filter(|id| !id.is_empty());
    let classes: Vec<String> = attribute("class")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut label = tag.clone();
    if let Some(id) = &id {
        label.push('#');
        label.push_str(id);
    }
    for class in classes.iter().take(2) {
        label.push('.');
        label.push_str(class);
    }
    let styles = styles_of(target["styles"].as_str().unwrap_or_default());
    let rect = &target["rect"];
    let number = |v: &Value| v.as_f64().unwrap_or_default();
    // In a preview, mesurer's box is on the page around it, scaled: the element's own size is in
    // its styles, and where it is in the preview isn't kept.
    let rect = if target["preview"] == true {
        let px = |name: &str| {
            (styles.iter())
                .find(|(k, _)| k == name)
                .and_then(|(_, v)| v.trim_end_matches("px").trim().parse::<f64>().ok())
                .unwrap_or_default()
        };
        Rect {
            x: 0.,
            y: 0.,
            width: px("width"),
            height: px("height"),
        }
    } else {
        Rect {
            x: number(&rect["left"]),
            y: number(&rect["top"]),
            width: number(&rect["width"]),
            height: number(&rect["height"]),
        }
    };
    ElementDetails {
        selector: target["selector"].as_str().unwrap_or_default().to_string(),
        label,
        tag,
        id,
        classes,
        text: target["textSnippet"]
            .as_str()
            .map(str::to_string)
            .filter(|t| !t.trim().is_empty()),
        rect,
        styles,
        attributes,
        viewport: viewport_of(target),
        ..ElementDetails::default()
    }
}

/// mesurer's styles (`display: flex\nposition: static…`) as pairs.
fn styles_of(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            let (name, value) = (name.trim(), value.trim());
            (!name.is_empty() && !value.is_empty()).then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

/// The window a comment was left in (`{width, height}`), when it is known.
fn viewport_of(target: &Value) -> Option<(u32, u32)> {
    let size = |k: &str| {
        target["viewport"][k]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.)
            .map(|v| v.round() as u32)
    };
    Some((size("width")?, size("height")?))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
