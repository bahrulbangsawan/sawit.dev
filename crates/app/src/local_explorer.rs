//! Cloudflare Local Explorer in the app: the explorers the Ports panel found running, shared with
//! the browser's toolbar (which offers the one its page's origin serves) and the Run panel, and
//! the Cloudflare logo every place that offers one shows. Finding them and calling them is
//! `sawit_core::cloudflare::explorer`'s.

use gpui_kit::*;
use sawit_core::cloudflare::explorer::{Explorer, PAGE_PATH};

/// The Cloudflare logo, bundled for the MCP page (assets/agent_logos).
pub const LOGO: &str = "agent_logos/cloudflare-color.svg";

/// The explorers running now, as the last Ports check found them. Every window's Ports panel
/// keeps it, from the same probe; the browsers watch it to show or hide their Explorer button.
#[derive(Default)]
pub struct Running(pub Vec<Explorer>);

impl Global for Running {}

/// Keeps `found` as the explorers running, telling those that watch only when it changed.
pub fn publish(found: &[Explorer], cx: &mut App) {
    if cx.default_global::<Running>().0 != found {
        cx.set_global(Running(found.to_vec()));
    }
}

/// The explorers running, as last published.
pub fn running(cx: &App) -> &[Explorer] {
    cx.try_global::<Running>().map_or(&[], |r| r.0.as_slice())
}

/// The Cloudflare logo at `size`.
pub fn logo(size: f32) -> Img {
    img(LOGO).size(px(size)).flex_none()
}

/// Where a page is, for the Explorer button: on a dev server on this Mac (`localhost`,
/// `127.0.0.1` or `[::1]`, over http), by its port, and whether it is Local Explorer itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    pub port: u16,
    pub on_explorer: bool,
}

/// `url`'s [`Origin`], when it is a page of a dev server on this Mac.
pub fn origin(url: &str) -> Option<Origin> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(ix) => (&rest[..ix], &rest[ix..]),
        None => (rest, ""),
    };
    let (host, port) = authority.rsplit_once(':')?;
    if !matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "[::1]"
    ) {
        return None;
    }
    let path = path.split(['?', '#']).next().unwrap_or_default();
    Some(Origin {
        port: port.parse().ok()?,
        on_explorer: path == PAGE_PATH || path.starts_with(&format!("{PAGE_PATH}/")),
    })
}

/// The explorer serving the page at `url`'s origin, with where the page is.
pub fn for_page<'a>(url: &str, cx: &'a App) -> Option<(&'a Explorer, Origin)> {
    let origin = origin(url)?;
    let explorer = running(cx).iter().find(|e| e.port == origin.port)?;
    Some((explorer, origin))
}

#[cfg(test)]
mod tests {
    use super::{Origin, origin};

    /// The button reads a page's origin: a dev server on this Mac by port, and whether the page
    /// is Local Explorer; other hosts, https and ports in a path aren't.
    #[test]
    fn a_pages_origin_is_read_from_its_url() {
        let at = |port, on_explorer| Some(Origin { port, on_explorer });
        assert_eq!(origin("http://localhost:8787/"), at(8787, false));
        assert_eq!(origin("http://localhost:5173"), at(5173, false));
        assert_eq!(
            origin("http://127.0.0.1:8787/cdn-cgi/local/explorer"),
            at(8787, true)
        );
        assert_eq!(
            origin("http://[::1]:5173/cdn-cgi/local/explorer/kv/CACHE?x=1"),
            at(5173, true)
        );
        assert_eq!(
            origin("http://localhost:8787/cdn-cgi/local/explorerx"),
            at(8787, false)
        );
        assert_eq!(
            origin("http://localhost:8787/?next=/cdn-cgi/local/explorer"),
            at(8787, false)
        );
        assert_eq!(origin("https://localhost:8787/"), None);
        assert_eq!(
            origin("http://example.com:8787/cdn-cgi/local/explorer"),
            None
        );
        assert_eq!(origin("http://localhost/"), None);
        assert_eq!(origin("about:blank"), None);
    }
}
