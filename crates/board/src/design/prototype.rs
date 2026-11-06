//! Prototyping: the Prototype tab of the Design panel, the connections drawn over the canvas, and
//! the player that presents the frames.
//!
//! - **The tab**: the one selected layer's interactions, each its trigger (click, hover, press,
//!   drag, after a delay…), its first action (navigate to a frame, back, open an overlay, close
//!   it, open a link) with its destination, and its transition and duration; buttons add one and
//!   remove one. The page's flows (named starting frames) are listed, and the selected top-level
//!   frame can start one. Each edit is one of the engine's `proto.*` commands, one undo step.
//! - **Connections** ([`links`]): a line from each layer with an interaction to the frame its
//!   first action goes to.
//! - **The player** ([`Player`]): a frame shown alone; a click on a layer that has an interaction
//!   on click runs its first action (navigate, back, open or close an overlay, a link), with the
//!   engine's own hotspots and destination rules (`figcraft_engine::proto_run`).

use std::collections::HashMap;

use figcraft_doc::proto::{Action, Trigger};
use figcraft_doc::{Document, NodeId, NodeKind};
use serde_json::{Value, json};

use super::Edit;

/// The triggers the tab offers, as `proto.interaction.*` takes them.
pub const TRIGGERS: [(&str, &str); 8] = [
    ("onClick", "On click"),
    ("onHover", "While hovering"),
    ("onPress", "While pressing"),
    ("onDrag", "On drag"),
    ("mouseEnter", "Mouse enter"),
    ("mouseLeave", "Mouse leave"),
    ("afterDelay", "After delay"),
    ("keyPress", "Key / gamepad"),
];

/// The actions the tab offers.
pub const ACTIONS: [(&str, &str); 6] = [
    ("navigate", "Navigate to"),
    ("back", "Back"),
    ("openOverlay", "Open overlay"),
    ("closeOverlay", "Close overlay"),
    ("scrollTo", "Scroll to"),
    ("openUrl", "Open link"),
];

/// The transitions the tab offers.
pub const TRANSITIONS: [(&str, &str); 7] = [
    ("instant", "Instant"),
    ("dissolve", "Dissolve"),
    ("smartAnimate", "Smart animate"),
    ("moveIn", "Move in"),
    ("moveOut", "Move out"),
    ("push", "Push"),
    ("slideIn", "Slide in"),
];

/// One interaction as the tab shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionRow {
    pub trigger: String,
    /// The first action's type, and the layer it goes to.
    pub action: String,
    pub dest: Option<NodeId>,
    /// An Open link action's address.
    pub url: Option<String>,
    pub transition: String,
    pub duration_ms: u32,
}

