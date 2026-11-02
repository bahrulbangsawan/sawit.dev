//! The layers each kind of skeleton becomes, drawn the same whatever the user last picked.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_doc::{NodeId, Routing};
use figcraft_engine::board::BoardTool;
use figcraft_engine::params::id_of;
use figcraft_engine::{EngineError, Result as EngineResult};
use serde::Serialize;
use serde_json::{Value, json};

use super::element::{
    Element, draws_a_line, is_rounded, label_or_text, line_extent, text_or_label,
};
use super::{End, Maker, Skeleton};

// What a skeleton leaves open is drawn the same on every board. FigCraft fills it in from the
// session: a new shape's or sticky's colour, the colour of ink and a connector's routing from the
// toolbar, and a sticky's author line from Preferences. In the app those are whatever the user
// last picked and typed, in a headless session (the MCP server with no app running) they are the
// defaults, so the same skeletons would come out differently by path and by the user's last click.
// `apply` therefore names all of them (below), and `set_colors` applies the colours the skeleton
// does name afterwards. Text is left alone: its default fill follows its page, not the toolbar.

/// The colour of a shape or a sticky whose skeleton names none: the engine's default for the
/// sticky tool (yellow), not the one the user has picked.
fn default_fill() -> String {
    BoardTool::default().sticky.to_css_hex()
}

/// The colour of ink whose skeleton names none: the engine's default for the marker (#1E1E1E).
fn default_ink() -> String {
    BoardTool::default().marker.to_css_hex()
}

/// How a connector runs: a straight line. The engine's own default is elbow, which draws a line or
/// an arrow with free ends as a Z, so every diagonal line of an agent or of an Excalidraw scene
/// would be bent. A straight one is today's arrow, edge to edge, and the spec's 2-point line.
fn default_routing() -> &'static str {
    Routing::Straight.id()
}

/// One end of a connector: the layer it joins, or else where it is. A free end is where it is;
/// for one that joins a layer, `at` is where it would be if it didn't.
struct Anchor {
    node: Option<NodeId>,
    at: (f64, f64),
}

impl Anchor {
    /// The end as `board.connector` takes it.
    fn json(&self) -> Value {
        match self.node {
            Some(node) => json!(node),
            None => json!({"x": self.at.0, "y": self.at.1}),
        }
    }
}

impl<'a> Maker<'a> {
    pub(super) fn shape(
        &mut self,
        index: usize,
        item: &Skeleton,
        element: Element,
    ) -> EngineResult<NodeId> {
        let shape = match element {
            Element::Ellipse => "ellipse",
            Element::Diamond => "diamond",
            _ if is_rounded(item.roundness.as_ref()) => "roundedRectangle",
            _ => "rectangle",
        };
        let (x, y) = self.at(item);
        let mut args = json!({"shape": shape, "x": x, "y": y, "color": default_fill()});
        put(&mut args, "width", item.width);
        put(&mut args, "height", item.height);
        put(&mut args, "text", label_or_text(item));
        let made = self.session.execute("board.shape", &args)?;
        let id = made
            .get("id")
            .and_then(id_of)
            .ok_or_else(|| EngineError::Other("`board.shape` made no layer".into()))?;
        let size = item.label.as_ref().and_then(|label| label.font_size);
        if let Some(label) = made.get("text").and_then(id_of) {
            self.font_size(label, size)?;
        }
        self.colour(index, id, item)?;
        Ok(id)
    }

    /// Sets text `id` in `size` px, when that is a size the engine takes.
    fn font_size(&mut self, id: NodeId, size: Option<f64>) -> EngineResult<()> {
        let Some(size) = size.filter(|size| size.is_finite() && *size > 0.0) else {
            return Ok(());
        };
        let props = json!({"fontSize": size});
        self.session
            .execute("node.set", &json!({"ids": [id], "props": props}))?;
        Ok(())
    }

