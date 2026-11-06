//! The bar over a selected artboard (a frame of layers right on its page), in the screen bar's
//! place and style: Export HTML, Export React and Send to Agent…. Over several frames selected
//! (screens or artboards) it holds Send Flow to Agent…, which hands them off as one flow, in
//! canvas order.

use figcraft_doc::NodeId;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::context_bar as model;
use sawit_board::{frame_code, screens};

use gpui_kit::component::button::*;
use gpui_kit::component::menu::DropdownMenu as _;

use super::bar::{agents_menu, export_button};
use crate::board::BoardView;
use crate::board::toolbar::{self, divider};

/// The bar's height and widths (one frame, a flow), and the room above the frames.
const HEIGHT: f64 = 40.;
const WIDTH: f64 = 360.;
const FLOW_WIDTH: f64 = 220.;
const ABOVE: f64 = 34.;

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
    let flow = frames.len() > 1;
    let width = if flow { FLOW_WIDTH } else { WIDTH }
        .min(canvas.0 - 2.0 * model::MARGIN)
        .max(0.0);
    let (x, y) = model::clamp_anchor(
        ((x0 + x1) / 2.0, y0 - ABOVE - HEIGHT),
        (width, HEIGHT),
        canvas,
    );
    view.find_agents(cx);
    let agents = view.screen_ui.agents.clone();
    let this = cx.entity().downgrade();
    let theme = cx.theme();
    let line = theme.border;
    let label = if flow {
        "Send Flow to Agent…"
    } else {
        "Send to Agent…"
    };
    let (to_agent, targets) = (this.clone(), frames.clone());
    let send = Button::new("frame-bar-send")
        .ghost()
        .small()
        .label(label)
        .dropdown_caret(true)
        .dropdown_menu(move |menu, _, _| agents_menu(menu, &targets, &agents, &to_agent));
    let mut bar = h_flex()
        .id("frame-bar")
        .debug_selector(|| "frame-bar".into())
        .occlude()
        .absolute()
        .left(px((x - width / 2.0) as f32))
        .top(px(y as f32))
        .w(px(width as f32))
        .h(px(HEIGHT as f32))
        .px_1()
        .gap_0p5()
        .rounded(px(10.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow_md()
        .overflow_x_scroll();
    if let [frame] = frames.as_slice() {
        bar = bar
            .child(export_button(this.clone(), "frame-bar-html", *frame, false))
            .child(export_button(this, "frame-bar-react", *frame, true))
            .child(divider(line));
    }
    Some(bar.child(send))
}
