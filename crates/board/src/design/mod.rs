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

#[cfg(test)]
mod tests {
    use figcraft_doc::{Document, NodeId};
    use figcraft_engine::Session;
    use serde_json::{Value, json};

    use super::*;
    use crate::doc::{BoardDoc, BoardError};

    /// A design file with nothing on its page.
    fn design_file() -> BoardDoc {
        BoardDoc {
            session: Session::with_document(Document::new("Design")),
        }
    }

    /// Makes a layer with `node.create` and returns its id.
    fn make(board: &mut BoardDoc, params: Value) -> NodeId {
        let made = board.execute("node.create", &params).unwrap();
        NodeId(made["id"].as_u64().unwrap())
    }

    /// A 100 × 100 rectangle called `name` at (`x`, 0).
    fn rectangle(board: &mut BoardDoc, name: &str, x: f64) -> NodeId {
        let params =
            json!({"type": "rectangle", "name": name, "x": x, "y": 0, "width": 100, "height": 100});
        make(board, params)
    }

    /// The file the panel tests share: rectangles R1 (x 0) and R2 (x 50), and a text.
    struct Fixture {
        board: BoardDoc,
        r1: NodeId,
        r2: NodeId,
        text: NodeId,
    }

    fn fixture() -> Fixture {
        let mut board = design_file();
        let r1 = rectangle(&mut board, "R1", 0.0);
        let r2 = rectangle(&mut board, "R2", 50.0);
        let text = make(
            &mut board,
            json!({"type": "text", "name": "Title", "text": "Hello", "x": 0, "y": 200}),
        );
        Fixture {
            board,
            r1,
            r2,
            text,
        }
    }

    fn read(board: &BoardDoc, ids: &[NodeId]) -> DesignSelection {
        DesignSelection::read(&board.doc().unwrap(), ids)
    }

    /// Runs `edit` as the panel does.
    fn apply(board: &mut BoardDoc, edit: &Edit) {
        board.execute(edit.command, &edit.params).unwrap();
    }

    /// `node.set` on `ids`, without the panel's model.
    fn node_set(board: &mut BoardDoc, ids: &[NodeId], props: Value) {
        board
            .execute("node.set", &json!({"ids": ids, "props": props}))
            .unwrap();
    }

    fn same(value: Value) -> Mixed {
        Mixed::Same(value)
    }

    #[test]
    fn same_values_read_same() {
        let f = fixture();
        let sel = read(&f.board, &[f.r1, f.r2]);
        assert_eq!(sel.get("width"), same(json!(100.0)));
        assert_eq!(sel.get("height"), same(json!(100.0)));
        assert_eq!(sel.get("y"), same(json!(0.0)));
        assert_eq!(sel.get("name"), Mixed::Mixed);
        assert_eq!(read(&f.board, &[f.r1]).get("name"), same(json!("R1")));
    }

    #[test]
    fn different_values_read_mixed() {
        let f = fixture();
        let sel = read(&f.board, &[f.r1, f.r2]);
        assert_eq!(sel.get("x"), Mixed::Mixed);
        assert_eq!(read(&f.board, &[f.r1]).get("x"), same(json!(0.0)));
        assert_eq!(read(&f.board, &[f.r2]).get("x"), same(json!(50.0)));
    }

    #[test]
    fn defaults_fill_omitted_keys() {
        let mut f = fixture();
        let sel = read(&f.board, &[f.r1, f.r2]);
        assert_eq!(sel.get("visible"), same(json!(true)));
        assert_eq!(sel.get("opacity"), same(json!(1.0)));
        assert_eq!(sel.get("locked"), same(json!(false)));
        assert_eq!(sel.get("rotation"), same(json!(0.0)));
        assert_eq!(sel.get("cornerRadius"), same(json!(0.0)));

        // A default never hides a value the layer really has.
        node_set(
            &mut f.board,
            &[f.r1],
            json!({"visible": false, "opacity": 0.5}),
        );
        let sel = read(&f.board, &[f.r1, f.r2]);
        assert_eq!(sel.get("visible"), Mixed::Mixed);
        assert_eq!(sel.get("opacity"), Mixed::Mixed);
        assert_eq!(read(&f.board, &[f.r1]).get("visible"), same(json!(false)));
        assert_eq!(read(&f.board, &[f.r1]).get("opacity"), same(json!(0.5)));
    }

