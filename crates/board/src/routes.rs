//! Route frames: a dev server's pages, live on the canvas. Each is an ordinary FigCraft frame whose
//! plugin data [`WEB_KEY`] holds `{"url": …, "viewport": {"w": …, "h": …}, "route": …}`; the app
//! renders its page in WebKit, the way it renders a UI screen ([`crate::screens`]). Pages of this
//! Mac load (`localhost`, `127.0.0.1`, `[::1]`: [`localhost`]); any other is refused for now.
//!
//! Add Routes from Dev Server, and `canvas_add_routes`, lay a server's routes out in a section
//! named for the server: a row per route, a frame per viewport in each row ([`layout`]), each frame
//! named `<route> / <viewport>`.

use figcraft_doc::Node;
use serde::{Deserialize, Serialize};

use crate::screens::Viewport;

/// The plugin data key of a route frame (a URL frame).
pub const WEB_KEY: &str = "sawit.web";
/// The viewports' widths Add Routes lays each route out at when none are chosen.
pub const DEFAULT_WIDTHS: [f64; 3] = [1440.0, 768.0, 390.0];
/// Most routes one Add Routes lays out.
pub const MOST_ROUTES: usize = 24;
/// Most viewports one Add Routes lays each route out at.
pub const MOST_VIEWPORTS: usize = 6;
/// The narrowest and widest viewport.
pub const WIDTHS: std::ops::RangeInclusive<f64> = 200.0..=3840.0;

/// What a route frame holds in its plugin data ([`WEB_KEY`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebData {
    /// The page: `http://localhost:5173/changelog`.
    pub url: String,
    /// The CSS size it is laid out at.
    pub viewport: Viewport,
    /// The route it shows, `/changelog`, when Add Routes made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

impl WebData {
    /// The plugin data's value.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// The route frame `node` is, if it is one.
pub fn web_of(node: &Node) -> Option<WebData> {
    serde_json::from_str(node.plugin_data.get(WEB_KEY)?).ok()
}

/// Whether `url` is a page of this Mac: `http(s)://` on `localhost`, `127.0.0.1` or `[::1]`, any
/// port. A name before an `@` is never taken for the host.
pub fn localhost(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let name = match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "[::1]"
    )
}

/// What a frame of a page that isn't on this Mac says: remote pages aren't loaded yet.
pub fn refused(url: &str) -> String {
    let host = (url.split_once("://").map_or(url, |(_, rest)| rest))
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    format!(
        "{host} isn't on this Mac: route frames load localhost pages only (localhost, 127.0.0.1, \
         [::1]) for now"
    )
}

/// The viewport a page `w` CSS px wide is shown at: a desktop's 16:10 (1440 × 900), a tablet's
/// portrait 3:4 (768 × 1024), a phone's (390 × 844).
pub fn viewport_for(w: f64) -> Viewport {
    let w = w.round().clamp(*WIDTHS.start(), *WIDTHS.end());
    let h = if w >= 1024.0 {
        w * 0.625
    } else if w >= 600.0 {
        w * 4.0 / 3.0
    } else {
        w * 844.0 / 390.0
    };
    Viewport { w, h: h.round() }
}

/// A frame of [`layout`]: where it goes in the section (from its top-left), its viewport, and the
/// route it shows (an index into the routes laid out).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub x: f64,
    pub y: f64,
    pub viewport: Viewport,
    pub route: usize,
}

/// Room around the frames in a section (more above them, where the section's name is), between
/// frames in a row, and between rows: frames' names show above them.
pub const PADDING: f64 = 120.0;
pub const TOP: f64 = 480.0;
pub const COLUMN_GAP: f64 = 120.0;
pub const ROW_GAP: f64 = 200.0;

/// `routes` routes laid out at `viewports` in a section: a row per route, its frames left to right
/// in the order of `viewports`, top-aligned. The frames, and the section's size.
pub fn layout(routes: usize, viewports: &[Viewport]) -> (Vec<Placed>, (f64, f64)) {
    let row_width: f64 = viewports.iter().map(|v| v.w).sum::<f64>()
        + COLUMN_GAP * viewports.len().saturating_sub(1) as f64;
    let row_height = viewports.iter().map(|v| v.h).fold(0.0, f64::max);
    let mut placed = Vec::new();
    for route in 0..routes {
        let y = TOP + route as f64 * (row_height + ROW_GAP);
        let mut x = PADDING;
        for viewport in viewports {
            placed.push(Placed {
                x,
                y,
                viewport: *viewport,
                route,
            });
            x += viewport.w + COLUMN_GAP;
        }
    }
    let rows = routes.max(1) as f64;
    let height = TOP + PADDING + rows * row_height + (rows - 1.0) * ROW_GAP;
    (placed, (row_width + 2.0 * PADDING, height))
}

/// A route frame's name: `/changelog / 1440`.
pub fn frame_name(route: &str, viewport: Viewport) -> String {
    format!("{route} / {}", viewport.w)
}

/// A section's name for the server at `url`: its host and port, `localhost:5173`.
pub fn server_name(url: &str) -> String {
    (url.split_once("://").map_or(url, |(_, rest)| rest))
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(url)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pages_of_this_mac_load() {
        for url in [
            "http://localhost:5173/",
            "http://localhost:8765/anim.html?x=1",
            "http://127.0.0.1:3000",
            "https://localhost/",
            "http://localhost",
            "http://LOCALHOST:3000/",
            "http://[::1]:5173/changelog",
            "http://[::1]/",
        ] {
            assert!(localhost(url), "{url}");
        }
        for url in [
            "https://example.com",
            "http://localhost.evil.com:80/",
            "http://evil.com/?localhost:1",
            "http://localhost@evil.com/",
            "http://evil.com@localhost:3000/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://127.0.0.2:80/",
            "http://[::2]:80/",
            "http://0.0.0.0:3000/",
        ] {
            assert!(!localhost(url), "{url}");
        }
        assert!(refused("https://example.com/x").starts_with("example.com isn't on this Mac"));
    }

    #[test]
    fn viewports_take_their_devices_heights() {
        let size = |w: f64| (viewport_for(w).w, viewport_for(w).h);
        assert_eq!(size(1440.0), (1440.0, 900.0));
        assert_eq!(size(768.0), (768.0, 1024.0));
        assert_eq!(size(390.0), (390.0, 844.0));
        assert_eq!(size(10.0).0, 200.0);
    }

    #[test]
    fn routes_go_in_rows_of_their_viewports() {
        let viewports: Vec<Viewport> = DEFAULT_WIDTHS.iter().map(|w| viewport_for(*w)).collect();
        let (placed, (w, h)) = layout(2, &viewports);
        assert_eq!(placed.len(), 6);
        // The first row: 1440, 768 and 390 side by side.
        let row: Vec<(f64, f64)> = placed[..3].iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(row, [(120.0, 480.0), (1680.0, 480.0), (2568.0, 480.0)]);
        // The second row is under the tallest of the first (768 × 1024).
        assert_eq!(placed[3].y, 480.0 + 1024.0 + 200.0);
        assert_eq!(placed[5].route, 1);
        assert_eq!(w, 1440.0 + 768.0 + 390.0 + 2.0 * 120.0 + 2.0 * 120.0);
        assert_eq!(h, 480.0 + 120.0 + 2.0 * 1024.0 + 200.0);
        assert_eq!(frame_name("/changelog", viewports[0]), "/changelog / 1440");
        assert_eq!(server_name("http://localhost:5173/x"), "localhost:5173");
    }
}
