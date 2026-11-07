//! The bar over a selected artboard (a frame of layers right on its page), on the canvas's one
//! floating bar like the screen bar (B13): Export ▾ (HTML, React, PNG, SVG) | Send to Agent ▾.
//! Over several frames selected (screens or artboards) it holds Send Flow to Agent ▾, which hands
//! them off as one flow, in canvas order.

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::{frame_code, screens};

use super::bar::{export_button, send_button};
use crate::board::BoardView;
use crate::board::float_bar::{self, FloatBar};
use crate::board::toolbar;

impl BoardView {
    /// The frames the frame bar is for: one artboard selected alone, or several frames (screens
    /// and artboards) right on their page. Not while one of them is live.
    pub(in crate::board) fn selected_frames(&self) -> Option<Vec<NodeId>> {
        let ds = self.board()?.session.ds().ok()?;
        let selection = ds.selection.clone();
        let doc = &ds.doc;
        let top = |id: &NodeId| frame_code::is_artboard(doc, *id);
        if selection.is_empty() || !selection.iter().all(top) {
            return None;
        }
        if selection.iter().any(|id| self.web_live() == Some(*id)) {
            return None;
        }
        let web = |id: &NodeId| {
            let node = doc.node(*id);
            node.is_some_and(|n| n.plugin_data.contains_key(sawit_board::routes::WEB_KEY))
        };
        match selection.as_slice() {
            [one] if screens::screen_of(doc.node(*one)?).is_some() || web(one) => None,
            [_] => Some(selection),
            _ if selection.iter().any(web) => None,
            _ => Some(selection),
        }
    }
}

/// The bar over the frames [`BoardView::selected_frames`] gives, when the canvas isn't editing
/// text or busy with a gesture.
pub(in crate::board) fn render_frame_bar(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Option<impl IntoElement + use<>> {
    if view.editing_text() || view.preview_open() {
        return None;
    }
    let frames = view.selected_frames()?;
    let ds = view.board()?.session.ds().ok()?;
    if view.board()?.session.tool.busy() {
        return None;
    }
    let mut bounds = None;
    for id in &frames {
        let b = ds.doc.world_bounds(*id)?;
        bounds = Some(match bounds {
            None => (b.x0, b.y0, b.x1),
            Some((x0, y0, x1)) => (f64::min(x0, b.x0), f64::min(y0, b.y0), f64::max(x1, b.x1)),
        });
    }
    let (bx0, by0, bx1) = bounds?;
    let (x0, y0) = ds.view.page_to_screen(bx0, by0);
    let (x1, _) = ds.view.page_to_screen(bx1, by0);
    let under = f64::from(toolbar::BOTTOM + toolbar::HEIGHT);
    let canvas = (ds.view.vw, ds.view.vh - under);
    let top_centre = ((x0 + x1) / 2.0, y0 - float_bar::ABOVE - float_bar::HEIGHT);
    view.find_agents(cx);
    let agents = view.screen_ui.agents.clone();
    let folder = view.handoff_folder(&frames);
    let this = cx.entity().downgrade();
    let label = match frames.len() > 1 {
        true => "Send Flow to Agent",
        false => "Send to Agent",
    };
    let mut bar = FloatBar::new("frame-bar");
    if let [frame] = frames.as_slice() {
        bar = bar
            .button(export_button(
                this.clone(),
                "frame-bar-export",
                *frame,
                false,
            ))
            .divider();
    }
    let send = send_button("frame-bar-send", label, frames, agents, folder, this);
    Some(bar.button(send).render(top_centre, canvas, window, cx))
}