    #[test]
    fn empty_lists_read_as_empty_lists_not_as_absent() {
        let mut f = fixture();
        // A rectangle starts with a fill, and with no stroke, effect or export setting.
        let sel = read(&f.board, &[f.r1]);
        for key in ["strokes", "effects", "exports", "dashes"] {
            assert_eq!(sel.get(key), same(json!([])), "{key}");
        }
        assert!(matches!(sel.get("fills"), Mixed::Same(Value::Array(a)) if a.len() == 1));

        // Without its fill R2 reads an empty list too, and differs from R1.
        node_set(&mut f.board, &[f.r2], json!({"fills": []}));
        assert_eq!(read(&f.board, &[f.r2]).get("fills"), same(json!([])));
        assert_eq!(read(&f.board, &[f.r1, f.r2]).get("fills"), Mixed::Mixed);
    }

    #[test]
    fn the_blend_is_the_layers_own() {
        let mut f = fixture();
        let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
        // A rectangle is Normal and a frame passes through, and the engine's inspector leaves
        // both out.
        assert_eq!(read(&f.board, &[f.r1]).get("blend"), same(json!("normal")));
        assert_eq!(
            read(&f.board, &[frame]).get("blend"),
            same(json!("passThrough"))
        );
        assert_eq!(read(&f.board, &[f.r1, frame]).get("blend"), Mixed::Mixed);

        // A frame set to Normal is not the same as one that passes through.
        let edit = read(&f.board, &[frame]).set("blend", json!("normal"));
        apply(&mut f.board, &edit);
        assert_eq!(read(&f.board, &[frame]).get("blend"), same(json!("normal")));
        let edit = read(&f.board, &[f.r1, frame]).set("blend", json!("multiply"));
        apply(&mut f.board, &edit);
        assert_eq!(
            read(&f.board, &[f.r1, frame]).get("blend"),
            same(json!("multiply"))
        );
    }

    #[test]
    fn numbers_read_as_floats() {
        let mut f = fixture();
        let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
        make(
            &mut f.board,
            json!({"type": "rectangle", "parent": frame, "width": 10, "height": 10}),
        );
        // Even a count the engine reports as an integer: the values compare by variant.
        assert_eq!(read(&f.board, &[frame]).get("childCount"), same(json!(1.0)));
    }

    #[test]
    fn every_default_is_valid_json() {
        for (key, json) in DEFAULTS {
            assert!(serde_json::from_str::<Value>(json).is_ok(), "{key}: {json}");
        }
    }

    #[test]
    fn lists_and_objects_compare_whole() {
        let mut f = fixture();
        // Both rectangles start with the same fill.
        let sel = read(&f.board, &[f.r1, f.r2]);
        assert!(matches!(sel.get("fills"), Mixed::Same(Value::Array(a)) if a.len() == 1));
        // A fill of another colour, or of another kind, is another value.
        node_set(&mut f.board, &[f.r2], json!({"fill": "#FF0000"}));
        assert_eq!(read(&f.board, &[f.r1, f.r2]).get("fills"), Mixed::Mixed);
        node_set(&mut f.board, &[f.r1], json!({"fill": "#FF0000"}));
        let red = read(&f.board, &[f.r1, f.r2]).get("fills");
        assert_eq!(red, same(json!([{"type": "solid", "color": "#FF0000"}])));
    }

