//! Colouring a layer from a skeleton's `backgroundColor` and `strokeColor`, by the spec's rules.

use figcraft_color::Color;
use figcraft_doc::{BoardItem, NodeId, NodeKind};
use figcraft_engine::{Result as EngineResult, Session};
use serde_json::{Map, json};

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
                paint(session, id, "stroke", colour)?;
            }
        }
        Some(BoardItem::Sticky { .. }) => board_object(session, id, id, background, stroke)?,
        Some(BoardItem::Shape { .. }) => {
            board_object(session, id, outline.unwrap_or(id), background, stroke)?;
        }
        _ if is_text => {
            if let Some(colour) = stroke {
                paint(session, id, "fill", colour)?;
            }
        }
        _ => {
            let mut props = Map::new();
            for (key, colour) in [("fill", background), ("stroke", stroke)] {
                if let Some(colour) = colour {
                    props.insert(key.into(), json!(paint_value(colour)));
                }
            }
            if !props.is_empty() {
                session.execute("node.set", &json!({"ids": [id], "props": props}))?;
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
    id: NodeId,
    drawn: NodeId,
    background: Option<&str>,
    stroke: Option<&str>,
) -> EngineResult<()> {
    if let Some(colour) = background {
        if is_transparent(colour) {
            paint(session, drawn, "fill", colour)?;
        } else {
            session.execute("board.setColor", &json!({"ids": [id], "color": colour}))?;
        }
    }
    if let Some(colour) = stroke {
        paint(session, drawn, "stroke", colour)?;
    }
    Ok(())
}

/// Give layer `id` the paint `key` (`fill` or `stroke`) of `colour`.
fn paint(session: &mut Session, id: NodeId, key: &str, colour: &str) -> EngineResult<()> {
    let props = json!({ (key): paint_value(colour) });
    session.execute("node.set", &json!({"ids": [id], "props": props}))?;
    Ok(())
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
