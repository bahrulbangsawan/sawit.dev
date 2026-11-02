//! Route frames in the canvas's tab ([`sawit_board::routes`]): Add Routes from Dev Server, a
//! command of the File menu and the Actions palette, which finds the project's dev server and its
//! routes ([`find`]) and asks which to lay out, at which viewports ([`dialog`]); the bar over a
//! selected route frame, with Live, Reload and Open in Browser ([`bar`]); and agents'
//! `canvas_add_routes`, whose work shows on the canvas until its frames are pictured ([`ops`]).
//! How a route frame is rendered, kept current and made live is the web frames'
//! ([`super::web`]).

mod bar;
mod dialog;
mod find;
mod ops;

use std::time::Instant;

use figcraft_doc::NodeId;

pub(super) use bar::render_route_bar;
pub(super) use dialog::render_routes_dialog;
pub(in crate::board) use find::{Dialog, Step};

/// The routes' side of the canvas's tab.
#[derive(Default)]
pub(in crate::board) struct RoutesUi {
    /// Add Routes from Dev Server, while it is open.
    pub dialog: Option<Dialog>,
    /// Agents' routes works whose frames are being pictured: each is done once they all are.
    pub rendering: Vec<Rendering>,
    /// The canvas is drawn again in a moment for them.
    pub ticking: bool,
}

/// An agent's routes work waiting for its frames' first pictures.
#[derive(Clone, Debug)]
pub(in crate::board) struct Rendering {
    pub work: String,
    pub frames: Vec<NodeId>,
    /// When the canvas first drew it waiting: the frames have [`ops::RENDER_TIME`] from then.
    pub since: Option<Instant>,
}
