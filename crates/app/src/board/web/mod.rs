//! Web frames on the canvas: UI screens, HTML pages kept in the project
//! ([`sawit_board::screens`]), and route frames, a dev server's pages on this Mac
//! ([`sawit_board::routes`]). A route frame of a page elsewhere is refused for now: it shows why,
//! and loads nothing.
//!
//! A screen is an ordinary FigCraft frame whose plugin data `"sawit.screen"` names its folder; a
//! URL frame's `"sawit.web"` holds `{"url": …, "viewport": {"w": …, "h": …}}`. At rest the canvas
//! paints each from a WebKit snapshot, a texture drawn between the rendered frame and the overlay
//! ([`bench::paint`]). Each frame has a web view of its own, built hidden as a child of the
//! window, in a clipping [`Host`] over the canvas ([`engines`]): once its page has loaded it
//! settles off the canvas, shown but outside the host's bounds so that animations run, until the
//! page says it is still (its fonts ready, its animations finished, two frames drawn), then it is
//! snapshotted and hidden again. A screen's folder is watched ([`watch`]): what an agent or the user
//! changes in it reloads the page, which settles and is snapshotted again.
//!
//! A route frame stays current by its page's own news ([`fresh`]): the page agent says its DOM
//! changed (a hot update) and the frame settles and is snapshotted again; a reload settles it too.
//! A frame that refreshes too often pauses a while ([`storm`]). Each frame's server is checked now
//! and then ([`servers`]): while it is stopped its frames keep their last picture and say so, and
//! they load again once it is back. Each says what it waits for over its picture ([`status`]).
//!
//! Double-click or Enter moves a frame's view onto it, sized node × zoom and scaled by zoom
//! (WebKit's view scale), and gives it the keyboard; Escape or a click outside snapshots it and
//! puts the texture back ([`live`]). Preview shows a screen full size, at a device's size
//! ([`preview`]).
//!
//! The `web` control op drives and measures it ([`ops`]).

mod bench;
mod config;
mod engines;
mod fresh;
mod live;
mod ops;
mod pool;
mod servers;
mod settle;
mod status;
mod storm;
#[cfg(test)]
mod tests;
mod watch;

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use figcraft_doc::{Document, Node, NodeId};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::*;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGColorSpace;
use sawit_board::{routes, screens};
use serde_json::Value;

use super::BoardView;
use crate::browser::engine::web_frame::{self, Host};
use crate::browser::engine::{Engine, EngineEvent, Frame as ViewFrame};
pub(super) use bench::{Painted, paint};
pub(super) use config::{Config, LiveScale, Settle};
pub(in crate::board) use settle::INIT;
pub(in crate::board) use status::{FrameStatus, frame_status};
#[cfg(test)]
pub(in crate::board) use watch::screens_of;

/// Most measurements the `web` op's status keeps.
const KEPT: usize = 400;

/// Where a web frame's page comes from.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) enum Source {
    /// A page of this Mac.
    Url(String),
    /// A page elsewhere, which isn't loaded ([`routes::refused`]).
    Remote(String),
    /// A screen's folder, by its name.
    Screen(String),
}

impl Source {
    /// The page's address, for a route frame.
    pub(in crate::board) fn url(&self) -> Option<&str> {
        match self {
            Source::Url(url) | Source::Remote(url) => Some(url),
            Source::Screen(_) => None,
        }
    }
}

/// What a web frame shows: its page, and the CSS viewport the page is laid out in.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Spec {
    pub source: Source,
    pub viewport: Viewport,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::board) struct Viewport {
    pub w: f64,
    pub h: f64,
}

/// The web frame `node` is, if it is one: a screen, or a route frame whose viewport is a point or
/// more, of a page on this Mac ([`routes::localhost`]) or, refused, of one elsewhere. A screen's
/// viewport is its frame's size, which [`web_nodes`] reads.
pub(in crate::board) fn spec(node: &Node) -> Option<Spec> {
    if let Some(screen) = screens::screen_of(node) {
        let made = screen.viewport;
        return Some(Spec {
            source: Source::Screen(screen.screen),
            viewport: Viewport {
                w: made.w,
                h: made.h,
            },
        });
    }
    let data = routes::web_of(node)?;
    let sized = data.viewport.w >= 1.0 && data.viewport.h >= 1.0;
    let source = match routes::localhost(&data.url) {
        true => Source::Url(data.url),
        false => Source::Remote(data.url),
    };
    sized.then_some(Spec {
        source,
        viewport: Viewport {
            w: data.viewport.w,
            h: data.viewport.h,
        },
    })
}

