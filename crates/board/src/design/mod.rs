//! The Design panel's model: what the selected layers hold, as the panel shows it, and the edits
//! the panel makes. It has no GPUI, so plain `cargo test` covers it; the views are in
//! `crates/app/src/board/design_panel/`.
//!
//! One key space serves reads and writes. [`DesignSelection::get`] answers under the keys that
//! `node.set` takes, and [`DesignSelection::set`] writes them back through `node.set`, so a value
//! read from one field can be handed to another layer's field as it is. `fields.rs` says how a
//! layer is read, and why some keys read in a shape other than `node_json`'s.

use std::collections::HashSet;

use figcraft_doc::{Document, NodeId, NodeKind};
use serde_json::{Map, Value, json};

pub mod export;
pub mod layout;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
pub mod typography;

mod fields;
mod lists;

pub mod effects;
pub mod paints;

pub use fields::{DEFAULTS, parse_number};

/// What the selected layers hold for one property. [`DesignSelection::get`] says which layers
/// count for a property.
#[derive(Clone, Debug, PartialEq)]
pub enum Mixed {
    /// None of the layers that count has the property: a font size among rectangles, a gap on a
    /// frame without auto layout. There is nothing for a field to show.
    Absent,
    /// Every layer that counts holds this value.
    Same(Value),
    /// The layers that count hold different values; for an auto layout key, a frame without auto
    /// layout is a different value from one that has it, and a text whose characters differ in a
    /// style, with two font sizes, holds several values by itself. The field shows "Mixed", and an
    /// edit sets the property on all of them.
    Mixed,
}

/// The panel's sections, in the order it shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// The kind and name of the layer, or the number of layers.
    Header,
    /// X, Y, W, H, rotation, the proportions lock, and align and distribute.
    Position,
    AutoLayout,
    /// Opacity, corner radius, blend mode and visibility.
    Appearance,
    Fill,
    Stroke,
    Effects,
    Typography,
    Export,
}

impl Section {
    /// Every section, in the order the panel shows them.
    pub const ALL: [Section; 9] = [
        Section::Header,
        Section::Position,
        Section::AutoLayout,
        Section::Appearance,
        Section::Fill,
        Section::Stroke,
        Section::Effects,
        Section::Typography,
        Section::Export,
    ];
}

/// An edit: the engine command to run and its params. Running it is one undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub command: &'static str,
    pub params: Value,
}

/// The layers the panel shows, and what each of them holds.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignSelection {
    /// The selected layers in the order they were given, each once, and only the layers the file
    /// has. Read-only: the rest of the model describes exactly these layers.
    pub ids: Vec<NodeId>,
    /// What each of `ids` holds, in the same order.
    layers: Vec<Layer>,
}

/// One selected layer.
#[derive(Clone, Debug, PartialEq)]
struct Layer {
    /// What the header calls the layer: "Rectangle".
    kind: &'static str,
    /// A frame, component, component set or instance: the layers that take auto layout.
    frame_like: bool,
    /// A group, which has no fill or stroke of its own.
    group: bool,
    text: bool,
    /// The layer's properties, by key ([`fields::layer_props`]).
    props: Map<String, Value>,
    /// The keys of `props` that the layer's own characters disagree on: a text with two font sizes
    /// holds no one size. Only a text has any ([`typography::fold_runs`]).
    mixed: Vec<&'static str>,
}

impl DesignSelection {
    /// What layers `ids` of `doc` hold. Ids the document doesn't have are left out, and so is an
    /// id that comes twice.
    pub fn read(doc: &Document, ids: &[NodeId]) -> Self {
        let mut seen = HashSet::new();
        let mut selection = Self {
            ids: Vec::new(),
            layers: Vec::new(),
        };
        for id in ids {
            if !seen.insert(*id) {
                continue;
            }
            let Some(node) = doc.node(*id) else {
                continue;
            };
            selection.ids.push(*id);
            let mut props = fields::layer_props(doc, node);
            let mixed = node
                .kind
                .text()
                .map(|text| typography::fold_runs(text, &mut props))
                .unwrap_or_default();
            selection.layers.push(Layer {
                kind: node.kind.label(),
                frame_like: node.kind.is_frame_like(),
                group: matches!(node.kind, NodeKind::Group),
                text: node.kind.text().is_some(),
                props,
                mixed,
            });
        }
        selection
    }

