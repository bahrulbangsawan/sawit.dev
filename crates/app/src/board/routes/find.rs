//! Add Routes from Dev Server, behind its dialog: the project's dev servers, as the Ports panel
//! finds them ([`sawit_core::browser::dev_server_list`]); the chosen one's routes, from its app's
//! files and its links ([`sawit_core::browser::routes::discover`]), both off the UI thread; and the
//! routes ticked laid out at the viewports chosen, in a section the view then shows.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui_kit::*;
use sawit_board::routes::{DEFAULT_WIDTHS, MOST_ROUTES};
use sawit_core::browser::routes::{CRAWL, Found, discover};
use sawit_core::browser::{DevServer, dev_server_list};
use sawit_core::probe;
use serde_json::{Value, json};

use crate::board::BoardView;
use crate::rt;

/// The widths the dialog offers; [`DEFAULT_WIDTHS`] are ticked at first.
pub(super) const WIDTHS: [f64; 6] = [1440.0, 1280.0, 1024.0, 768.0, 430.0, 390.0];

/// Where the dialog is.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) enum Step {
    /// Looking for the project's dev servers.
    Servers,
    /// None runs in this folder.
    NoServer(PathBuf),
    /// Finding the chosen server's routes.
    Routes,
    /// The routes are found: they are ticked to be added.
    Ready,
}

/// Add Routes from Dev Server's dialog.
pub(in crate::board) struct Dialog {
    pub step: Step,
    pub servers: Vec<DevServer>,
    /// The chosen server, in `servers`.
    pub server: usize,
    pub found: Option<Found>,
    /// The routes found that are unticked.
    pub left_out: HashSet<String>,
    /// The widths offered, each ticked or not.
    pub widths: Vec<(f64, bool)>,
    pub focus: FocusHandle,
    /// It takes the keyboard with the next frame it is drawn in.
    pub focus_pending: bool,
}

impl Dialog {
    /// The routes ticked, in the order found, at most [`MOST_ROUTES`].
    pub(in crate::board) fn chosen(&self) -> Vec<String> {
        let found = self.found.iter().flat_map(|found| &found.routes);
        (found.map(|r| r.path.clone()))
            .filter(|path| !self.left_out.contains(path))
            .take(MOST_ROUTES)
            .collect()
    }

    /// The widths ticked.
    pub(in crate::board) fn chosen_widths(&self) -> Vec<f64> {
        (self.widths.iter())
            .filter(|(_, on)| *on)
            .map(|(w, _)| *w)
            .collect()
    }
}

impl BoardView {
    /// Opens Add Routes from Dev Server and starts looking for the project's dev servers (every
    /// dev server, for the canvas of no project). It takes the keyboard with the first frame it is
    /// drawn in, as the menus do, so an op can open it too.
    pub(in crate::board) fn open_routes_dialog(&mut self, cx: &mut Context<Self>) {
        self.close_menus(cx);
        let focus = cx.focus_handle();
        self.routes_ui.dialog = Some(Dialog {
            focus_pending: true,
            step: Step::Servers,
            servers: Vec::new(),
            server: 0,
            found: None,
            left_out: HashSet::new(),
            widths: WIDTHS.map(|w| (w, DEFAULT_WIDTHS.contains(&w))).to_vec(),
            focus,
        });
        self.find_servers(cx);
        cx.notify();
    }

    /// Looks for the project's dev servers again.
    pub(super) fn find_servers(&mut self, cx: &mut Context<Self>) {
        let folder = self.canvas_project().unwrap_or_else(|| PathBuf::from("/"));
        if let Some(dialog) = self.routes_ui.dialog.as_mut() {
            dialog.step = Step::Servers;
        }
        let jobs = rt::spawn(cx, probe::listening_jobs());
        cx.spawn(async move |this, cx| {
            let jobs = jobs.await.ok().and_then(Result::ok).unwrap_or_default();
            let servers = dev_server_list(&jobs, &folder);
            let _ = this.update(cx, |this, cx| {
                let Some(dialog) = this.routes_ui.dialog.as_mut() else {
                    return;
                };
                dialog.servers = servers;
                if dialog.servers.is_empty() {
                    dialog.step = Step::NoServer(folder);
                    cx.notify();
                } else {
                    this.find_routes(0, cx);
                }
            });
        })
        .detach();
    }