    #[test]
    fn editing_mixed_x_moves_all_in_one_step() {
        let mut f = fixture();
        let sel = read(&f.board, &[f.r1, f.r2]);
        let steps = f.board.session.undo_depth().0;

        let edit = sel.set("x", json!(10.0));
        apply(&mut f.board, &edit);
        assert_eq!(f.board.session.undo_depth().0, steps + 1);
        assert_eq!(
            read(&f.board, &[f.r1, f.r2]).get("x"),
            same(json!(10.0)),
            "both layers moved"
        );

        f.board.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(f.board.session.undo_depth().0, steps);
        assert_eq!(read(&f.board, &[f.r1]).get("x"), same(json!(0.0)));
        assert_eq!(read(&f.board, &[f.r2]).get("x"), same(json!(50.0)));
    }

    #[test]
    fn an_edit_is_node_set_on_the_selected_layers() {
        let f = fixture();
        let sel = read(&f.board, &[f.r1, f.r2]);
        assert_eq!(
            sel.set("opacity", json!(0.5)),
            Edit {
                command: "node.set",
                params: json!({"ids": [f.r1, f.r2], "props": {"opacity": 0.5}}),
            }
        );
    }

    #[test]
    fn parse_rejects_bad_numbers() {
        assert_eq!(parse_number("width", "abc"), None);
        assert_eq!(parse_number("width", "-5"), None);
        assert_eq!(parse_number("x", "1e400"), None);
        assert_eq!(parse_number("x", ""), None);
        assert_eq!(parse_number("x", "-5"), Some(-5.0));
        assert_eq!(parse_number("opacity", "50%"), Some(0.5));
        assert_eq!(parse_number("opacity", "3"), Some(1.0));
        assert_eq!(parse_number("paddingTop", "-5"), None);
    }

    #[test]
    fn parse_refuses_what_is_not_a_finite_number() {
        for input in [
            "NaN", "inf", "-inf", "Infinity", "  ", "1,5", "12px", "0x10", "--3",
        ] {
            assert_eq!(parse_number("x", input), None, "{input:?}");
        }
        assert_eq!(parse_number("opacity", "%"), None);
        assert_eq!(parse_number("opacity", "abc%"), None);
        assert_eq!(parse_number("opacity", "1e400%"), None);
    }

    #[test]
    fn parse_trims_and_reads_plain_numbers() {
        assert_eq!(parse_number("x", " 12 "), Some(12.0));
        assert_eq!(parse_number("y", "\t-3.5\n"), Some(-3.5));
        assert_eq!(parse_number("rotation", "-90"), Some(-90.0));
        assert_eq!(parse_number("width", "0"), Some(0.0));
        assert_eq!(parse_number("width", "1e2"), Some(100.0));
        // A percent sign belongs to opacity alone.
        assert_eq!(parse_number("width", "50%"), None);
        assert_eq!(parse_number("x", "50%"), None);
    }

    #[test]
    fn opacity_is_a_fraction_or_a_percent_kept_between_0_and_1() {
        assert_eq!(parse_number("opacity", "0.25"), Some(0.25));
        assert_eq!(parse_number("opacity", " 50 % "), Some(0.5));
        assert_eq!(parse_number("opacity", "100%"), Some(1.0));
        assert_eq!(parse_number("opacity", "150%"), Some(1.0));
        assert_eq!(parse_number("opacity", "-0.5"), Some(0.0));
        assert_eq!(parse_number("opacity", "-20%"), Some(0.0));
    }

    #[test]
    fn only_sizes_gaps_paddings_and_radii_refuse_a_negative() {
        for key in [
            "width",
            "height",
            "strokeWeight",
            "fontSize",
            "gap",
            "crossGap",
            "padding",
            "paddingTop",
            "paddingRight",
            "paddingBottom",
            "paddingLeft",
            "cornerRadius",
            "topLeftRadius",
            "topRightRadius",
            "bottomRightRadius",
            "bottomLeftRadius",
        ] {
            assert_eq!(parse_number(key, "-1"), None, "{key}");
            assert_eq!(parse_number(key, "1"), Some(1.0), "{key}");
            assert_eq!(parse_number(key, "0"), Some(0.0), "{key}");
        }
        for key in ["x", "y", "rotation"] {
            assert_eq!(parse_number(key, "-1"), Some(-1.0), "{key}");
        }
        // Minus zero is zero.
        for key in ["width", "x", "opacity"] {
            let zero = parse_number(key, "-0").unwrap();
            assert!(zero == 0.0 && zero.is_sign_positive(), "{key}");
        }
    }

