//! A drag of rows in the layer tree: what it carries, what follows the pointer, where a drop on a
//! row lands, and the drop itself, through `node.reparent`.

use figcraft_doc::NodeId;
use figcraft_engine::guard;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_board::layers::{self, DropPos, Reparent};
use serde_json::json;

use super::LayerDrag;
use crate::board::BoardView;

/// What a drag in the layer tree carries: the canvas it started in, and the row dragged. The
/// layers it moves are the canvas's own ([`LayerDrag`]).
#[derive(Clone, Copy, Debug)]
pub(super) struct DraggedRow {
    pub(super) view: EntityId,
    pub(super) row: NodeId,
}

/// What follows the pointer while layers are dragged: the name of the layer, or how many there are.
pub(super) struct DragLabel(pub(super) SharedString);

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .text_xs()
            .shadow_md()
            .child(self.0.clone())
    }
}

/// Where a drop on a row lands, `rel` being how far down the row the pointer is (0 at its top, 1
/// at its bottom): into the row's layer in the middle half of the row when it takes layers
/// (`into`), else above it in the top half and below it in the bottom half, as in FigCraft.
pub(super) fn drop_pos(rel: f32, into: bool) -> DropPos {
    if into && (0.25..0.75).contains(&rel) {
        DropPos::Into
    } else if rel < 0.5 {
        DropPos::Above
    } else {
        DropPos::Below
    }
}

impl BoardView {
    /// Drops the layers being dragged ([`Self::drag`](BoardView)) on row `over` of the layer tree,
    /// at `pos`: next to the row's layer or into it, as `sawit_board::layers::drop_target` works
    /// it out, through `node.reparent`, as one undo step. Where nothing may be dropped, or a drop
    /// would leave the layers where they are, it does nothing. The drag ends.
    pub fn drop_rows(&mut self, over: usize, pos: DropPos, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        cx.notify();
        let rows = self.layer_rows();
        let Some(doc) = self.board().and_then(|board| board.doc().ok()) else {
            return;
        };
        let target = guard::run("ui.dropTarget", || {
            Ok(layers::drop_target(&doc, &rows, &drag.ids, over, pos))
        });
        let Ok(Some(Reparent { ids, parent, index })) = target else {
            return;
        };
        let params = json!({"ids": ids, "parent": parent, "index": index});
        self.choose("node.reparent", params, cx);
    }

    /// A drag starts from the row of layer `row`: it carries the selection when the row is in it,
    /// else the row's layer, as FigCraft's panel drags. What follows the pointer.
    pub(super) fn begin_drag(&mut self, row: NodeId, cx: &mut Context<Self>) -> SharedString {
        let selection = (self.board())
            .map(|board| board.session.selection())
            .unwrap_or_default();
        let ids = if selection.contains(&row) {
            selection
        } else {
            vec![row]
        };
        let label = match ids.len() {
            1 => (self.layer_rows().iter())
                .find(|shown| shown.id == row)
                .map_or_else(|| "1 layer".into(), |shown| shown.name.clone().into()),
            n => format!("{n} layers").into(),
        };
        self.drag = Some(LayerDrag { ids, over: None });
        cx.notify();
        label
    }

    /// The drag moved, over row `row` or elsewhere (`event.bounds` are the row's): over the row, the
    /// mark shows where a drop there lands, if it moves the layers; off it, the row's mark goes.
    pub(super) fn drag_over(
        &mut self,
        row: usize,
        event: &DragMoveEvent<DraggedRow>,
        cx: &mut Context<Self>,
    ) {
        let Some(ids) = self.drag.as_ref().map(|drag| drag.ids.clone()) else {
            return;
        };
        let at = event.event.position;
        let bounds = event.bounds;
        let over = if bounds.contains(&at) {
            let rel = f32::from(at.y - bounds.origin.y) / f32::from(bounds.size.height);
            let rows = self.layer_rows();
            let doc = self.board().and_then(|board| board.doc().ok());
            let landing = doc.and_then(|doc| {
                guard::run("ui.dropTarget", || {
                    let target = |pos| layers::drop_target(&doc, &rows, &ids, row, pos);
                    let pos = drop_pos(rel, target(DropPos::Into).is_some());
                    Ok(target(pos).map(|_| pos))
                })
                .ok()
                .flatten()
            });
            landing.map(|pos| (row, pos))
        } else if self
            .drag
            .as_ref()
            .and_then(|drag| drag.over)
            .map(|(at, _)| at)
            == Some(row)
        {
            None
        } else {
            return;
        };
        if let Some(drag) = &mut self.drag
            && drag.over != over
        {
            drag.over = over;
            cx.notify();
        }
    }

    /// The drag was dropped on a row: the layers go where its mark showed, if one did. A drag from
    /// another canvas's panel drops nothing here.
    pub(super) fn drop_dragged(&mut self, dragged: &DraggedRow, cx: &mut Context<Self>) {
        if dragged.view != cx.entity_id() {
            return;
        }
        match self.drag.as_ref().and_then(|drag| drag.over) {
            Some((row, pos)) => self.drop_rows(row, pos, cx),
            None => {
                self.drag = None;
                cx.notify();
            }
        }
    }
}
