//! Nodes and clusters: each outline a board shape, with its label and the room the label needs.

use figcraft_doc::BoardShape;
use kurbo::Rect;
use roxmltree::Node;

use super::geometry::{holds_middle, shape_box};
use super::shapes::{container_kind, invisible, is_rounded};
use super::style::{has_class, is_icon, is_shape, is_turned, paint, visible};
use super::text::{font_size, set_widest_line, set_widest_word, text_of, text_place};
use super::{
    Converter, LABEL_LINE_HEIGHT, ONE_LINE_GROWTH, SURFACE_ALT_FILL, TEXT_PAD, TRANSPARENT, Target,
};
use crate::skeleton::{Label, Skeleton};

impl<'a, 'input> Converter<'a, 'input> {
    // --- Nodes ----------------------------------------------------------------------------------

    /// A node or a cluster: its outline as a shape, and what else it draws. With `bind_label`, a
    /// node's one text is its outline's label, and its outline is what edges join; a cluster's
    /// outline is that too, and its texts stay texts.
    ///
    /// The outline is the largest shape. A rectangle, an ellipse or a diamond is a board shape of
    /// its own; any other outline (a cylinder, a hexagon) is drawn as lines under a box nobody
    /// sees, which holds the label and is what edges join. So is a node that is only text, in a
    /// diagram with edges; in one without, nothing joins it and its text stays a text, set as the
    /// diagram's other texts are. An icon in it is a picture over its outline.
    ///
    /// A cluster that Pele only filled, with one of its neutral surfaces (a lane of an event
    /// model), comes in light grey: unfilled and unlined, it would be a box nobody sees.
    pub(super) fn group(&mut self, group: Node<'a, 'input>, bind_label: bool) {
        if self.is_done(group) || !visible(group) {
            return;
        }
        let shapes: Vec<Node> = group
            .descendants()
            .skip(1)
            .filter(|n| is_shape(*n) && !self.is_done(*n) && visible(*n) && !self.is_mask(*n))
            .collect();
        let texts: Vec<Node> = group
            .descendants()
            .skip(1)
            .filter(|n| n.has_tag_name("text") && !self.is_done(*n) && !text_of(*n).is_empty())
            .collect();
        let icons: Vec<Node> = group
            .descendants()
            .skip(1)
            .filter(|n| is_icon(*n) && !self.is_done(*n))
            .collect();
        self.done.insert(group.id());
        let outline = shapes
            .iter()
            .filter_map(|shape| Some((*shape, shape_box(*shape)?)))
            .fold(
                None,
                |largest: Option<(Node, Rect)>, (shape, frame)| match largest {
                    Some((_, most)) if most.area() >= frame.area() => largest,
                    _ => Some((shape, frame)),
                },
            );
        // The container, its frame as Pele drew it, and how much room it makes for a label.
        let container = match outline {
            Some((shape, frame)) => match container_kind(shape) {
                Some(kind) => {
                    self.done.insert(shape.id());
                    let rounded = is_rounded(shape, Some(group));
                    let mut drawn = self.box_shape(shape, kind, frame, rounded);
                    let unseen = drawn.background_color.as_deref() == Some(TRANSPARENT)
                        && drawn.stroke_color.as_deref() == Some(TRANSPARENT);
                    if !bind_label && unseen && paint(shape, "fill").is_some() {
                        drawn.background_color = Some(SURFACE_ALT_FILL.into());
                    }
                    Some((drawn, frame, Room::Words))
                }
                None => Some((invisible(frame), frame, Room::Words)),
            },
            None if bind_label && self.has_edges && texts.len() == 1 && !is_turned(texts[0]) => {
                let frame = text_place(texts[0]).bounds().inflate(TEXT_PAD, TEXT_PAD);
                Some((invisible(frame), frame, Room::Lines))
            }
            None => None,
        };
        let mut members = Vec::new();
        for shape in &shapes {
            if !self.is_done(*shape) {
                members.extend(self.shape(*shape));
            }
        }
        if let Some((mut container, frame, room)) = container {
            let label = texts.first().copied().filter(|text| {
                bind_label
                    && texts.len() == 1
                    && !is_turned(*text)
                    && holds_middle(frame, text_place(*text).bounds())
            });
            if let Some(text) = label {
                let said = text_of(text);
                // At the size Pele set it in, which it sized the node for.
                let size = font_size(text);
                fit(&mut container, &said, room, size);
                container.label = Some(Label {
                    text: said,
                    font_size: Some(size),
                });
                self.done.insert(text.id());
            }
            if bind_label || has_class(group, &["pele-cluster"]) {
                let id = format!("mermaid-{}", self.targets.len() + 1);
                container.id = Some(id.clone());
                // Pele ended its edges on the outline as it drew it, so that is what they join,
                // whatever room the label took.
                self.targets.push(Target {
                    id,
                    frame,
                    note: has_class(group, &["pele-note"]),
                });
            }
            members.push(container);
        }
        for icon in icons {
            members.extend(self.icon(icon));
        }
        for text in &texts {
            if !self.is_done(*text) {
                members.push(self.text(*text));
            }
        }
        self.out.extend(members);
    }
}

/// How much room [`fit`] makes for a label.
#[derive(Clone, Copy)]
enum Room {
    /// For its longest word. The engine breaks a label's lines between words where it must, and
    /// a node grown to keep Pele's lines whole would run into its neighbours; but a word too long
    /// for its line is broken inside it.
    Words,
    /// For each of its lines as it is: the box made around a node that is only text, which
    /// nobody sees.
    Lines,
}

/// Grows `shape` around its middle until the engine can set `label` in it, at font `size`, with
/// the `room` asked for. A board shape keeps its label inside its insets, so an ellipse or a
/// diamond holds less than the shape Pele drew around the same text (the web canvas's converter's
/// `fit`, with FigCraft's insets for Excalidraw's).
fn fit(shape: &mut Skeleton, label: &str, room: Room, size: f64) {
    let insets = match shape.kind.as_str() {
        "ellipse" => BoardShape::Ellipse,
        "diamond" => BoardShape::Diamond,
        _ => BoardShape::Rectangle,
    }
    .text_insets();
    let [top, right, bottom, left] = insets;
    let (width, height) = (shape.width.unwrap_or(0.0), shape.height.unwrap_or(0.0));
    let across = 1.0 - left - right;
    let (widest, tall) = match room {
        Room::Words => {
            // A label Pele set on one line in a box made for it: the engine's face sets it a
            // little wider, and a box grown that little keeps the line whole, where a broken one
            // would spill out of the box's height.
            let line = set_widest_line(label, size);
            let one_line = !label.contains('\n') && line / across <= width * ONE_LINE_GROWTH;
            let widest = if one_line {
                line
            } else {
                set_widest_word(label, size)
            };
            (widest, 0.0)
        }
        Room::Lines => {
            let lines = label.lines().count().max(1) as f64;
            let tall = lines * size * LABEL_LINE_HEIGHT / (1.0 - top - bottom);
            (set_widest_line(label, size), tall)
        }
    };
    // A pixel to spare, so that a word set exactly as wide as its room isn't broken.
    let wide = (widest + 1.0) / across;
    let (grown_width, grown_height) = (width.max(wide), height.max(tall));
    shape.x -= (grown_width - width) / 2.0;
    shape.y -= (grown_height - height) / 2.0;
    shape.width = Some(grown_width);
    shape.height = Some(grown_height);
}