    #[test]
    fn sections_by_kind() {
        let mut f = fixture();
        let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
        let others = [
            rectangle(&mut f.board, "A", 300.0),
            rectangle(&mut f.board, "B", 400.0),
        ];
        let group = f
            .board
            .execute("object.group", &json!({"ids": others}))
            .unwrap();
        let group = NodeId(group["id"].as_u64().unwrap());

        let rects = read(&f.board, &[f.r1, f.r2]).sections();
        assert!(!rects.contains(&Section::Typography));
        assert!(!rects.contains(&Section::AutoLayout));

        let text = read(&f.board, &[f.text]).sections();
        assert!(text.contains(&Section::Typography));
        // A text that sits among rectangles still brings its Typography along.
        let both = read(&f.board, &[f.r1, f.text]).sections();
        assert!(both.contains(&Section::Typography));

        assert!(
            read(&f.board, &[frame])
                .sections()
                .contains(&Section::AutoLayout)
        );
        // Auto layout needs every layer to be a frame.
        let mixed = read(&f.board, &[frame, f.r1]).sections();
        assert!(!mixed.contains(&Section::AutoLayout));

        // A group has no fill or stroke of its own, unless something else is selected too.
        let groups = read(&f.board, &[group]).sections();
        assert!(!groups.contains(&Section::Fill) && !groups.contains(&Section::Stroke));
        let groups_and_a_rectangle = read(&f.board, &[group, f.r1]).sections();
        assert!(groups_and_a_rectangle.contains(&Section::Fill));
        assert!(groups_and_a_rectangle.contains(&Section::Stroke));
    }

    #[test]
    fn sections_come_in_the_panels_order() {
        use Section::*;
        let mut f = fixture();
        let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
        assert_eq!(
            read(&f.board, &[f.r1]).sections(),
            [Header, Position, Appearance, Fill, Stroke, Effects, Export]
        );
        assert_eq!(
            read(&f.board, &[frame]).sections(),
            [
                Header, Position, AutoLayout, Appearance, Fill, Stroke, Effects, Export
            ]
        );
        assert_eq!(
            read(&f.board, &[f.text]).sections(),
            [
                Header, Position, Appearance, Fill, Stroke, Effects, Typography, Export
            ]
        );
    }

    #[test]
    fn header_names_kind_or_count() {
        let f = fixture();
        assert_eq!(read(&f.board, &[f.r1]).header(), "Rectangle");
        assert_eq!(read(&f.board, &[f.text]).header(), "Text");
        assert_eq!(read(&f.board, &[f.r1, f.r2]).header(), "2 layers");
        assert_eq!(read(&f.board, &[f.r1, f.r2, f.text]).header(), "3 layers");
    }

    #[test]
    fn nothing_selected_shows_nothing() {
        let f = fixture();
        let sel = read(&f.board, &[]);
        assert!(sel.ids.is_empty());
        assert!(sel.sections().is_empty());
        assert_eq!(sel.header(), "");
        assert_eq!(sel.get("x"), Mixed::Absent);
        assert_eq!(sel.get("visible"), Mixed::Absent);
    }

    #[test]
    fn layers_the_file_lacks_and_repeats_are_left_out() {
        let f = fixture();
        let sel = read(&f.board, &[f.r1, NodeId(9_999), f.r1, f.r2]);
        assert_eq!(sel.ids, [f.r1, f.r2]);
        assert_eq!(sel.header(), "2 layers");
        assert_eq!(
            sel.set("opacity", json!(0.5)).params["ids"],
            json!([f.r1, f.r2])
        );
    }

    #[test]
    fn align_left_runs() {
        let mut f = fixture();
        apply(
            &mut f.board,
            &Edit {
                command: "select.set",
                params: json!({"ids": [f.r1, f.r2]}),
            },
        );
        let edit = align("align.left");
        assert_eq!(edit.command, "align.left");
        apply(&mut f.board, &edit);
        let sel = read(&f.board, &[f.r1, f.r2]);
        assert_eq!(sel.get("x"), same(json!(0.0)), "both left edges are one");
    }

