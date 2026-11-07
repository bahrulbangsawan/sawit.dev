//! Where a file is in the design workflow (B12).

use figcraft_doc::{Document, NodeKind};
use serde_json::{Value, json};

use super::{color_tokens, is_screen};

/// Where a file is in the design workflow: what it has, the stage it is at, and the next step.
pub fn design_status(doc: &Document) -> Value {
    let colors = color_tokens(doc).len();
    let tokens = doc.variables.variables.len();
    let mut components = 0usize;
    let mut instances = 0usize;
    for node in doc.nodes() {
        match node.kind {
            NodeKind::Component { .. } => components += 1,
            NodeKind::Instance { .. } => instances += 1,
            _ => {}
        }
    }
    let mut screens = 0usize;
    let mut cover = false;
    for page in &doc.pages {
        for frame in doc.top_frames(*page) {
            let Some(node) = doc.node(frame) else {
                continue;
            };
            if node.name.to_lowercase().contains("cover") {
                cover = true;
            } else if is_screen(doc, frame) && !matches!(node.kind, NodeKind::Component { .. }) {
                screens += 1;
            }
        }
    }
    let (stage, next) = if colors == 0 {
        (
            "no system",
            "Start with the design system: colour tokens with light and dark, a type scale, spacing and radii as variables (variable.create), or lay out the starter system.",
        )
    } else if components == 0 {
        (
            "system",
            "Build the components (buttons, inputs, cards, navigation) from the tokens, with their variants and states side by side on a Components page (component.create).",
        )
    } else if screens == 0 {
        (
            "components",
            "Build each screen at a real device size from instances of the components, with auto layout (instance.create, layout.add).",
        )
    } else if !cover {
        (
            "screens",
            "Make the cover last: an artboard named Cover with the file's name, what it is, status and date, from the same tokens.",
        )
    } else {
        (
            "cover",
            "The workflow is done: run canvas_lint and fix its errors.",
        )
    };
    json!({
        "stage": stage, "next": next,
        "tokens": tokens, "color_tokens": colors, "components": components,
        "instances": instances, "screens": screens, "cover": cover,
    })
}
