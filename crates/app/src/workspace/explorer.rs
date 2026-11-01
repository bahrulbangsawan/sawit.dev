//! ⌃⌘E (`workspace: open local explorer`): the active project's Cloudflare Local Explorer in its
//! browser tab, or the Run panel, which says how to start the dev server that serves one.

use sawit_core::cloudflare::explorer::{Explorer, running_in};

use super::*;

/// Which explorer ⌃⌘E opens: of the project's dev servers', the one its browser shows a page of,
/// else the one on the lowest port; with no project, or none of its own running, the only one
/// running on this Mac.
pub(super) fn pick<'a>(
    explorers: &'a [Explorer],
    root: Option<&Path>,
    browser_port: Option<u16>,
) -> Option<&'a Explorer> {
    let mine = root.map(|r| running_in(explorers, r)).unwrap_or_default();
    if let Some(shown) = mine.iter().find(|e| Some(e.port) == browser_port) {
        return Some(shown);
    }
    match (mine.first(), explorers) {
        (Some(first), _) => Some(first),
        (None, [only]) => Some(only),
        _ => None,
    }
}

impl Workspace {
    /// Opens the active project's Local Explorer in its browser tab ([`pick`]); when none runs,
    /// shows the Run panel, whose Local Explorer section starts the dev server or says what the
    /// project's Wrangler or Vite plugin needs.
    pub(super) fn open_local_explorer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.root(cx);
        let browser_port = self
            .browser_tab(root.as_deref())
            .and_then(|(_, b)| {
                let url = b.read(cx).describe()["url"].as_str()?.to_string();
                crate::local_explorer::origin(&url)
            })
            .map(|o| o.port);
        let explorers = self.ports_panel.read(cx).explorers();
        match pick(explorers, root.as_deref(), browser_port).map(Explorer::url) {
            Some(url) => {
                self.open_browser(Some(url), window, cx);
            }
            None => self.show_panel(SidebarTab::Run, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sawit_core::cloudflare::explorer::Explorer;

    use super::pick;

    fn explorer(port: u16, cwd: &str) -> Explorer {
        Explorer {
            port,
            addr: format!("127.0.0.1:{port}").parse().unwrap(),
            pid: u32::from(port),
            label: "wrangler dev".into(),
            cwd: Some(cwd.into()),
            spec: sawit_core::cloudflare::explorer::parse_spec(
                br#"{"openapi":"3.0.3","paths":{}}"#,
            )
            .unwrap(),
        }
    }

    /// ⌃⌘E opens the project's own explorer, the one its browser is on when it has several, and
    /// another project's only when it is the only one running.
    #[test]
    fn the_key_opens_the_projects_own_explorer() {
        let shop = Path::new("/w/shop");
        let all = [
            explorer(5173, "/w/shop/web"),
            explorer(8787, "/w/shop/api"),
            explorer(8790, "/w/blog"),
        ];
        let port = |found: Option<&Explorer>| found.map(|e| e.port);
        assert_eq!(port(pick(&all, Some(shop), None)), Some(5173));
        assert_eq!(port(pick(&all, Some(shop), Some(8787))), Some(8787));
        assert_eq!(port(pick(&all, Some(shop), Some(8790))), Some(5173));
        assert_eq!(port(pick(&all, Some(Path::new("/w/notes")), None)), None);
        assert_eq!(port(pick(&all, None, None)), None);
        let blog_only = [explorer(8790, "/w/blog")];
        assert_eq!(port(pick(&blog_only, Some(shop), None)), Some(8790));
        assert_eq!(port(pick(&[], Some(shop), None)), None);
    }
}
