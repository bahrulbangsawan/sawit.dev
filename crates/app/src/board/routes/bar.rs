//! The bar over a selected route frame, in the context bar's place: Live (the page itself on the
//! frame, as a double-click does), Reload, Full page (on: the frame takes its page's whole height
//! each time the page settles; off: it keeps the height it has; `web::fit`), and Open in Browser,
//! which opens the route in the project's Browser tab.

use figcraft_doc::NodeId;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::*;
use sawit_board::routes::{WebData, web_of};

use crate::board::BoardView;
use crate::board::float_bar::{self, BarButton, FloatBar};
use crate::board::toolbar;
use crate::menu_icons;

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

/// The bar over the route frame selected alone, when one is and the canvas isn't editing text:
/// Live, Reload and Full Page (a toggle), then Open in Browser, on the canvas's one floating bar
/// ([`FloatBar`]). A remote page can't be live, reloaded or fitted: those three are disabled.
pub(in crate::board) fn render_route_bar(
    view: &mut BoardView,
    window: &mut Window,
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
    let top_centre = ((x0 + x1) / 2.0, y0 - float_bar::ABOVE - float_bar::HEIGHT);
    let local = sawit_board::routes::localhost(&data.url);
    let this = cx.entity().downgrade();
    let live = {
        let this = this.clone();
        BarButton::new("route-bar-live", Lucide::Radio, "Live")
            .tooltip("Use the page itself on the canvas")
            .disabled(!local)
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.web_enter(frame, true, cx));
            })
    };
    let reload = {
        let this = this.clone();
        BarButton::new("route-bar-reload", menu_icons::REFRESH, "Reload")
            .tooltip("Load the page again")
            .disabled(!local)
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.web_reload_frame(frame, cx));
            })
    };
    let full = {
        let this = this.clone();
        BarButton::new("route-bar-full-page", Lucide::UnfoldVertical, "Full Page")
            .toggle(data.fit)
            .tooltip(match data.fit {
                true => "Showing the whole page: click to keep this height",
                false => "Show the whole page, as tall as it is",
            })
            .disabled(!local)
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.web_toggle_full_page(frame, cx));
            })
    };
    let url = data.url.clone();
    let open = BarButton::new(
        "route-bar-browser",
        menu_icons::OPEN_BROWSER,
        "Open in Browser",
    )
    .tooltip(data.url.clone())
    .on_click(move |_, window, cx| {
        let _ = this.update(cx, |view, cx| view.open_in_browser(url.clone(), window, cx));
    });
    let bar = FloatBar::new("route-bar")
        .button(live)
        .button(reload)
        .button(full)
        .divider()
        .button(open);
    Some(bar.render(top_centre, canvas, window, cx))
}
