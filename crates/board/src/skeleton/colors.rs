//! Colouring a layer from a skeleton's `backgroundColor` and `strokeColor`, by the spec's rules.

use figcraft_color::Color;
use figcraft_doc::{BoardItem, NodeId, NodeKind};
use figcraft_engine::{EngineError, Result as EngineResult, Session};
use serde_json::{Map, Value, json};

/// Colour layer `id` from a skeleton's `backgroundColor` and `strokeColor`, by the spec's rules.
/// `apply` colours what it draws with it. It is public so that changing the colours of a layer
/// that is already there does the same as drawing it with them.
///
/// - Stickies and shapes with text: the background is `board.setColor` (a board colour name or
///   any colour; transparent clears the fill); the stroke is the outline's stroke paint.
/// - Connectors and ink have a line and no fill: the stroke is the line's colour, and the
///   background is ignored, because `board.setColor` would recolour the line with it.
/// - Text is coloured by its fill, and Excalidraw's text colour is its `strokeColor`: that is the
///   fill, and the background is ignored (nothing is drawn behind text).
/// - Any other layer (sections, frames, pictures): the background is its fill and the stroke its
///   stroke paint.
///
/// Transparent (`transparent`, `none`, or a colour with no alpha) is no paint at all. A colour that
/// is empty counts as not given. Colours the engine doesn't know are its error.
pub fn set_colors(
    session: &mut Session,
    id: NodeId,
    background: Option<&str>,
    stroke: Option<&str>,
) -> EngineResult<()> {
    colour(session, id, background, stroke, None)
}