    #[test]
    fn align_and_distribute_carry_no_ids() {
        for command in [
            "align.left",
            "align.hcenter",
            "align.right",
            "align.top",
            "align.vcenter",
            "align.bottom",
            "distribute.horizontal",
            "distribute.vertical",
        ] {
            assert_eq!(
                align(command),
                Edit {
                    command,
                    params: json!({}),
                }
            );
        }
    }

    /// One key space: a key is read, written and read again under the same name, whatever shape
    /// `node_json` gives it (nested layout, sizing and corner values, or not given at all).
    #[test]
    fn flattened_keys_read_back_what_node_set_wrote() {
        let mut f = fixture();
        let frame = make(
            &mut f.board,
            json!({"type": "frame", "name": "F", "width": 200, "height": 100}),
        );
        let child = make(
            &mut f.board,
            json!({"type": "rectangle", "parent": frame, "width": 10, "height": 10}),
        );

        // Without auto layout the frame has none of its keys; every layer has a corner and a lock.
        let sel = read(&f.board, &[frame]);
        for key in [
            "layout",
            "gap",
            "crossGap",
            "padding",
            "paddingTop",
            "paddingRight",
            "paddingBottom",
            "paddingLeft",
            "mainAlign",
            "crossAlign",
            "wrap",
            "sizingH",
            "sizingV",
        ] {
            assert_eq!(sel.get(key), Mixed::Absent, "{key}");
        }
        assert_eq!(sel.get("constrainProportions"), same(json!(false)));
        for key in [
            "topLeftRadius",
            "topRightRadius",
            "bottomRightRadius",
            "bottomLeftRadius",
        ] {
            assert_eq!(sel.get(key), same(json!(0.0)), "{key}");
        }

        let writes = [
            ("layout", json!("vertical"), json!("vertical")),
            ("gap", json!(12.0), json!(12.0)),
            ("crossGap", json!(7.0), json!(7.0)),
            ("paddingTop", json!(1.0), json!(1.0)),
            ("paddingRight", json!(2.0), json!(2.0)),
            ("paddingBottom", json!(3.0), json!(3.0)),
            ("paddingLeft", json!(4.0), json!(4.0)),
            ("mainAlign", json!("center"), json!("center")),
            ("crossAlign", json!("max"), json!("max")),
            ("wrap", json!(true), json!(true)),
            ("sizingH", json!("hug"), json!("hug")),
            ("topLeftRadius", json!(5.0), json!(5.0)),
            ("topRightRadius", json!(6.0), json!(6.0)),
            ("bottomRightRadius", json!(7.0), json!(7.0)),
            ("bottomLeftRadius", json!(8.0), json!(8.0)),
            ("constrainProportions", json!(true), json!(true)),
        ];
        for (key, write, back) in writes {
            let edit = read(&f.board, &[frame]).set(key, write);
            apply(&mut f.board, &edit);
            assert_eq!(read(&f.board, &[frame]).get(key), same(back), "{key}");
        }
        let sel = read(&f.board, &[frame]);
        assert_eq!(sel.get("sizingV"), same(json!("fixed")));
        // The sides that differ are one list, the way `node.set` takes them.
        assert_eq!(sel.get("padding"), same(json!([1.0, 2.0, 3.0, 4.0])));
        assert_eq!(sel.get("cornerRadius"), same(json!([5.0, 6.0, 7.0, 8.0])));

        // The sides that agree are one number.
        let edit = sel.set("padding", json!(8.0));
        apply(&mut f.board, &edit);
        let edit = sel.set("cornerRadius", json!(6.0));
        apply(&mut f.board, &edit);
        let sel = read(&f.board, &[frame]);
        assert_eq!(sel.get("padding"), same(json!(8.0)));
        assert_eq!(sel.get("cornerRadius"), same(json!(6.0)));
        for key in ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"] {
            assert_eq!(sel.get(key), same(json!(8.0)), "{key}");
        }
        for key in [
            "topLeftRadius",
            "topRightRadius",
            "bottomRightRadius",
            "bottomLeftRadius",
        ] {
            assert_eq!(sel.get(key), same(json!(6.0)), "{key}");
        }

