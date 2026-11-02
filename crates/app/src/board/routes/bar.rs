//! The bar over a selected route frame, in the context bar's place: Live (the page itself on the
//! frame, as a double-click does), Reload, and Open in Browser, which opens the route in the
//! project's Browser tab.

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::context_bar as model;
use sawit_board::routes::{WebData, web_of};

use crate::board::BoardView;
use crate::board::toolbar::{self, divider};

/// The bar's height and width, and the room between it and the frame's name above the frame.
const HEIGHT: f64 = 40.;
const WIDTH: f64 = 300.;
const ABOVE: f64 = 34.;

impl BoardView {
    /// The route frame selected alone, when one is and it isn't live, with its data.
    fn selected_route(&self) -> Option<(NodeId, WebData)> {
        let ds = self.board()?.session.ds().ok()?;
        let [frame] = ds.selection.as_slice() else {
            return None;
        };
        if self.web_live() == Some(*frame) {
            return None;
        }
        Some((*frame, web_of(ds.doc.node(*frame)?)?))
    }

    /// Opens `url` in the project's Browser tab.
    pub(in crate::board) fn open_in_browser(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = self.canvas_project();
        let Some(workspace) = crate::cli::workspace_for(window.window_handle(), cx) else {
            self.show_toast("The window's Browser tab couldn't be found", cx);
            return;
        };
        let _ = workspace.update(cx, |ws, cx| {
            ws.open_browser_of(project, Some(url), window, cx)
        });
    }
}

/// The bar over the route frame selected alone, when one is and the canvas isn't editing text.
pub(in crate::board) fn render_route_bar(
    view: &mut BoardView,
    cx: &mut Context<BoardView>,
) -> Option<impl IntoElement + use<>> {
    if view.editing_text() || view.preview_open() {
        return None;
    }
    let (frame, data) = view.selected_route()?;
    let ds = view.board()?.session.ds().ok()?;
    if view.board()?.session.tool.busy() {
        return None;
    }
    let b = ds.doc.world_bounds(frame)?;
    let (x0, y0) = ds.view.page_to_screen(b.x0, b.y0);
    let (x1, _) = ds.view.page_to_screen(b.x1, b.y0);
    let under = f64::from(toolbar::BOTTOM + toolbar::HEIGHT);
    let canvas = (ds.view.vw, ds.view.vh - under);
    let width = WIDTH.min(canvas.0 - 2.0 * model::MARGIN).max(0.0);
    let top_centre = ((x0 + x1) / 2.0, y0 - ABOVE - HEIGHT);
    let (x, y) = model::clamp_anchor(top_centre, (width, HEIGHT), canvas);
    let local = sawit_board::routes::localhost(&data.url);
    let theme = cx.theme();
    let line = theme.border;
    let live = Button::new("route-bar-live")
        .ghost()
        .small()
        .label("Live")
        .tooltip("Use the page itself on the canvas")
        .disabled(!local)
        .on_click(cx.listener(move |view, _, _, cx| {
            let _ = view.web_enter(frame, cx);
        }));
    let reload = Button::new("route-bar-reload")
        .ghost()
        .small()
        .label("Reload")
        .disabled(!local)
        .on_click(cx.listener(move |view, _, _, cx| view.web_reload_frame(frame, cx)));
    let url = data.url.clone();
    let open = Button::new("route-bar-browser")
        .ghost()
        .small()
        .label("Open in Browser")
        .tooltip(data.url.clone())
        .on_click(
            cx.listener(move |view, _, window, cx| view.open_in_browser(url.clone(), window, cx)),
        );
    Some(
        h_flex()
            .id("route-bar")
            .debug_selector(|| "route-bar".into())
            .occlude()
            .absolute()
            .left(px((x - width / 2.0) as f32))
            .top(px(y as f32))
            .w(px(width as f32))
            .h(px(HEIGHT as f32))
            .px_1()
            .gap_0p5()
            .justify_center()
            .rounded(px(10.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_md()
            .child(live)
            .child(reload)
            .child(divider(line))
            .child(open),
    )
}