    /// What the layers hold for `key`, one of the keys `node.set` takes. Which layers count
    /// depends on the key:
    ///
    /// - The auto layout keys (`layout`, `gap`, `crossGap`, `padding` and its four sides,
    ///   `mainAlign`, `crossAlign` and `wrap`) count every frame, meaning a frame, component,
    ///   component set or instance. A frame without auto layout lacks them, and is a value of its
    ///   own: `node.set` of one of these keys gives such a frame an auto layout, so another
    ///   frame's value is not the selection's. The answer is [`Mixed::Mixed`] when the frames
    ///   disagree, [`Mixed::Absent`] when no frame has the key, and [`Mixed::Same`] only when every
    ///   frame holds the same value. A layer that is not a frame doesn't count: `node.set` skips it.
    /// - Every other key counts the layers that hold it. A rectangle selected with a text leaves
    ///   the font size to the text, and the answer is [`Mixed::Absent`] only when no layer has it.
    ///   A text whose characters differ in the key, in a run of another font size, is
    ///   [`Mixed::Mixed`] even alone, and whatever the other layers hold (`typography::fold_runs`
    ///   says which keys a run can change).
    pub fn get(&self, key: &str) -> Mixed {
        if fields::LAYOUT_KEYS.contains(&key) {
            return self.get_of_frames(key);
        }
        if self.layers.iter().any(|layer| layer.mixed.contains(&key)) {
            return Mixed::Mixed;
        }
        let mut held = self.layers.iter().filter_map(|layer| layer.props.get(key));
        let Some(first) = held.next() else {
            return Mixed::Absent;
        };
        if held.all(|value| value == first) {
            Mixed::Same(first.clone())
        } else {
            Mixed::Mixed
        }
    }

    /// [`get`](Self::get) for an auto layout key: every frame counts, and one that lacks the key
    /// is a different value from one that holds it.
    fn get_of_frames(&self, key: &str) -> Mixed {
        let mut frames = self.layers.iter().filter(|layer| layer.frame_like);
        let Some(first) = frames.next() else {
            return Mixed::Absent;
        };
        let first = first.props.get(key);
        let mut any_held = first.is_some();
        let mut all_equal = true;
        for frame in frames {
            let value = frame.props.get(key);
            any_held |= value.is_some();
            all_equal &= value == first;
        }
        match first {
            Some(value) if all_equal => Mixed::Same(value.clone()),
            None if !any_held => Mixed::Absent,
            _ => Mixed::Mixed,
        }
    }

    /// The sections to show for these layers, in the panel's order. Nothing is selected: none.
    ///
    /// - The header, position, appearance, effects and export sections are always there.
    /// - Auto layout, when every layer is a frame, component or instance.
    /// - Fill and stroke, unless every layer is a group.
    /// - Typography, when any layer is a text.
    pub fn sections(&self) -> Vec<Section> {
        if self.layers.is_empty() {
            return Vec::new();
        }
        let all_frames = self.layers.iter().all(|layer| layer.frame_like);
        let only_groups = self.layers.iter().all(|layer| layer.group);
        let any_text = self.layers.iter().any(|layer| layer.text);
        Section::ALL
            .into_iter()
            .filter(|section| match section {
                Section::AutoLayout => all_frames,
                Section::Fill | Section::Stroke => !only_groups,
                Section::Typography => any_text,
                _ => true,
            })
            .collect()
    }

    /// The header's text: the kind of the one layer ("Rectangle"), or how many layers there are
    /// ("3 layers"). Empty when nothing is selected.
    pub fn header(&self) -> String {
        match self.layers.as_slice() {
            [] => String::new(),
            [layer] => layer.kind.to_string(),
            layers => format!("{} layers", layers.len()),
        }
    }

    /// The edit that sets `key` to `value` on the layers: `node.set {ids, props: {key: value}}`.
    /// All the layers move together, as one undo step.
    ///
    /// - A text key (the typography fields) goes to the text layers only, because the engine
    ///   refuses it on any other layer and would refuse the whole edit.
    /// - An auto layout key (the ones [`get`](Self::get) counts every frame for) goes to every
    ///   layer, and the engine skips the layers that are not frames. Writing one of them gives a
    ///   frame that has no auto layout one (horizontal, the engine's default), so an edit over
    ///   frames of which only some have auto layout switches it on for the rest.
    pub fn set(&self, key: &str, value: Value) -> Edit {
        let ids: Vec<NodeId> = if fields::TEXT_KEYS.contains(&key) {
            let layers = self.ids.iter().zip(&self.layers);
            layers
                .filter(|(_, layer)| layer.text)
                .map(|(id, _)| *id)
                .collect()
        } else {
            self.ids.clone()
        };
        let mut props = Map::new();
        props.insert(key.to_string(), value);
        Edit {
            command: "node.set",
            params: json!({"ids": ids, "props": props}),
        }
    }
}

/// The edit of an align or distribute button, which works on the engine's selection: `command` is
/// one of `align.left`, `align.hcenter`, `align.right`, `align.top`, `align.vcenter`,
/// `align.bottom`, `distribute.horizontal` and `distribute.vertical`.
pub fn align(command: &'static str) -> Edit {
    Edit {
        command,
        params: json!({}),
    }
}