    pub(super) fn sticky(&mut self, index: usize, item: &Skeleton) -> EngineResult<NodeId> {
        let (x, y) = self.at(item);
        // `author: false`: a note an agent makes is not signed with the user's name.
        let mut args = json!({"x": x, "y": y, "color": default_fill(), "author": false});
        put(&mut args, "text", label_or_text(item));
        let id = self.create("board.sticky", &args)?;
        self.colour(index, id, item)?;
        Ok(id)
    }

    pub(super) fn text(&mut self, index: usize, item: &Skeleton) -> EngineResult<NodeId> {
        let (x, y) = self.at(item);
        let text = text_or_label(item).unwrap_or_default();
        let mut args = json!({"type": "text", "x": x, "y": y, "text": text});
        // A width makes the text wrap at it; without one it is as wide as it is long.
        put(&mut args, "width", item.width.filter(|w| *w > 0.0));
        let id = self.create("node.create", &args)?;
        self.font_size(id, item.font_size)?;
        self.colour(index, id, item)?;
        Ok(id)
    }

    pub(super) fn frame(&mut self, index: usize, item: &Skeleton) -> EngineResult<NodeId> {
        let (x, y) = self.at(item);
        let mut args = json!({"type": "section", "x": x, "y": y});
        put(&mut args, "width", item.width);
        put(&mut args, "height", item.height);
        let title = item.name.as_deref().filter(|n| !n.is_empty());
        put(&mut args, "name", title.or(label_or_text(item)));
        let id = self.create("node.create", &args)?;
        self.colour(index, id, item)?;
        Ok(id)
    }

    /// A picture: SVG becomes an editable frame, anything else an image layer. One that can't be
    /// imported is skipped with a note, because its bytes come from outside and the shapes
    /// around it are no worse for its loss.
    pub(super) fn image(&mut self, index: usize, item: &Skeleton) -> EngineResult<Option<NodeId>> {
        let name = item
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        let (data, name) = match (item.svg.as_deref(), item.data.as_deref()) {
            (Some(svg), _) => {
                let name = name.unwrap_or("diagram.svg");
                let svg_name = if name.to_ascii_lowercase().ends_with(".svg") {
                    name.to_string()
                } else {
                    format!("{name}.svg")
                };
                (STANDARD.encode(svg), svg_name)
            }
            (None, Some(data)) => (data.to_string(), name.unwrap_or("image.png").to_string()),
            (None, None) => {
                let note =
                    format!("skipped elements[{index}]: the image has neither `svg` nor `data`");
                self.notes.push(note);
                return Ok(None);
            }
        };
        let (x, y) = self.at(item);
        let args = json!({"data": data, "name": name, "x": x, "y": y});
        let placed = match self.session.execute("file.import", &args) {
            Ok(placed) => placed,
            Err(error) => {
                let note =
                    format!("skipped elements[{index}]: the image could not be imported ({error})");
                self.notes.push(note);
                return Ok(None);
            }
        };
        let id = placed
            .get("id")
            .and_then(id_of)
            .ok_or_else(|| EngineError::Other("`file.import` made no layer".into()))?;
        let warnings = placed.get("warnings").and_then(Value::as_array);
        for warning in warnings.into_iter().flatten().filter_map(Value::as_str) {
            self.notes.push(format!("elements[{index}]: {warning}"));
        }
        // SVG keeps the size it was drawn at; a picture takes the size it is given.
        if !name.to_ascii_lowercase().ends_with(".svg") {
            self.fit_picture(id, &placed, item)?;
        } else if item.as_group {
            self.session
                .execute("object.convertToGroup", &json!({"ids": [id]}))?;
        }
        Ok(Some(id))
    }

