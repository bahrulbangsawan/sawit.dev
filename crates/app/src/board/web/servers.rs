//! The route frames' servers: each origin on the page on show (`http://localhost:5173`) is checked
//! now and then with a connection off the UI thread ([`answers`]). A frame loads only once its
//! server answers. When the server stops, its frames keep their last picture and say "Server
//! stopped" ([`super::status`]); a view still waiting for its first load goes. When it is back,
//! their views are built anew, so each page loads, settles and is snapshotted again.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui_kit::*;
use sawit_core::browser::routes::answers;
use serde_json::json;

use super::{BoardView, Phase, Source, WebFrames, WebMsg};

/// How often a server that answers is checked, and one that doesn't.
const EVERY_UP: Duration = Duration::from_secs(2);
const EVERY_DOWN: Duration = Duration::from_millis(1000);
/// How long a check waits for the server to take the connection.
const WAIT: Duration = Duration::from_millis(400);

/// What is known of a server.
#[derive(Debug, Default)]
pub(in crate::board) struct Server {
    /// Whether it answered the last check; `None` before the first.
    pub up: Option<bool>,
    checking: bool,
    checked: Option<Instant>,
}

/// The origin of a page: `http://localhost:5173/changelog` is `http://localhost:5173`.
pub(in crate::board) fn origin(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    format!("{scheme}://{host}")
}

impl WebFrames {
    /// The servers of the route frames on the page that are due a check at `now`, marked as being
    /// checked; the servers no frame on the page uses any more are forgotten.
    pub(super) fn servers_due(&mut self, now: Instant) -> Vec<String> {
        let shown: HashSet<String> = (self.frames.values())
            .filter_map(|frame| match &frame.spec.source {
                Source::Url(url) => Some(origin(url)),
                _ => None,
            })
            .collect();
        self.servers.retain(|origin, _| shown.contains(origin));
        let mut due = Vec::new();
        for origin in shown {
            let server = self.servers.entry(origin.clone()).or_default();
            let every = if server.up == Some(true) {
                EVERY_UP
            } else {
                EVERY_DOWN
            };
            let ready = server
                .checked
                .is_none_or(|at| now.duration_since(at) >= every);
            if !server.checking && ready {
                server.checking = true;
                due.push(origin);
            }
        }
        due
    }

    /// Whether `source`'s page may load now: a route frame's once its server answers.
    pub(super) fn server_up(&self, source: &Source) -> bool {
        match source {
            Source::Url(url) => self.servers.get(&origin(url)).and_then(|s| s.up) == Some(true),
            _ => true,
        }
    }

    /// Whether `source`'s server was checked and doesn't answer.
    pub(in crate::board) fn server_down(&self, source: &Source) -> bool {
        match source {
            Source::Url(url) => self.servers.get(&origin(url)).and_then(|s| s.up) == Some(false),
            _ => false,
        }
    }

    /// The frames whose pages are on the server at `origin`.
    fn on_server(&self, origin_url: &str) -> Vec<figcraft_doc::NodeId> {
        (self.frames.iter())
            .filter(|(_, f)| f.spec.source.url().is_some_and(|u| origin(u) == origin_url))
            .filter(|(_, f)| matches!(f.spec.source, Source::Url(_)))
            .map(|(id, _)| *id)
            .collect()
    }
}

impl BoardView {
    /// Checks the servers `due` off the UI thread; each answer comes back as a message.
    pub(super) fn web_check_servers(&mut self, due: Vec<String>, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_ref() else {
            return;
        };
        for origin in due {
            let to_view = web.to_view.clone();
            cx.background_spawn(async move {
                let up = answers(&origin, WAIT);
                let _ = to_view.unbounded_send(WebMsg::Server(origin, up));
            })
            .detach();
        }
    }

    /// The server at `origin` answered (`up`) or didn't. A server that stopped leaves its frames
    /// as they are, with their pictures, but a view still waiting for its first load goes; one
    /// that is back has its frames' views built anew, all but the live one, which reloads by
    /// itself or by hand.
    pub(super) fn web_server(&mut self, origin: String, up: bool, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let live = web.live.as_ref().map(|live| live.id);
        let Some(server) = web.servers.get_mut(&origin) else {
            return;
        };
        let was = server.up.replace(up);
        server.checking = false;
        server.checked = Some(Instant::now());
        self.web_check_later(if up { EVERY_UP } else { EVERY_DOWN }, cx);
        if was == Some(up) {
            return;
        }
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let frames = web.on_server(&origin);
        for id in &frames {
            let Some(frame) = web.frames.get_mut(id) else {
                continue;
            };
            if up && was == Some(false) && Some(*id) != live {
                frame.engine = None;
                frame.phase = Phase::Cold;
                frame.refresh = true;
                frame.owed = false;
            } else if !up && frame.phase == Phase::Loading {
                frame.engine = None;
                frame.phase = Phase::Cold;
                frame.refresh = true;
            }
        }
        web.note(json!({"event": "server", "origin": origin, "up": up, "frames": frames.len()}));
        cx.notify();
    }

    /// Checks the servers due in `wait`, while the canvas is on screen; a canvas shown again
    /// checks them as it is drawn.
    fn web_check_later(&mut self, wait: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| {
                let due = match (this.shown, this.web.as_mut()) {
                    (true, Some(web)) => web.servers_due(Instant::now()),
                    _ => return,
                };
                this.web_check_servers(due, cx);
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::origin;

    #[test]
    fn a_page_is_on_its_origins_server() {
        assert_eq!(
            origin("http://localhost:5173/changelog?x=1"),
            "http://localhost:5173"
        );
        assert_eq!(origin("http://[::1]:3000"), "http://[::1]:3000");
        assert_eq!(origin("https://127.0.0.1/a#b"), "https://127.0.0.1");
    }
}