    /// Finds the routes of dev server `index`, off the UI thread.
    pub(super) fn find_routes(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(dialog) = self.routes_ui.dialog.as_mut() else {
            return;
        };
        let Some(server) = dialog.servers.get(index).cloned() else {
            return;
        };
        dialog.server = index;
        dialog.step = Step::Routes;
        dialog.found = None;
        dialog.left_out.clear();
        let found = rt::spawn(cx, async move {
            discover(&server.folder, &server.url, &CRAWL).await
        });
        cx.spawn(async move |this, cx| {
            let found = found.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                let Some(dialog) = this.routes_ui.dialog.as_mut() else {
                    return;
                };
                if dialog.server == index && dialog.step == Step::Routes {
                    dialog.found = Some(found);
                    dialog.step = Step::Ready;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Closes the dialog; the canvas takes the keyboard back.
    pub(in crate::board) fn close_routes_dialog(&mut self, cx: &mut Context<Self>) {
        if self.routes_ui.dialog.take().is_some() {
            self.take_keyboard(cx);
        }
    }

    /// Lays the ticked routes out at the ticked widths in a section beside what the page holds,
    /// in one undo step, then shows it and says what was added.
    pub(in crate::board) fn add_chosen_routes(&mut self, cx: &mut Context<Self>) -> Value {
        let Some(dialog) = self.routes_ui.dialog.as_ref() else {
            return json!({"ok": false, "error": "Add Routes isn't open"});
        };
        let Some(server) = dialog.servers.get(dialog.server) else {
            return json!({"ok": false, "error": "no dev server"});
        };
        let (routes, widths) = (dialog.chosen(), dialog.chosen_widths());
        if dialog.step != Step::Ready || routes.is_empty() || widths.is_empty() {
            return json!({"ok": false, "error": "nothing to add yet"});
        }
        let skipped = dialog.found.as_ref().map_or(0, |f| f.skipped.len());
        let op =
            json!({"op": "routes", "server": server.url, "routes": routes, "viewports": widths});
        let reply = self.run_op(&op, cx);
        self.close_routes_dialog(cx);
        if reply.get("ok") != Some(&Value::Bool(true)) {
            let error = reply["error"]
                .as_str()
                .unwrap_or("the routes couldn't be added");
            self.show_toast(error.to_string(), cx);
            return reply;
        }
        if let Some(section) = reply["section"].as_str().and_then(|s| s.parse().ok()) {
            self.show_node(section, cx);
        }
        let frames = reply["frames"].as_array().map_or(0, Vec::len);
        let mut said = format!(
            "Added {frames} frames for {} {} of {}",
            routes.len(),
            if routes.len() == 1 { "route" } else { "routes" },
            reply["name"].as_str().unwrap_or("the dev server"),
        );
        if skipped > 0 {
            said.push_str(&format!(
                "; {skipped} dynamic {} had no sample",
                if skipped == 1 { "route" } else { "routes" }
            ));
        }
        self.show_toast(said, cx);
        reply
    }

    /// Fits node `id` in the view, no closer than 100%.
    fn show_node(&mut self, id: figcraft_doc::NodeId, cx: &mut Context<Self>) {
        let Some(ds) = (self.board.as_mut().ok()).and_then(|b| b.session.ds_mut().ok()) else {
            return;
        };
        if let Some(b) = ds.doc.world_bounds(id) {
            // Room around it, and more under it, where the toolbar floats.
            let (w, h) = (b.x1 - b.x0, b.y1 - b.y0);
            let mut room = b;
            (room.x0, room.x1) = (b.x0 - w * 0.04, b.x1 + w * 0.04);
            (room.y0, room.y1) = (b.y0 - h * 0.04, b.y1 + h * 0.18);
            ds.view.fit(room, 1.0);
        }
        cx.notify();
    }
}