fn tag(value: &Value) -> String {
    value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Layer `id`'s interactions.
pub fn interactions(doc: &Document, id: NodeId) -> Vec<InteractionRow> {
    let Some(node) = doc.node(id) else {
        return Vec::new();
    };
    node.interactions
        .iter()
        .map(|i| {
            let trigger = serde_json::to_value(i.trigger.clone())
                .map(|v| tag(&v))
                .unwrap_or_default();
            let first = i.actions.first().and_then(|a| serde_json::to_value(a).ok());
            let action = first.as_ref().map(tag).unwrap_or_default();
            let dest = first
                .as_ref()
                .and_then(|a| a.get("dest"))
                .and_then(figcraft_engine::params::id_of);
            let url = first
                .as_ref()
                .and_then(|a| a.get("url"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let transition = serde_json::to_value(i.transition)
                .map(|v| tag(&v))
                .unwrap_or_default();
            InteractionRow {
                trigger,
                action,
                dest,
                url,
                transition,
                duration_ms: i.duration_ms,
            }
        })
        .collect()
}

/// The frames an action can go to: the top-level frames of `page`, by name.
pub fn destinations(doc: &Document, page: NodeId) -> Vec<(NodeId, String)> {
    doc.top_frames(page)
        .into_iter()
        .filter_map(|id| Some((id, doc.node(id)?.name.clone())))
        .collect()
}

/// The page's flows: each its starting frame and name.
pub fn flows(doc: &Document, page: NodeId) -> Vec<(NodeId, String)> {
    match doc.node(page).map(|n| &n.kind) {
        Some(NodeKind::Page { flows, .. }) => {
            flows.iter().map(|f| (f.start, f.name.clone())).collect()
        }
        _ => Vec::new(),
    }
}

/// The action JSON of type `action` going to `dest` (for the actions that go somewhere).
fn action_json(action: &str, dest: Option<NodeId>, url: Option<&str>) -> Option<Value> {
    Some(match action {
        "navigate" | "openOverlay" | "scrollTo" => json!({"type": action, "dest": dest?}),
        "openUrl" => json!({"type": "openUrl", "url": url.unwrap_or("https://")}),
        "back" | "closeOverlay" => json!({"type": action}),
        _ => return None,
    })
}

/// The edit that gives layer `id` an interaction: on click, navigate to `dest` (or back, without
/// one).
pub fn add(id: NodeId, dest: Option<NodeId>) -> Edit {
    let action = match dest {
        Some(dest) => json!({"type": "navigate", "dest": dest}),
        None => json!({"type": "back"}),
    };
    Edit {
        command: "proto.interaction.add",
        params: json!({"id": id, "trigger": {"type": "onClick"}, "action": action}),
    }
}

/// The edit that removes interaction `index` of layer `id`.
pub fn remove(id: NodeId, index: usize) -> Edit {
    Edit {
        command: "proto.interaction.remove",
        params: json!({"id": id, "index": index}),
    }
}

fn set(id: NodeId, index: usize, patch: Value) -> Edit {
    let mut params = json!({"id": id, "index": index});
    if let (Some(params), Some(patch)) = (params.as_object_mut(), patch.as_object()) {
        params.extend(patch.clone());
    }
    Edit {
        command: "proto.interaction.set",
        params,
    }
}

/// The edit that gives interaction `index` of layer `id` the trigger `trigger` (one of
/// [`TRIGGERS`]); a delay starts at 800 ms, a key at Space.
pub fn set_trigger(id: NodeId, index: usize, trigger: &str) -> Edit {
    let trigger = match trigger {
        "afterDelay" => json!({"type": "afterDelay", "ms": 800}),
        "keyPress" => json!({"type": "keyPress", "key": "Space"}),
        other => json!({"type": other}),
    };
    set(id, index, json!({"trigger": trigger}))
}

/// The edit that gives interaction `index` of layer `id` the first action `action` (one of
/// [`ACTIONS`]), going to `dest` for those that go somewhere (the first frame of the page when
/// none is given). Refused for an action that needs a destination and has none.
pub fn set_action(
    id: NodeId,
    index: usize,
    action: &str,
    dest: Option<NodeId>,
    url: Option<&str>,
) -> Option<Edit> {
    Some(set(
        id,
        index,
        json!({"action": action_json(action, dest, url)?}),
    ))
}

/// The edit that gives interaction `index` of layer `id` the transition `transition` (one of
/// [`TRANSITIONS`]).
pub fn set_transition(id: NodeId, index: usize, transition: &str) -> Edit {
    set(id, index, json!({"transition": {"type": transition}}))
}

/// The edit that makes interaction `index` of layer `id` last `ms` milliseconds (0 to 10000).
pub fn set_duration(id: NodeId, index: usize, ms: f64) -> Option<Edit> {
    (ms.is_finite() && (0.0..=10_000.0).contains(&ms))
        .then(|| set(id, index, json!({"durationMs": ms.round() as u32})))
}

/// The edit that makes top-level frame `start` a flow's starting point.
pub fn add_flow(start: NodeId) -> Edit {
    Edit {
        command: "proto.flow.add",
        params: json!({"start": start}),
    }
}

/// The edit that removes the flow starting at `start`.
pub fn remove_flow(start: NodeId) -> Edit {
    Edit {
        command: "proto.flow.remove",
        params: json!({"start": start}),
    }
}

/// A connection: from the layer with the interaction to the frame its first action goes to,
/// in page coordinates (each a box: x0, y0, x1, y1).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub from: NodeId,
    pub to: NodeId,
    pub from_box: [f64; 4],
    pub to_box: [f64; 4],
}

/// The connections on `page`.
pub fn links(doc: &Document, page: NodeId) -> Vec<Link> {
    let mut out = Vec::new();
    for node in doc.nodes() {
        if node.interactions.is_empty() || doc.page_of(node.id) != Some(page) {
            continue;
        }
        for interaction in &node.interactions {
            let dest = interaction.actions.iter().find_map(|a| match a {
                Action::Navigate { dest, .. }
                | Action::OpenOverlay { dest, .. }
                | Action::ScrollTo { dest, .. } => Some(*dest),
                _ => None,
            });
            let (Some(dest), Some(from), Some(to)) = (
                dest,
                doc.world_bounds(node.id),
                dest.and_then(|d| doc.world_bounds(d)),
            ) else {
                continue;
            };
            out.push(Link {
                from: node.id,
                to: dest,
                from_box: [from.x0, from.y0, from.x1, from.y1],
                to_box: [to.x0, to.y0, to.x1, to.y1],
            });
        }
    }
    out
}

/// The player: the frame on show, the frames before it (for Back), and an overlay over it.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub frame: NodeId,
    pub history: Vec<NodeId>,
    pub overlay: Option<NodeId>,
    /// The section-wise frames visited, which a Navigate to a section resolves against.
    visited: HashMap<NodeId, NodeId>,
}

/// What a click in the player did.
#[derive(Clone, Debug, PartialEq)]
pub enum Played {
    /// Nothing there reacts to a click.
    Nothing,
    /// Another frame shows, or an overlay opened or closed.
    Moved,
    /// A link to open in the browser.
    Url(String),
}

impl Player {
    /// The player at `start`, or the top-level frame `start` is in.
    pub fn new(doc: &Document, start: NodeId) -> Option<Player> {
        let frame = doc.top_level_frame(start)?;
        let mut visited = HashMap::new();
        figcraft_engine::proto_run::remember_sections(doc, frame, &mut visited);
        Some(Player {
            frame,
            history: Vec::new(),
            overlay: None,
            visited,
        })
    }

    /// Where the frame on show, and the overlay, react to a click: each layer with an interaction
    /// on click and its box in page coordinates, the overlay's first.
    pub fn hotspots(&self, doc: &Document) -> Vec<(NodeId, [f64; 4])> {
        let mut out = Vec::new();
        for root in self.overlay.into_iter().chain([self.frame]) {
            for (id, r) in figcraft_engine::proto_run::hotspots(doc, root) {
                let clicks = doc.node(id).is_some_and(|n| {
                    n.interactions.iter().any(|i| {
                        matches!(
                            i.trigger,
                            Trigger::OnClick
                                | Trigger::OnPress
                                | Trigger::MouseDown
                                | Trigger::MouseUp
                        )
                    })
                });
                if clicks {
                    out.push((id, [r.x0, r.y0, r.x1, r.y1]));
                }
            }
        }
        out
    }

    /// A click at page point (`x`, `y`): the first action of the top-most layer there that has
    /// one on click.
    pub fn click(&mut self, doc: &Document, x: f64, y: f64) -> Played {
        let hit = self
            .hotspots(doc)
            .into_iter()
            .rfind(|(_, b)| x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]);
        let Some((id, _)) = hit else {
            return Played::Nothing;
        };
        let Some(node) = doc.node(id) else {
            return Played::Nothing;
        };
        let actions: Vec<Action> = node
            .interactions
            .iter()
            .filter(|i| {
                matches!(
                    i.trigger,
                    Trigger::OnClick | Trigger::OnPress | Trigger::MouseDown | Trigger::MouseUp
                )
            })
            .flat_map(|i| i.actions.clone())
            .collect();
        for action in actions {
            match action {
                Action::Navigate { dest, .. } => {
                    let Some(to) =
                        figcraft_engine::proto_run::navigate_target(doc, dest, &self.visited)
                            .or_else(|| doc.top_level_frame(dest))
                    else {
                        continue;
                    };
                    self.history.push(self.frame);
                    self.frame = to;
                    self.overlay = None;
                    figcraft_engine::proto_run::remember_sections(doc, to, &mut self.visited);
                    return Played::Moved;
                }
                Action::Back => {
                    if let Some(back) = self.history.pop() {
                        self.frame = back;
                        self.overlay = None;
                        return Played::Moved;
                    }
                }
                Action::OpenOverlay { dest, .. } => {
                    self.overlay = Some(dest);
                    return Played::Moved;
                }
                Action::CloseOverlay => {
                    if self.overlay.take().is_some() {
                        return Played::Moved;
                    }
                }
                Action::OpenUrl { url, .. } => return Played::Url(url),
                _ => {}
            }
        }
        Played::Nothing
    }
}

#[cfg(test)]
mod tests;
