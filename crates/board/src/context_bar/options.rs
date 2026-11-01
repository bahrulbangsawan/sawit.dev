//! The options of each control: the buttons the view draws, and the command each runs.

use figcraft_doc::{BoardShape, Routing};
use figcraft_engine::board::TEXT_SIZES;
use serde_json::{Value, json};

use super::{CAPS, Control, ControlOption};

/// How the view calls each of [`CAPS`], in the same order.
const CAP_LABELS: [&str; CAPS.len()] = [
    "None",
    "Line arrow",
    "Triangle arrow",
    "Reversed triangle",
    "Circle",
    "Diamond",
];

/// The ways a connector can run, in the order the bar offers them, and how the view calls each.
const ROUTINGS: [(Routing, &str); 3] = [
    (Routing::Straight, "Straight"),
    (Routing::Elbow, "Elbow"),
    (Routing::Curved, "Curved"),
];

/// An option that runs `board.setStyle` with `key` set to `value`.
fn style_option(key: &str, value: Value, label: &str, selected: bool) -> ControlOption {
    ControlOption {
        command: "board.setStyle",
        params: json!({ key: value }),
        label: label.to_owned(),
        swatch: None,
        selected,
    }
}

/// The options of a cap control: `key` (`startCap` or `endCap`) set to each of [`CAPS`].
fn cap_options(key: &str, current: Option<&str>) -> Vec<ControlOption> {
    CAPS.iter()
        .zip(CAP_LABELS)
        .map(|(cap, label)| style_option(key, json!(cap), label, Some(*cap) == current))
        .collect()
}

/// What `control` offers, a [`ControlOption`] for each button of the view, in order: the control's
/// own colours, [`BoardShape::ALL`], [`TEXT_SIZES`], straight, elbow and curved, or [`CAPS`].
///
/// The view draws these, and runs the command of the one picked with its params on the selection,
/// through `BoardView::choose`. Every option works on the whole selection, whether or not it is
/// the one selected, except a selected shape option: the engine refuses a shape that every
/// selected shape already has, so the view runs nothing for that one.
///
/// `selected` marks the option that is the control's current value. No option is selected when
/// the selected items differ, or have a value that the control has no option for.
pub fn options(control: &Control) -> Vec<ControlOption> {
    match control {
        Control::Colors { items, current } => items
            .iter()
            .map(|(name, rgba)| ControlOption {
                command: "board.setColor",
                params: json!({ "color": name }),
                label: (*name).to_owned(),
                swatch: Some(*rgba),
                selected: Some(*name) == *current,
            })
            .collect(),
        Control::Shape { current } => BoardShape::ALL
            .iter()
            .map(|shape| {
                let selected = Some(*shape) == *current;
                style_option("shape", json!(shape.id()), shape.label(), selected)
            })
            .collect(),
        // The engine takes the size, not its name.
        Control::FontSize { current } => TEXT_SIZES
            .iter()
            .map(|(name, size)| {
                style_option("fontSize", json!(size), name, Some(*name) == *current)
            })
            .collect(),
        Control::Routing { current } => ROUTINGS
            .iter()
            .map(|(routing, label)| {
                let selected = Some(*routing) == *current;
                style_option("style", json!(routing.id()), label, selected)
            })
            .collect(),
        Control::StartCap { current } => cap_options("startCap", current.as_deref()),
        Control::EndCap { current } => cap_options("endCap", current.as_deref()),
    }
}
