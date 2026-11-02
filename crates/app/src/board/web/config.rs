//! The web frames' knobs, read once from the environment: how pages settle, how many views are
//! kept, the snapshots' colour space and scale, and how the live view is scaled.

use crate::browser::engine::web_frame::Target;

/// How a frame's page settles before its snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::board) enum Settle {
    /// It stays hidden: timers run, animations don't.
    Hidden,
    /// Shown, but outside the host's bounds: clipped, in the window.
    OffCanvas,
    /// Shown, beyond the window's right edge.
    OffWindow,
}

/// The web frames' knobs, from the environment.
#[derive(Clone, Copy, Debug)]
pub(in crate::board) struct Config {
    /// `SAWIT_WEB_SETTLE`: hidden, offcanvas (default) or offwindow.
    pub settle: Settle,
    /// `SAWIT_WEB_SETTLE_MS`: the longest a page is given, after its load, to say it has settled
    /// (its fonts ready, its animations finished, two frames drawn) before it is snapshotted
    /// anyway (default 3000).
    pub settle_ms: u64,
    /// `SAWIT_WEB_POOL`: most web views kept (default 6); loads beyond it wait.
    pub pool: usize,
    /// `SAWIT_WEB_COLOR`: srgb (default: the snapshot's own bytes, which GPUI's layer shows as
    /// sRGB, as the live view does), or display (converted to the display's space: shown too
    /// pale, for comparison).
    pub target: Target,
    /// `SAWIT_WEB_SCALE`: snapshot at this many points per CSS px, whatever the zoom.
    pub scale: Option<f64>,
    /// `SAWIT_WEB_LIVE_LAG=1`: the live view moves a frame late, in step with GPUI's drawing. It
    /// hides while the canvas pans or zooms, so it is off.
    pub live_lag: bool,
    /// `SAWIT_WEB_LIVE_SCALE`: how the live view is scaled to the zoom (`Engine::set_view_scale`).
    pub live_scale: LiveScale,
}

/// How the live view is scaled to the canvas's zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::board) enum LiveScale {
    /// `viewscale` (default): WebKit's view scale with the layout size computed from it. The
    /// page lays out exactly as unscaled.
    ViewScale,
    /// `magnification`: the page lays out at the view's width in points.
    Magnification,
    /// `pagezoom`: text below 9 px after zoom is clamped to 9 px.
    PageZoom,
}

impl Config {
    pub(in crate::board) fn from_env() -> Config {
        let var = |name: &str| std::env::var(name).ok();
        Config {
            settle: match var("SAWIT_WEB_SETTLE").as_deref() {
                Some("hidden") => Settle::Hidden,
                Some("offwindow") => Settle::OffWindow,
                _ => Settle::OffCanvas,
            },
            settle_ms: var("SAWIT_WEB_SETTLE_MS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            pool: var("SAWIT_WEB_POOL")
                .and_then(|v| v.parse().ok())
                .unwrap_or(6)
                .max(1),
            target: match var("SAWIT_WEB_COLOR").as_deref() {
                Some("display") => Target::Display,
                _ => Target::Srgb,
            },
            scale: var("SAWIT_WEB_SCALE").and_then(|v| v.parse().ok()),
            live_lag: var("SAWIT_WEB_LIVE_LAG").is_some_and(|v| v == "1"),
            live_scale: match var("SAWIT_WEB_LIVE_SCALE").as_deref() {
                Some("pagezoom") => LiveScale::PageZoom,
                Some("magnification") => LiveScale::Magnification,
                _ => LiveScale::ViewScale,
            },
        }
    }
}