/// A web frame on the page on show: where it is on the page, and its corners.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct WebNode {
    pub id: NodeId,
    pub spec: Spec,
    /// Page coordinates: x0, y0, x1, y1.
    pub rect: [f64; 4],
    pub radii: [f64; 4],
}

/// The web frames on `page`, by id. A screen's viewport is its frame's size, in whole CSS px.
pub(in crate::board) fn web_nodes(doc: &Document, page: NodeId) -> Vec<WebNode> {
    let mut nodes: Vec<WebNode> = doc
        .nodes()
        .filter(|node| node.visible)
        .filter(|node| {
            let data = &node.plugin_data;
            data.contains_key(screens::SCREEN_KEY) || data.contains_key(routes::WEB_KEY)
        })
        .filter(|node| doc.page_of(node.id) == Some(page))
        .filter_map(|node| {
            let mut spec = spec(node)?;
            let r = doc.world_bounds(node.id)?;
            if matches!(spec.source, Source::Screen(_)) {
                spec.viewport = Viewport {
                    w: (r.x1 - r.x0).round().max(1.0),
                    h: (r.y1 - r.y0).round().max(1.0),
                };
            }
            Some(WebNode {
                id: node.id,
                spec,
                rect: [r.x0, r.y0, r.x1, r.y1],
                radii: node.corners.radii,
            })
        })
        .collect();
    nodes.sort_by_key(|node| node.id);
    nodes
}

/// What a frame's web view is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Phase {
    /// No web view.
    Cold,
    Loading,
    Settling,
    Snapping,
    Idle,
}

/// A snapshot on the GPU's side: the image, and the points per CSS px it was taken at.
pub(super) struct Texture {
    pub image: Arc<RenderImage>,
    pub scale: f64,
}

/// One web frame's view and texture.
pub(super) struct WebFrame {
    pub spec: Spec,
    pub engine: Option<Engine>,
    pub phase: Phase,
    pub texture: Option<Texture>,
    /// Last shown live or snapshotted: the pool lets go of the oldest view first.
    pub used: Instant,
    /// When the view was built, and when its load ended.
    pub built: Option<Instant>,
    pub loaded: Option<Instant>,
    /// Snapshots still to take back to back (the `snapshot` op's `repeat`).
    pub repeat: u32,
    /// The web content process.
    pub pid: i32,
    /// The page's `__probe`, as last read.
    pub probe: Option<Value>,
    /// A frame whose view went is to be built again: its texture is far from the zoom's density,
    /// or its page changed.
    pub refresh: bool,
    /// The settle the page is asked for: its answer names it, and an older one is stale.
    pub settle: u32,
    /// Its page changed while it loaded, settled or was snapshotted: it reloads once that's over.
    pub reload_after: bool,
    /// A screen has a page to show (`index.html`); a URL frame always does.
    pub has_page: bool,
    /// How often its page refreshed itself lately, and the pause that follows too many.
    pub storm: storm::Storm,
    /// A refresh it was due while paused, or while its server was stopped: it comes once that is
    /// over.
    pub owed: bool,
}

/// The frame live now.
pub(super) struct Live {
    pub id: NodeId,
    pub since: Instant,
    /// Where its view was put last, in the host, and its page zoom.
    pub placed: Option<(ViewFrame, f64)>,
    /// Where it goes next frame, with [`Config::live_lag`].
    pub next: Option<(ViewFrame, f64)>,
    /// Hidden while a menu is open over the canvas, or while the canvas pans or zooms.
    pub hidden: bool,
    /// The view of the canvas at the last prepaint, and when it last moved: the live view comes
    /// back once the canvas has been still a moment.
    pub view: Option<[f64; 7]>,
    pub moved: Option<Instant>,
    /// When a menu opened over it: it hides once its snapshot is in, so the menu is over a
    /// picture of the page as it is.
    pub menu_since: Option<Instant>,
    /// The selection before it went live, which comes back when it leaves.
    pub selection: Vec<NodeId>,
}

/// What the engines' handlers and the page tell the canvas.
pub(super) enum WebMsg {
    Engine(NodeId, EngineEvent),
    Probe(NodeId, String),
    /// Files of these screens changed on disk ([`watch`]).
    Changed(Vec<String>),
    /// Whether the server of these pages answers ([`servers`]).
    Server(String, bool),
    /// What Preview's view or page says.
    Preview(EngineEvent),
}