    /// Resize the picture `id`, just placed (`placed` is the engine's answer), to the size the
    /// skeleton gives: both sides as given, or one side as given and the other in proportion.
    fn fit_picture(&mut self, id: NodeId, placed: &Value, item: &Skeleton) -> EngineResult<()> {
        let natural = |key: &str| placed.get(key).and_then(Value::as_f64).filter(|n| *n > 0.0);
        let given = |size: Option<f64>| size.filter(|n| n.is_finite() && *n > 0.0);
        let (Some(natural_width), Some(natural_height)) = (natural("width"), natural("height"))
        else {
            return Ok(());
        };
        let (width, height) = match (given(item.width), given(item.height)) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) => (width, natural_height * width / natural_width),
            (None, Some(height)) => (natural_width * height / natural_height, height),
            (None, None) => return Ok(()),
        };
        let props = json!({"width": width, "height": height});
        self.session
            .execute("node.set", &json!({"ids": [id], "props": props}))?;
        Ok(())
    }

    /// An arrow, a line or a freedraw: ink when it is a freedraw or a line through more than two
    /// points, else a connector.
    pub(super) fn line(
        &mut self,
        index: usize,
        item: &Skeleton,
        element: Element,
    ) -> EngineResult<Option<NodeId>> {
        let (x, y) = self.at(item);
        let points: Vec<[f64; 2]> = item
            .points
            .iter()
            .flatten()
            .map(|p| [x + p[0], y + p[1]])
            .collect();
        if element == Element::Freedraw || (element == Element::Line && points.len() > 2) {
            if !draws_a_line(&points) {
                let note = format!("skipped elements[{index}]: its points draw no line");
                self.notes.push(note);
                return Ok(None);
            }
            let args = json!({"points": points, "color": default_ink()});
            let id = self.create("board.ink", &args)?;
            self.colour(index, id, item)?;
            return Ok(Some(id));
        }
        self.connector(index, item, element, &points)
    }

    /// A connector from the start of `item` to its end. An end joins the layer it names. One that
    /// names no layer it can join is free: at its own `x`/`y`, else where `item` is drawn from (its
    /// `x`, `y`) or to (its last point, else `x`, `y` plus its size).
    fn connector(
        &mut self,
        index: usize,
        item: &Skeleton,
        element: Element,
        points: &[[f64; 2]],
    ) -> EngineResult<Option<NodeId>> {
        let (x, y) = self.at(item);
        let (width, height) = line_extent(item);
        let end_at = points
            .last()
            .map_or((x + width, y + height), |p| (p[0], p[1]));
        let from = self.anchor(item.start.as_ref(), (x, y));
        let mut to = self.anchor(item.end.as_ref(), end_at);
        if from.node.is_some() && from.node == to.node {
            let note = format!(
                "elements[{index}]: both ends join the same layer, so the end is left free where it is drawn"
            );
            self.notes.push(note);
            to.node = None;
        }
        if from.node.is_none() && to.node.is_none() && from.at == to.at {
            let note = format!("skipped elements[{index}]: it starts and ends at the same point");
            self.notes.push(note);
            return Ok(None);
        }
        let end_cap = if element == Element::Arrow {
            "triangleArrow"
        } else {
            "none"
        };
        let mut args = json!({
            "from": from.json(), "to": to.json(), "style": default_routing(),
            "startCap": "none", "endCap": end_cap,
        });
        put(
            &mut args,
            "label",
            label_or_text(item).filter(|t| !t.is_empty()),
        );
        let id = self.create("board.connector", &args)?;
        self.colour(index, id, item)?;
        Ok(Some(id))
    }

    /// One end of a connector. `end`'s `x`/`y` are page coordinates, so the offset moves them;
    /// what `end` leaves out comes from `fallback`, which is already moved.
    fn anchor(&self, end: Option<&End>, fallback: (f64, f64)) -> Anchor {
        let at = end.map_or(fallback, |end| {
            (
                end.x.map_or(fallback.0, |x| x + self.offset.0),
                end.y.map_or(fallback.1, |y| y + self.offset.1),
            )
        });
        let node = end
            .and_then(|end| end.id.as_deref())
            .and_then(|id| self.lookup(id))
            .filter(|node| self.can_join(*node));
        Anchor { node, at }
    }
}

/// `args[key] = value` when there is a value.
fn put<T: Serialize>(args: &mut Value, key: &str, value: Option<T>) {
    if let Some(value) = value {
        args[key] = json!(value);
    }
}
