//! What a route frame waits for, said in a pill over its top-left corner: its first picture
//! ("Loading…"), its server ("Server stopped", its last picture kept), the end of a pause after too
//! many refreshes ("Paused: reloading too often"), or nothing at all for a page that isn't on this
//! Mac. A frame with nothing to wait for shows no pill, and neither does the live one.

use std::time::Instant;

use gpui_kit::component::*;
use gpui_kit::*;

use super::{BoardView, Source, WebFrames, WebNode, web_nodes};

/// The pill's height, and how far in from the frame's corner it sits.
const HEIGHT: f32 = 22.0;
const INSET: f32 = 8.0;
/// The narrowest frame on screen that shows a pill.
const NARROWEST: f64 = 56.0;

/// What a route frame waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::board) enum FrameStatus {
    /// Its page hasn't been pictured yet.
    Loading,
    /// Its server doesn't answer: its last picture stays.
    Stopped,
    /// It refreshed too often, and waits out a pause.
    Paused,
    /// Its page isn't on this Mac, so it isn't loaded.
    Remote,
}

impl FrameStatus {
    pub(in crate::board) fn text(self) -> &'static str {
        match self {
            FrameStatus::Loading => "Loading…",
            FrameStatus::Stopped => "Server stopped",
            FrameStatus::Paused => "Paused: reloading too often",
            FrameStatus::Remote => "Not loaded: localhost pages only, for now",
        }
    }
}

/// What route frame `node` waits for at `now`, if anything.
pub(in crate::board) fn frame_status(
    web: &WebFrames,
    node: &WebNode,
    now: Instant,
) -> Option<FrameStatus> {
    let frame = web.frames.get(&node.id);
    match &node.spec.source {
        Source::Screen(_) => None,
        Source::Remote(_) => Some(FrameStatus::Remote),
        Source::Url(_) if web.server_down(&node.spec.source) => Some(FrameStatus::Stopped),
        Source::Url(_) if frame.is_some_and(|f| f.storm.paused(now)) => Some(FrameStatus::Paused),
        Source::Url(_) if frame.is_none_or(|f| f.texture.is_none()) => Some(FrameStatus::Loading),
        Source::Url(_) => None,
    }
}

impl BoardView {
    /// The pills of the route frames on screen that wait for something, placed over the canvas.
    pub(in crate::board) fn render_web_status(&self, cx: &App) -> Vec<AnyElement> {
        let (Some(web), Some(ds)) = (
            self.web.as_ref(),
            self.board().and_then(|b| b.session.ds().ok()),
        ) else {
            return Vec::new();
        };
        let live = web.live.as_ref().map(|live| live.id);
        let now = Instant::now();
        let view = &ds.view;
        let theme = cx.theme();
        (web_nodes(&ds.doc, ds.page).iter())
            .filter(|node| Some(node.id) != live)
            .filter_map(|node| {
                let status = frame_status(web, node, now)?;
                let (x0, y0) = view.page_to_screen(node.rect[0], node.rect[1]);
                let (x1, y1) = view.page_to_screen(node.rect[2], node.rect[3]);
                let shown = x1 > 0.0 && y1 > 0.0 && x0 < view.vw && y0 < view.vh;
                if !shown || x1 - x0 < NARROWEST || y1 - y0 < f64::from(HEIGHT) + 16.0 {
                    return None;
                }
                let dot = match status {
                    FrameStatus::Loading => theme.muted_foreground,
                    FrameStatus::Stopped | FrameStatus::Paused => theme.warning,
                    FrameStatus::Remote => theme.danger,
                };
                let pill = h_flex()
                    .id(("web-status", node.id.0))
                    .debug_selector(move || format!("web-status-{}", node.id.0))
                    .absolute()
                    .left(px(x0 as f32 + INSET))
                    .top(px(y0 as f32 + INSET))
                    .max_w(px((x1 - x0) as f32 - 2.0 * INSET))
                    .h(px(HEIGHT))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .shadow_sm()
                    .text_xs()
                    .child(div().flex_none().size(px(7.)).rounded_full().bg(dot))
                    .child(div().min_w_0().truncate().child(status.text()));
                Some(pill.into_any_element())
            })
            .collect()
    }
}
