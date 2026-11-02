//! An `apply` in progress: each element made in its stage, and found again by the id it was given.

use std::collections::HashMap;

use figcraft_doc::{BoardItem, NodeId, NodeKind};
use figcraft_engine::params::id_of;
use figcraft_engine::{EngineError, Result as EngineResult, Session};
use serde_json::{Value, json};

use super::colors::{self, Later};
use super::element::{Element, STAGES};
use super::{Applied, Maker, SAWIT_ID, Skeleton, TYPES, agent_id, node_id};

impl<'a> Maker<'a> {
    pub(super) fn new(session: &'a mut Session, offset: (f64, f64)) -> EngineResult<Self> {
        let page = session.page()?;
        let mut before: HashMap<String, NodeId> = HashMap::new();
        for node in session.doc()?.nodes() {
            if let Some(id) = agent_id(node) {
                let newest = before.entry(id.to_string()).or_insert(node.id);
                *newest = (*newest).max(node.id);
            }
        }
        Ok(Self {
            session,
            offset,
            page,
            before,
            made: HashMap::new(),
            notes: Vec::new(),
            later: Later::default(),
        })
    }

    pub(super) fn make(mut self, items: &[Skeleton]) -> EngineResult<Applied> {
        let elements: Vec<Option<Element>> = items.iter().map(|i| Element::of(&i.kind)).collect();
        for (index, (item, element)) in items.iter().zip(&elements).enumerate() {
            if element.is_none() {
                self.notes.push(unknown_type(index, &item.kind));
            }
        }
        let mut nodes: Vec<Option<NodeId>> = vec![None; items.len()];
        for stage in 0..STAGES {
            for (index, (item, element)) in items.iter().zip(&elements).enumerate() {
                let Some(element) = element.filter(|e| e.stage() == stage) else {
                    continue;
                };
                nodes[index] = self
                    .element(index, item, element)
                    .map_err(|e| in_element(index, item, e))?;
            }
        }
        std::mem::take(&mut self.later)
            .set(self.session)
            .map_err(|(index, e)| in_element(index, &items[index], e))?;
        for (index, (item, node)) in items.iter().zip(&nodes).enumerate() {
            self.place_in_frame(item, *node)
                .map_err(|e| in_element(index, item, e))?;
        }
        Ok(Applied {
            ids: nodes.iter().flatten().map(NodeId::to_string).collect(),
            notes: self.notes,
        })
    }

    /// Make one element, and remember the id the agent gave it.
    fn element(
        &mut self,
        index: usize,
        item: &Skeleton,
        element: Element,
    ) -> EngineResult<Option<NodeId>> {
        let node = match element {
            Element::Rectangle | Element::Ellipse | Element::Diamond => {
                Some(self.shape(index, item, element)?)
            }
            Element::Sticky => Some(self.sticky(index, item)?),
            Element::Text => Some(self.text(index, item)?),
            Element::Frame => Some(self.frame(index, item)?),
            Element::Image => self.image(index, item)?,
            Element::Arrow | Element::Line | Element::Freedraw => {
                self.line(index, item, element)?
            }
        };
        if let (Some(node), Some(id)) = (node, item.id.as_deref().filter(|id| !id.is_empty())) {
            let props = json!({"pluginData": {(SAWIT_ID): id}});
            self.session
                .execute("node.set", &json!({"ids": [node], "props": props}))?;
            self.made.insert(id.to_string(), node);
        }
        Ok(node)
    }

    /// Run a command that makes a layer, and name the layer it made.
    pub(super) fn create(&mut self, command: &str, args: &Value) -> EngineResult<NodeId> {
        let made = self.session.execute(command, args)?;
        made.get("id")
            .and_then(id_of)
            .ok_or_else(|| EngineError::Other(format!("`{command}` made no layer")))
    }

    /// Where `item` is on the page.
    pub(super) fn at(&self, item: &Skeleton) -> (f64, f64) {
        (item.x + self.offset.0, item.y + self.offset.1)
    }

    /// Colour layer `id`, drawn for element `index` (`item`), keeping the paints it can for the end.
    pub(super) fn colour(&mut self, index: usize, id: NodeId, item: &Skeleton) -> EngineResult<()> {
        colors::colour(
            self.session,
            id,
            item.background_color.as_deref(),
            item.stroke_color.as_deref(),
            Some((&mut self.later, index)),
        )
    }

    /// The node `id` names for this call: its own element, else one the board has under that
    /// agent id, else the node with that id.
    pub(super) fn lookup(&self, id: &str) -> Option<NodeId> {
        let named = self.made.get(id).or_else(|| self.before.get(id)).copied();
        named.or_else(|| self.session.doc().ok().and_then(|doc| node_id(doc, id)))
    }

    /// Whether a connector can end on `node`: a layer of this page, but not the page, and not
    /// another connector.
    pub(super) fn can_join(&self, node: NodeId) -> bool {
        let Ok(doc) = self.session.doc() else {
            return false;
        };
        doc.node(node)
            .is_some_and(|n| !n.is_page() && !matches!(n.board, Some(BoardItem::Connector(_))))
            && doc.page_of(node) == Some(self.page)
    }

    /// Whether `frame` can take `node` in: a section or a frame of this page that isn't `node`
    /// or inside it (sections go in sections only), and `node` isn't a connector or ink, which
    /// are always on the page.
    fn can_hold(&self, frame: NodeId, node: NodeId) -> bool {
        let Ok(doc) = self.session.doc() else {
            return false;
        };
        let (Some(holder), Some(inner)) = (doc.node(frame), doc.node(node)) else {
            return false;
        };
        let holds = matches!(holder.kind, NodeKind::Section { .. })
            || (holder.kind.is_frame_like() && !holder.is_board_atom());
        let takes_it = !matches!(inner.kind, NodeKind::Section { .. }) || doc.takes_sections(frame);
        let on_the_page = !matches!(inner.board, Some(BoardItem::Connector(_) | BoardItem::Ink));
        holds
            && takes_it
            && on_the_page
            && !doc.is_ancestor(node, frame)
            && doc.page_of(frame) == Some(self.page)
    }

    /// Move `node`, made from `item`, into the section its `frameId` names, if it can go there.
    /// It stays where it was on the page.
    fn place_in_frame(&mut self, item: &Skeleton, node: Option<NodeId>) -> EngineResult<()> {
        let (Some(frame_id), Some(node)) = (item.frame_id.as_deref(), node) else {
            return Ok(());
        };
        let Some(frame) = self
            .lookup(frame_id)
            .filter(|frame| self.can_hold(*frame, node))
        else {
            return Ok(());
        };
        self.session
            .execute("node.reparent", &json!({"ids": [node], "parent": frame}))?;
        Ok(())
    }
}

/// The note on an element that has no known type.
fn unknown_type(index: usize, kind: &str) -> String {
    let problem = if kind.trim().is_empty() {
        "it has no type".to_string()
    } else {
        format!("`{kind}` is not a type")
    };
    format!("skipped elements[{index}]: {problem} (the types are {TYPES})")
}

/// `error`, said to be that of element `index`.
fn in_element(index: usize, item: &Skeleton, error: EngineError) -> EngineError {
    EngineError::Other(format!("elements[{index}] ({}): {error}", item.kind))
}
