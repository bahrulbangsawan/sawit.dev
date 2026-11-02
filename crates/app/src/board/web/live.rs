//! Live mode: one frame's own view moved onto it, node × zoom and scaled by zoom with WebKit's view
//! scale ([`Engine::set_view_scale`](crate::browser::engine::Engine::set_view_scale)) so the page
//! keeps its CSS viewport and its layout, given the keyboard. Double-click or Enter on a web frame
//! enters it; Escape (in the page or on the canvas) or a press on the canvas leaves, after a
//! snapshot at the live size, which goes back under the view before it hides.
//!
//! The native view draws above GPUI, so it steps aside where GPUI must show: while the canvas pans
//! or zooms it hides, the frame's latest picture showing in its place, and comes back once the
//! canvas has been still for [`STILL`]; while a menu is open over the canvas it hides too, once a
//! snapshot of the page as it is has come in (or after [`MENU_WAIT`]). The frame's selection, and
//! with it its handles, go while it is live, and so does its name.

use std::time::{Duration, Instant};

use figcraft_doc::NodeId;
use figcraft_engine::ViewState;
use gpui_kit::*;
use serde_json::json;

use super::{BoardView, Live, LiveScale, Phase, ViewFrame, WebFrames, WebNode, ms, web_nodes};

/// How long the canvas stays still before the live view comes back after a pan or a zoom.
pub(super) const STILL: Duration = Duration::from_millis(140);
/// The longest a menu waits for the live page's picture before the view hides under it.
pub(super) const MENU_WAIT: Duration = Duration::from_millis(150);

/// What placing the live view asks of the canvas.
#[derive(Default)]
pub(super) struct Placed {
    /// Draw the next frame too: the view moves a frame late ([`super::Config::live_lag`]).
    pub again: bool,
    /// Snapshot this frame now: a menu opened over it.
    pub snap_for_menu: Option<NodeId>,
    /// Draw again in this long: the view comes back once the canvas is still.
    pub wake_in: Option<Duration>,
}

/// Where a live view goes in the host (the canvas), and its page zoom: the node's box on screen,
/// at the zoom that lays the page out at its viewport's width.
pub(super) fn live_place(node: &WebNode, view: &ViewState) -> (ViewFrame, f64) {
    let (x0, y0) = view.page_to_screen(node.rect[0], node.rect[1]);
    let (x1, y1) = view.page_to_screen(node.rect[2], node.rect[3]);
    let frame = ViewFrame {
        x: x0 as f32,
        y: y0 as f32,
        width: (x1 - x0) as f32,
        height: (y1 - y0) as f32,
    };
    (frame, (x1 - x0) / node.spec.viewport.w)
}

/// What says the canvas moved: its view's centre and zoom, and where it is in the window.
fn view_key(view: &ViewState, bounds: Bounds<Pixels>) -> [f64; 7] {
    [
        view.cx,
        view.cy,
        view.zoom,
        f64::from(bounds.origin.x),
        f64::from(bounds.origin.y),
        f64::from(bounds.size.width),
        f64::from(bounds.size.height),
    ]
}

impl WebFrames {
    /// Puts the live view on its frame as the view is now: hidden while the canvas moves, and back
    /// once it has been still for [`STILL`]; hidden under a menu once its picture is in.
    pub(super) fn place_live(
        &mut self,
        nodes: &[WebNode],
        view: &ViewState,
        bounds: Bounds<Pixels>,
        menus: bool,
        now: Instant,
    ) -> Placed {
        let (lag, scale) = (self.config.live_lag, self.config.live_scale);
        let Some(live) = self.live.as_mut() else {
            return Placed::default();
        };
        let Some(node) = nodes.iter().find(|node| node.id == live.id) else {
            self.live = None;
            return Placed::default();
        };
        let (Some(engine), Some(host)) = (
            self.frames
                .get(&live.id)
                .and_then(|frame| frame.engine.as_ref()),
            self.host.as_ref(),
        ) else {
            return Placed::default();
        };
        let mut placed = Placed::default();
        let key = view_key(view, bounds);
        if live.view.is_some_and(|was| was != key) {
            live.moved = Some(now);
        }
        live.view = Some(key);
        let moving = live.moved.filter(|at| now.duration_since(*at) < STILL);
        if let Some(at) = moving {
            placed.wake_in = Some(STILL.saturating_sub(now.duration_since(at)));
        }
        // A menu: snapshot first, hide once the picture is in or the wait is over.
        let menu_ready = match (menus, live.menu_since) {
            (false, _) => {
                live.menu_since = None;
                false
            }
            (true, None) => {
                live.menu_since = Some(now);
                placed.snap_for_menu = Some(live.id);
                placed.wake_in = Some(MENU_WAIT);
                false
            }
            (true, Some(since)) => now.duration_since(since) >= MENU_WAIT,
        };
        let hide = moving.is_some() || menu_ready;
        if !hide {
            let target = live_place(node, view);
            let (frame, zoom) = match lag {
                true => live.next.replace(target).unwrap_or(target),
                false => target,
            };
            if live.placed.map(|(_, z)| z) != Some(zoom) {
                match scale {
                    LiveScale::ViewScale => {
                        if !engine.set_view_scale(zoom) {
                            engine.set_zoom(zoom);
                        }
                    }
                    LiveScale::Magnification => engine.set_magnification(zoom),
                    LiveScale::PageZoom => engine.set_zoom(zoom),
                }
            }
            if live.placed != Some((frame, zoom)) {
                engine.place_in(host, frame);
                live.placed = Some((frame, zoom));
            }
            placed.again = lag && live.next != live.placed;
        }
        if live.hidden != hide {
            engine.set_visible(!hide);
            if !hide {
                engine.focus();
            }
            live.hidden = hide;
        }
        placed
    }
}

