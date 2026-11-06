//! Full-page route frames ([`sawit_board::routes::WebData::fit`]), responsive and current.
//!
//! Each time a full-page frame's page settles (a load, a hot update, a resize, an undo that gave
//! the frame another height) it settles first in a view of the viewport's own height (900 for a
//! desktop), where its page is measured ([`WebFrames::measure_first`], then
//! [`BoardView::web_measured`]). The frame then takes the page's height
//! ([`sawit_board::routes::page_frame_height`]): it grows or shrinks, and the layers under it in
//! its section move with its bottom ([`sawit_board::ops::fit_to_page`], no undo step). Its view
//! takes the frame's size, settles again there, and is snapshotted. Measuring at the viewport's
//! height, never the frame's, is what keeps a page whose height follows its viewport's (`100vh`)
//! from growing the frame at every settle.
//!
//! A route frame resized (by the user, an agent, an undo) is laid out again where it is, its
//! page kept as it is, rather than reloaded ([`BoardView::web_relayout`]).
//!
//! The route bar's Full page turns it on or off ([`BoardView::web_toggle_full_page`]).

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::routes::{WEB_KEY, page_frame_height, web_of};
use serde_json::{Value, json};

use super::{BoardView, Phase, Viewport, WebFrames};

impl WebFrames {
    /// A settle of frame `id` begins: a full-page frame not live settles first in a view of its
    /// viewport's height, to be measured, unless it was just measured.
    pub(super) fn measure_first(&mut self, id: NodeId) {
        let live = self.live.as_ref().is_some_and(|live| live.id == id);
        let parking = self
            .frames
            .get(&id)
            .and_then(|frame| Some((frame.spec.viewport.w, frame.spec.fit?)))
            .map(|(w, h)| self.parking(Viewport { w, h }));
        let host = self.host.as_ref();
        let Some(frame) = self.frames.get_mut(&id) else {
            return;
        };
        frame.measuring = false;
        if std::mem::take(&mut frame.measured) || live {
            return;
        }
        let (Some(parking), Some(engine), Some(host)) = (parking, &frame.engine, host) else {
            return;
        };
        engine.place_in(host, parking);
        frame.measuring = true;
    }
}

impl BoardView {
    /// Full-page frame `id`'s page, settled in a view of its viewport's height (settle `token`),
    /// says how tall it is (`json`, the probe): the frame takes that height, unless the user is
    /// at work on the canvas (a drag, typing, Preview), when it keeps its own. Its view then takes
    /// the frame's size and settles again before its snapshot.
    pub(super) fn web_measured(
        &mut self,
        id: NodeId,
        token: u32,
        json: &str,
        cx: &mut Context<Self>,
    ) {
        let busy = self.editing_text()
            || self.preview_open()
            || self.board().is_none_or(|board| board.session.tool.busy());
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        if !frame.measuring || frame.phase != Phase::Settling || frame.settle != token {
            return;
        }
        frame.measuring = false;
        // WebKit hands back the string JSON.stringify made, itself as JSON.
        let text = serde_json::from_str::<String>(json).unwrap_or_else(|_| json.to_string());
        let probe = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        let page = probe.get("pageHeight").and_then(Value::as_f64);
        let (now, base) = (frame.spec.viewport, frame.spec.fit);
        let target = base
            .zip(page)
            .and_then(|(base, page)| page_frame_height(base, page))
            .filter(|target| (target - now.h).abs() >= 1.0 && !busy);
        web.note(
            json!({"event": "measured", "id": id.0, "page": page, "frame": now.h,
                        "fit_to": target}),
        );
        let mut height = now.h;
        if let Some(target) = target {
            let fitted = self.run(|board| sawit_board::ops::fit_to_page(board, id, target), cx);
            if fitted == Some(true) {
                height = target;
            }
        }
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let size = Viewport {
            w: now.w,
            h: height,
        };
        let parking = web.parking(size);
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        if height != now.h {
            frame.fitted_to = Some(size);
        }
        frame.measured = true;
        if let (Some(engine), Some(host)) = (&frame.engine, &web.host) {
            engine.place_in(host, parking);
        }
        self.web_resettle(id, "fit", cx);
        cx.notify();
    }

    /// Route frame `id` was resized: its view, at rest, takes the new size and its page settles
    /// again there (measured first when it is a full-page frame) and is snapshotted.
    pub(super) fn web_relayout(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let Some(viewport) = web.frames.get(&id).map(|frame| frame.spec.viewport) else {
            return;
        };
        let parking = web.parking(viewport);
        if let (Some(engine), Some(host)) = (
            web.frames.get(&id).and_then(|frame| frame.engine.as_ref()),
            &web.host,
        ) {
            engine.place_in(host, parking);
        }
        self.web_resettle(id, "resize", cx);
    }

    /// The route bar's Full page: frame `id` shows its whole page from now on, measured now, or
    /// stops doing so and keeps the height it has. One undo step.
    pub(in crate::board) fn web_toggle_full_page(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(ds) = self.board().and_then(|board| board.session.ds().ok()) else {
            return;
        };
        let Some(mut data) = ds.doc.node(id).and_then(web_of) else {
            return;
        };
        data.fit = !data.fit;
        let on = data.fit;
        let params = json!({"id": id, "props": {"pluginData": {(WEB_KEY): data.to_json()}}});
        self.choose("node.set", params, cx);
        if !on {
            return;
        }
        // The spec takes `fit` at the next paint; measure now, or once the view is at rest.
        let Some(frame) = self.web.as_mut().and_then(|web| web.frames.get_mut(&id)) else {
            return;
        };
        frame.spec.fit = Some(data.viewport.h);
        if frame.engine.is_some() && frame.phase == Phase::Idle {
            self.web_resettle(id, "full page", cx);
        } else {
            frame.relayout = true;
        }
    }
}