/// [`set_colors`], with the paints that `node.set` can't refuse kept in `later` (with the index of
/// the element that layer `id` is drawn for) instead of set, when there is a `later`.
pub(super) fn colour(
    session: &mut Session,
    id: NodeId,
    background: Option<&str>,
    stroke: Option<&str>,
    mut later: Option<(&mut Later, usize)>,
) -> EngineResult<()> {
    let (background, stroke) = (given(background), given(stroke));
    let (item, is_text, outline) = {
        let node = session.doc()?.get(id)?;
        (
            node.board.clone(),
            matches!(node.kind, NodeKind::Text(_)),
            node.children.first().copied(),
        )
    };
    match item {
        Some(BoardItem::Connector(_) | BoardItem::Ink) => {
            if let Some(colour) = stroke {
                paint(session, &mut later, id, "stroke", colour)?;
            }
        }
        Some(BoardItem::Sticky { .. }) => {
            board_object(session, &mut later, id, id, background, stroke)?;
        }
        Some(BoardItem::Shape { .. }) => {
            let drawn = outline.unwrap_or(id);
            board_object(session, &mut later, id, drawn, background, stroke)?;
        }
        _ if is_text => {
            if let Some(colour) = stroke {
                paint(session, &mut later, id, "fill", colour)?;
            }
        }
        _ => {
            let paints: Vec<(&'static str, &str)> = [("fill", background), ("stroke", stroke)]
                .into_iter()
                .filter_map(|(key, colour)| Some((key, paint_value(colour?))))
                .collect();
            if paints.is_empty() {
                return Ok(());
            }
            let props: Map<String, Value> = paints
                .iter()
                .map(|(key, value)| (key.to_string(), json!(value)))
                .collect();
            let params = json!({"ids": [id], "props": props});
            // Both are kept, or both are set now with one `node.set` as before, so that a colour
            // the engine refuses fails the command it failed before.
            let all_certain = paints.iter().all(|(_, value)| certain(value));
            if let Some((later, index)) = later.as_mut().filter(|_| all_certain) {
                for (key, value) in paints {
                    later.keep(key, value, id, *index);
                }
                repeat_as(session, params);
            } else {
                session.execute("node.set", &params)?;
            }
        }
    }
    Ok(())
}

/// A colour as it was given: an empty one is none.
fn given(colour: Option<&str>) -> Option<&str> {
    colour.map(str::trim).filter(|c| !c.is_empty())
}

/// Colour a sticky or a shape with text, `id`, whose fill and stroke are `drawn`'s (the outline of
/// a shape; the sticky itself). The background comes first: `board.setColor` sets the outline's
/// stroke too, which the stroke colour then replaces.
fn board_object(
    session: &mut Session,
    later: &mut Option<(&mut Later, usize)>,
    id: NodeId,
    drawn: NodeId,
    background: Option<&str>,
    stroke: Option<&str>,
) -> EngineResult<()> {
    if let Some(colour) = background {
        if is_transparent(colour) {
            paint(session, later, drawn, "fill", colour)?;
        } else {
            session.execute("board.setColor", &json!({"ids": [id], "color": colour}))?;
        }
    }
    if let Some(colour) = stroke {
        paint(session, later, drawn, "stroke", colour)?;
    }
    Ok(())
}

/// Give layer `id` the paint `key` (`fill` or `stroke`) of `colour`: now, or with `later` when
/// `node.set` can't refuse it.
fn paint(
    session: &mut Session,
    later: &mut Option<(&mut Later, usize)>,
    id: NodeId,
    key: &'static str,
    colour: &str,
) -> EngineResult<()> {
    let value = paint_value(colour);
    let params = json!({"ids": [id], "props": { (key): value }});
    if let Some((later, index)) = later.as_mut().filter(|_| certain(value)) {
        later.keep(key, value, id, *index);
        repeat_as(session, params);
        return Ok(());
    }
    session.execute("node.set", &params)?;
    Ok(())
}

/// Leave `edit.repeat` the `node.set` (`params`) that set a kept paint on its own before paints
/// were kept, as that command did: what an `apply` leaves it to repeat is what it was then, the
/// last element drawn or its last paint, never a [`Later::set`] of several layers.
fn repeat_as(session: &mut Session, params: Value) {
    session.last_command = Some(("node.set".into(), params));
}

/// What `node.set` takes for `colour`: transparent is no paint.
fn paint_value(colour: &str) -> &str {
    if is_transparent(colour) {
        "none"
    } else {
        colour
    }
}

fn is_transparent(colour: &str) -> bool {
    colour.eq_ignore_ascii_case("none") || Color::parse_css(colour).is_some_and(|c| c.a <= 0.0)
}

/// Whether `node.set` takes `value` (what [`paint_value`] gives) as a paint whatever the layer:
/// no paint, or a colour it reads, as it reads one.
fn certain(value: &str) -> bool {
    value.eq_ignore_ascii_case("none") || Color::parse_css(value).is_some()
}

/// The paints an `apply` gives what it draws, kept to be set once it has drawn everything, each one
/// on all the layers that take it with one `node.set` ([`Later::set`]). Every command that changes a
/// board has the engine look at each connector on it, so a command of its own for each paint of
/// each layer made an `apply` about a fifth slower. Only paints `node.set` can't refuse are kept
/// ([`certain`]), and nothing an `apply` does after colouring a layer reads or sets its paints, so
/// the board comes out the same, a colour the engine refuses is still the error of its element,
/// and `edit.repeat` repeats what it did before ([`repeat_as`]).
#[derive(Default)]
pub(super) struct Later {
    /// The paints, in the order they were first given.
    paints: Vec<Kept>,
}

/// A paint kept for the end: `node.set`'s `key` of `value`, and the layers that take it, each with
/// the index of the element it is drawn for.
struct Kept {
    key: &'static str,
    value: String,
    layers: Vec<(NodeId, usize)>,
}

impl Later {
    /// Keep paint `key` of `value` for layer `id`, drawn for element `index`.
    fn keep(&mut self, key: &'static str, value: &str, id: NodeId, index: usize) {
        let kept = self
            .paints
            .iter_mut()
            .find(|kept| kept.key == key && kept.value == value);
        match kept {
            Some(kept) => kept.layers.push((id, index)),
            None => self.paints.push(Kept {
                key,
                value: value.to_string(),
                layers: vec![(id, index)],
            }),
        }
    }

    /// Set the kept paints, each with one `node.set` on all its layers. An error is that of the
    /// first of them, with the index of its element. `edit.repeat` is left to repeat what it was
    /// before: these `node.set`s stand for the ones [`repeat_as`] already put there.
    pub(super) fn set(self, session: &mut Session) -> Result<(), (usize, EngineError)> {
        let last = session.last_command.clone();
        let mut set = Ok(());
        for Kept { key, value, layers } in self.paints {
            let ids: Vec<NodeId> = layers.iter().map(|(id, _)| *id).collect();
            let props = json!({ (key): value });
            if let Err(error) = session.execute("node.set", &json!({"ids": ids, "props": props})) {
                set = Err((layers[0].1, error));
                break;
            }
        }
        session.last_command = last;
        set
    }
}