        // A child of an auto layout frame has a sizing of its own.
        assert_eq!(
            read(&f.board, &[child]).get("sizingV"),
            same(json!("fixed"))
        );
        let edit = read(&f.board, &[child]).set("sizingV", json!("fill"));
        apply(&mut f.board, &edit);
        assert_eq!(read(&f.board, &[child]).get("sizingV"), same(json!("fill")));
        // ... and a rectangle outside one has none.
        assert_eq!(read(&f.board, &[f.r1]).get("sizingH"), Mixed::Absent);

        // Only a text has a vertical alignment.
        assert_eq!(
            read(&f.board, &[f.r1]).get("textAlignVertical"),
            Mixed::Absent
        );
        assert_eq!(
            read(&f.board, &[f.text]).get("textAlignVertical"),
            same(json!("top"))
        );
        let edit = read(&f.board, &[f.text]).set("textAlignVertical", json!("middle"));
        apply(&mut f.board, &edit);
        assert_eq!(
            read(&f.board, &[f.text]).get("textAlignVertical"),
            same(json!("middle"))
        );
    }

    #[test]
    fn layers_without_a_key_do_not_count_for_it() {
        let f = fixture();
        let size = read(&f.board, &[f.text]).get("fontSize");
        assert!(matches!(size, Mixed::Same(_)), "{size:?}");
        // A text and a rectangle: the rectangle has no font size to disagree about.
        assert_eq!(read(&f.board, &[f.r1, f.text]).get("fontSize"), size);
        // Rectangles alone have none at all.
        assert_eq!(read(&f.board, &[f.r1, f.r2]).get("fontSize"), Mixed::Absent);
        assert_eq!(read(&f.board, &[f.r1]).get("characters"), Mixed::Absent);
    }

    /// A 200 × 100 frame called `name` at (`x`, 400), given a vertical auto layout with a gap of 12
    /// when `auto_layout` is set.
    fn frame_at(board: &mut BoardDoc, name: &str, x: f64, auto_layout: bool) -> NodeId {
        let params =
            json!({"type": "frame", "name": name, "x": x, "y": 400, "width": 200, "height": 100});
        let id = make(board, params);
        if auto_layout {
            node_set(board, &[id], json!({"layout": "vertical", "gap": 12.0}));
        }
        id
    }

    /// The auto layout keys, listed by hand: what the panel has to treat as a frame's own.
    const AUTO_LAYOUT_KEYS: [&str; 11] = [
        "layout",
        "gap",
        "crossGap",
        "padding",
        "paddingTop",
        "paddingRight",
        "paddingBottom",
        "paddingLeft",
        "mainAlign",
        "crossAlign",
        "wrap",
    ];

    /// A frame without auto layout lacks every auto layout key, yet `node.set` of one gives it an
    /// auto layout. So next to a frame that has them it is a value of its own: the other frame's
    /// value is not the selection's, and a field that showed it as the selection's would, once
    /// committed, change this frame.
    #[test]
    fn a_frame_without_auto_layout_makes_layout_keys_mixed() {
        let mut f = fixture();
        let with = frame_at(&mut f.board, "A", 0.0, true);
        let without = frame_at(&mut f.board, "B", 300.0, false);
        let also_without = frame_at(&mut f.board, "C", 600.0, false);

        // The frame with auto layout holds every key.
        let a = read(&f.board, &[with]);
        assert_eq!(a.get("layout"), same(json!("vertical")));
        assert_eq!(a.get("gap"), same(json!(12.0)));
        for key in AUTO_LAYOUT_KEYS {
            assert!(matches!(a.get(key), Mixed::Same(_)), "A holds {key}");
        }

        // The one without holds none of them: there is nothing to show for it alone.
        let b = read(&f.board, &[without]);
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(b.get(key), Mixed::Absent, "B alone: {key}");
        }
        let both_without = read(&f.board, &[without, also_without]);
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(both_without.get(key), Mixed::Absent, "B and C: {key}");
        }

        // Together they disagree, whichever comes first.
        for ids in [[with, without], [without, with]] {
            let both = read(&f.board, &ids);
            assert_eq!(both.get("layout"), Mixed::Mixed, "{ids:?}");
            assert_eq!(both.get("gap"), Mixed::Mixed, "{ids:?}");
            for key in AUTO_LAYOUT_KEYS {
                assert_eq!(both.get(key), Mixed::Mixed, "{ids:?}: {key}");
            }
        }
        let three = read(&f.board, &[without, with, also_without]);
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(three.get(key), Mixed::Mixed, "A, B and C: {key}");
        }

        // A rectangle is no frame, so it doesn't count: A with a rectangle reads as A does.
        let with_rectangle = read(&f.board, &[with, f.r1]);
        assert_eq!(with_rectangle.get("gap"), same(json!(12.0)));
    }

    /// `node.set` skips the layers that are not frames when it writes an auto layout key, so they
    /// don't count for one.
    #[test]
    fn layers_that_are_not_frames_do_not_count_for_layout_keys() {
        let mut f = fixture();
        let with = frame_at(&mut f.board, "A", 0.0, true);
        let without = frame_at(&mut f.board, "B", 300.0, false);

        // A rectangle beside a frame with auto layout leaves the answer to the frame.
        let with_rectangle = read(&f.board, &[with, f.r1]);
        assert_eq!(with_rectangle.get("gap"), same(json!(12.0)));
        assert_eq!(with_rectangle.get("layout"), same(json!("vertical")));
        for key in AUTO_LAYOUT_KEYS {
            let alone = read(&f.board, &[with]).get(key);
            assert_eq!(with_rectangle.get(key), alone, "A and a rectangle: {key}");
        }

        // Beside a frame without auto layout they add nothing: still nothing to show.
        let without_others = read(&f.board, &[without, f.r1, f.text]);
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(
                without_others.get(key),
                Mixed::Absent,
                "B and others: {key}"
            );
        }
        // With no frame at all there is nothing to count.
        let no_frame = read(&f.board, &[f.r1, f.text]);
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(no_frame.get(key), Mixed::Absent, "no frame: {key}");
        }

        // The rectangle doesn't hide that the frames differ.
        let all = read(&f.board, &[f.r1, with, without]);
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(all.get(key), Mixed::Mixed, "{key}");
        }
    }

    /// The frames all have auto layout: the keys read as they do for any other property.
    #[test]
    fn frames_that_all_have_auto_layout_read_as_they_hold() {
        let mut f = fixture();
        let a = frame_at(&mut f.board, "A", 0.0, true);
        let b = frame_at(&mut f.board, "B", 300.0, true);

        let both = read(&f.board, &[a, b]);
        assert_eq!(both.get("layout"), same(json!("vertical")));
        assert_eq!(both.get("gap"), same(json!(12.0)));
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(both.get(key), read(&f.board, &[a]).get(key), "{key}");
        }

        node_set(&mut f.board, &[b], json!({"gap": 4.0}));
        let both = read(&f.board, &[a, b]);
        assert_eq!(both.get("gap"), Mixed::Mixed);
        assert_eq!(both.get("layout"), same(json!("vertical")));
        assert_eq!(both.get("wrap"), same(json!(false)));
    }

    /// What `set` says about the auto layout keys, as the engine does it: the frame without auto
    /// layout gets one, horizontal because that is the engine's default, and a rectangle in the
    /// selection is skipped, not refused.
    #[test]
    fn writing_a_layout_key_gives_a_frame_without_auto_layout_one() {
        let mut f = fixture();
        let with = frame_at(&mut f.board, "A", 0.0, true);
        let without = frame_at(&mut f.board, "B", 300.0, false);
        assert_eq!(read(&f.board, &[without]).get("layout"), Mixed::Absent);
        let rectangle = read(&f.board, &[f.r1]);

        let edit = read(&f.board, &[with, without, f.r1]).set("gap", json!(12.0));
        assert_eq!(edit.params["ids"], json!([with, without, f.r1]));
        apply(&mut f.board, &edit);

        let after = read(&f.board, &[without]);
        assert_eq!(after.get("layout"), same(json!("horizontal")));
        assert_eq!(after.get("gap"), same(json!(12.0)));
        assert_eq!(
            read(&f.board, &[with]).get("layout"),
            same(json!("vertical")),
            "the frame that had one keeps its direction"
        );
        assert_eq!(read(&f.board, &[f.r1]), rectangle);
    }

    /// `fields::LAYOUT_KEYS` is what `get` counts every frame for, so it must be the keys a layer
    /// gains when it gets an auto layout, and no more or fewer: those the layout lifts, and the
    /// sizing that comes with it.
    #[test]
    fn layout_keys_are_the_keys_an_auto_layout_adds_to_a_frame() {
        let mut f = fixture();
        let id = frame_at(&mut f.board, "F", 0.0, false);
        let keys = |board: &BoardDoc| -> Vec<String> {
            let selection = read(board, &[id]);
            selection.layers[0].props.keys().cloned().collect()
        };
        let before = keys(&f.board);
        node_set(&mut f.board, &[id], json!({"layout": "horizontal"}));
        let after = keys(&f.board);

        let mut added: Vec<&str> = after
            .iter()
            .filter(|key| !before.contains(key))
            .map(String::as_str)
            .collect();
        added.sort_unstable();
        let mut expected: Vec<&str> = fields::LAYOUT_KEYS.to_vec();
        expected.extend(["sizingH", "sizingV"]);
        expected.sort_unstable();
        assert_eq!(added, expected);
        // The list the tests above go by is the same one.
        let mut by_hand = AUTO_LAYOUT_KEYS.to_vec();
        by_hand.sort_unstable();
        let mut named = fields::LAYOUT_KEYS.to_vec();
        named.sort_unstable();
        assert_eq!(named, by_hand);
    }

    #[test]
    fn a_text_key_goes_to_the_text_layers_only() {
        let mut f = fixture();
        let sel = read(&f.board, &[f.r1, f.text]);
        let keys = [
            "characters",
            "fontFamily",
            "fontStyle",
            "fontSize",
            "lineHeight",
            "letterSpacing",
            "textAlign",
            "textAlignVertical",
            "textAutoResize",
        ];
        for key in keys {
            assert_eq!(
                sel.set(key, json!("x")).params["ids"],
                json!([f.text]),
                "{key}"
            );
        }
        // Every other key still goes to every layer.
        for key in [
            "x",
            "opacity",
            "fills",
            "width",
            "constrainProportions",
            "gap",
        ] {
            assert_eq!(
                sel.set(key, json!(1.0)).params["ids"],
                json!([f.r1, f.text]),
                "{key}"
            );
        }

        // So the engine, which refuses a text key on a rectangle, takes the edit, and the
        // rectangle stays as it was.
        let before = read(&f.board, &[f.r1]);
        let edit = sel.set("fontSize", json!(18.0));
        apply(&mut f.board, &edit);
        assert_eq!(read(&f.board, &[f.text]).get("fontSize"), same(json!(18.0)));
        assert_eq!(read(&f.board, &[f.r1]), before);
    }

    #[test]
    fn a_text_key_without_a_text_layer_is_refused_and_changes_nothing() {
        let mut f = fixture();
        let sel = read(&f.board, &[f.r1, f.r2]);
        let edit = sel.set("fontSize", json!(18.0));
        assert_eq!(edit.params["ids"], json!([]));

        let revision = f.board.revision();
        let refused = f.board.execute(edit.command, &edit.params);
        assert!(matches!(refused, Err(BoardError::Engine(_))), "{refused:?}");
        assert_eq!(f.board.revision(), revision);
        assert_eq!(read(&f.board, &[f.r1, f.r2]), sel);
    }
}