impl BoardView {
    /// Makes frame `id` live. A frame whose view isn't built or settled yet becomes live once it
    /// has its texture. What happened, for the `web` op.
    pub(in crate::board) fn web_enter(
        &mut self,
        id: NodeId,
        cx: &mut Context<Self>,
    ) -> Result<&'static str, String> {
        if self
            .web
            .as_ref()
            .and_then(|web| web.live.as_ref())
            .is_some_and(|live| live.id == id)
        {
            return Ok("already live");
        }
        self.web_exit(cx);
        let selection = (self.board().and_then(|board| board.session.ds().ok()))
            .map(|ds| ds.selection.clone())
            .unwrap_or_default();
        let Some(web) = self.web.as_mut() else {
            return Err("web frames are off".into());
        };
        let Some(frame) = web.frames.get_mut(&id) else {
            return Err(format!("{id} is not a web frame on this page"));
        };
        let state = match (&frame.engine, frame.phase) {
            (Some(engine), Phase::Idle) => {
                engine.set_visible(true);
                engine.focus();
                frame.used = Instant::now();
                web.live = Some(Live {
                    id,
                    since: Instant::now(),
                    placed: None,
                    next: None,
                    hidden: false,
                    view: None,
                    moved: None,
                    menu_since: None,
                    selection,
                });
                web.want_live = None;
                "live"
            }
            (Some(_), _) => {
                web.want_live = Some(id);
                "waiting for its snapshot"
            }
            (None, _) => {
                frame.phase = Phase::Cold;
                frame.refresh = true;
                web.want_live = Some(id);
                "building its view"
            }
        };
        web.note(json!({"event": "enter", "id": id.0, "state": state}));
        if state == "live" {
            // Its handles would sit half under the view: the selection goes while it is live.
            if let Ok(board) = self.board.as_mut() {
                board.session.set_selection(Vec::new());
            }
        }
        cx.notify();
        Ok(state)
    }

    /// Leaves live mode: the keyboard goes back to the canvas, the selection comes back, and the
    /// view is snapshotted at its live size; it hides once that texture is in.
    pub(in crate::board) fn web_exit(&mut self, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        web.want_live = None;
        let Some(live) = web.live.take() else {
            return;
        };
        if let Some(engine) = web
            .frames
            .get(&live.id)
            .and_then(|frame| frame.engine.as_ref())
        {
            engine.focus_parent();
        }
        let id = live.id;
        web.note(json!({"event": "exit", "id": id.0, "live_ms": ms(live.since.elapsed())}));
        let selection = live.selection.clone();
        web.leaving = Some(live);
        if let Ok(board) = self.board.as_mut() {
            let doc = board.session.ds().ok().map(|ds| ds.doc.clone());
            let kept: Vec<NodeId> = (selection.into_iter())
                .filter(|id| doc.as_ref().is_some_and(|doc| doc.node(*id).is_some()))
                .collect();
            board.session.set_selection(kept);
        }
        self.web_snapshot(id, "leave", cx);
        cx.notify();
    }

    /// The frame live now, if one is.
    pub(in crate::board) fn web_live(&self) -> Option<NodeId> {
        self.web.as_ref()?.live.as_ref().map(|live| live.id)
    }

    /// A key on the canvas: Escape leaves live mode, Enter makes the one web frame selected live.
    /// Whether the key was taken.
    pub(in crate::board) fn web_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(web) = self.web.as_ref() else {
            return false;
        };
        match key {
            "escape" if web.live.is_some() => {
                self.web_exit(cx);
                true
            }
            "enter" if web.live.is_none() && !self.editing_text() => {
                let selected = self
                    .board()
                    .and_then(|board| board.session.ds().ok())
                    .and_then(|ds| match ds.selection.as_slice() {
                        [one] => Some(*one),
                        _ => None,
                    });
                match selected.filter(|id| web.frames.contains_key(id)) {
                    Some(id) => self.web_enter(id, cx).is_ok(),
                    None => false,
                }
            }
            _ => false,
        }
    }

    /// A press on the canvas: one outside the live view (the view takes its own) leaves live mode;
    /// a double-click on a web frame enters it. Whether the press was taken.
    pub(in crate::board) fn web_press(
        &mut self,
        position: Point<Pixels>,
        clicks: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.web.is_none() {
            return false;
        }
        self.web_exit(cx);
        if clicks != 2 {
            return false;
        }
        let local = position - self.bounds.origin;
        let Some(ds) = self.board().and_then(|board| board.session.ds().ok()) else {
            return false;
        };
        let (x, y) = ds
            .view
            .screen_to_page(f64::from(local.x), f64::from(local.y));
        let hit = web_nodes(&ds.doc, ds.page).into_iter().rev().find(|node| {
            x >= node.rect[0] && x <= node.rect[2] && y >= node.rect[1] && y <= node.rect[3]
        });
        match hit {
            Some(node) => self.web_enter(node.id, cx).is_ok(),
            None => false,
        }
    }
}