/// The canvas's web frames.
pub(super) struct WebFrames {
    pub config: Config,
    pub host: Option<Host>,
    pub frames: HashMap<NodeId, WebFrame>,
    pub to_view: UnboundedSender<WebMsg>,
    pub live: Option<Live>,
    /// A frame to make live once its view is built and has a texture.
    pub want_live: Option<NodeId>,
    /// The frame that left live mode: its view hides once its new texture is in.
    pub leaving: Option<Live>,
    /// Textures replaced since the last paint, to take out of the sprite atlas.
    pub stale: Vec<Arc<RenderImage>>,
    /// The canvas in the window, and the window's width, at the last prepaint.
    pub canvas: Bounds<Pixels>,
    pub window_width: f32,
    /// The canvas's zoom at the last prepaint, and whether a zoom gesture is on.
    pub zoom: f64,
    pub zooming: bool,
    pub space: Option<CFRetained<CGColorSpace>>,
    pub bench: Option<bench::Bench>,
    pub last_bench: Option<Value>,
    /// The paint's times of the last frame, for the bench: whole paint, web textures (ms).
    pub paint_times: Rc<Cell<(f64, f64)>>,
    /// Timings and readings, newest last.
    pub log: VecDeque<Value>,
    /// The canvas's screens folder, and what watches it once it is there.
    pub dir: PathBuf,
    pub watch: Option<watch::Watch>,
    pub watch_tried: Option<Instant>,
    /// Screens whose files changed, to reload together; whether that is on its way.
    pub dirty: HashSet<String>,
    pub flushing: bool,
    /// Screens Sawit itself just wrote and reloaded, with when the watcher's news of that write
    /// stops being taken as a change of its own.
    pub quiet: HashMap<String, Instant>,
    /// The route frames' servers, by their origin, and whether each answers ([`servers`]).
    pub servers: HashMap<String, servers::Server>,
    _messages: Task<()>,
}

impl WebFrames {
    /// The web frames of the canvas whose file is `canvas_file`: its screens are in the screens
    /// folder beside it ([`screens::screens_dir`]).
    pub(super) fn new(cx: &mut Context<BoardView>, canvas_file: &Path) -> WebFrames {
        let config = Config::from_env();
        log::info!("web frames: {config:?}");
        let (to_view, mut messages) = unbounded();
        let task = cx.spawn(async move |this, cx| {
            while let Some(message) = messages.next().await {
                if this
                    .update(cx, |this, cx| this.web_message(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        WebFrames {
            space: web_frame::color_space(config.target),
            config,
            host: None,
            frames: HashMap::new(),
            to_view,
            live: None,
            want_live: None,
            leaving: None,
            stale: Vec::new(),
            canvas: Bounds::default(),
            window_width: 0.0,
            zoom: 1.0,
            zooming: false,
            bench: None,
            last_bench: None,
            paint_times: Rc::default(),
            log: VecDeque::new(),
            dir: screens::screens_dir(&sawit_board::store::bound_file(canvas_file)),
            watch: None,
            watch_tried: None,
            dirty: HashSet::new(),
            flushing: false,
            quiet: HashMap::new(),
            servers: HashMap::new(),
            _messages: task,
        }
    }

    /// The folder of screen `name`.
    pub(super) fn folder(&self, name: &str) -> PathBuf {
        screens::folder(&self.dir, name)
    }

    /// The frames that show screen `name`.
    pub(super) fn showing(&self, name: &str) -> Vec<NodeId> {
        let shows = |frame: &WebFrame| frame.spec.source == Source::Screen(name.to_string());
        let mut ids: Vec<NodeId> = (self.frames.iter())
            .filter(|(_, frame)| shows(frame))
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids
    }

    /// Keeps `entry` among the readings, and logs it.
    pub(super) fn note(&mut self, entry: Value) {
        log::info!("webframe {entry}");
        if self.log.len() == KEPT {
            self.log.pop_front();
        }
        self.log.push_back(entry);
    }

    /// The canvas's tab went off screen or came back: the views go with it.
    pub(super) fn set_shown(&mut self, shown: bool) {
        if let Some(host) = &self.host {
            host.set_hidden(!shown);
        }
    }
}

/// Milliseconds, to a tenth.
pub(super) fn ms(duration: std::time::Duration) -> f64 {
    (duration.as_secs_f64() * 10_000.0).round() / 10.0
}
