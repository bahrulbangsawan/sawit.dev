//! What agents ask of routes while the app runs: `canvas_add_routes` begins a routes work (its
//! section, `begin {kind: "routes"}`), then sends the `routes` op with that work, which lays the
//! frames out in the section with the agent shown at it ([`BoardView::agent_routes_op`]). The work
//! then shows "Rendering" until each of its frames has its first picture, and is done; a frame
//! that hasn't one within [`RENDER_TIME`] of the canvas showing it fails the work, saying why.

use std::time::{Duration, Instant};

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_core::agents::work::WorkStatus;
use serde_json::Value;

use super::Rendering;
use crate::agents::Look;
use crate::board::BoardView;
use crate::board::web::{FrameStatus, frame_status, web_nodes};
use crate::workspace::Answer;

/// How long a routes work's frames have to be pictured once the canvas shows them.
pub(super) const RENDER_TIME: Duration = Duration::from_secs(45);
/// The step a routes work shows while its frames are pictured.
const RENDERING: &str = "Rendering";

impl BoardView {
    /// `look`'s agent's `routes` op: the frames are laid out with the agent shown at its work, and
    /// a work's frames are watched until they are pictured.
    pub(in crate::board) fn agent_routes_op(
        &mut self,
        op: Value,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> Answer {
        let reply = self.run_agent_op(&op, look, None, cx);
        let work = reply
            .get("work")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let (Some(work), true) = (work, reply.get("ok") == Some(&Value::Bool(true))) {
            let frames: Vec<NodeId> = (reply["frames"].as_array().into_iter().flatten())
                .filter_map(|f| f["id"].as_str()?.parse().ok())
                .collect();
            let now = cx.background_executor().now();
            self.agents
                .progressed(&work, Some(RENDERING.into()), None, now);
            self.routes_ui.rendering.retain(|r| r.work != work);
            self.routes_ui.rendering.push(Rendering {
                work,
                frames,
                since: None,
            });
            self.after_agents_changed(cx);
        }
        Answer::Now(reply)
    }

    /// Each frame drawn while routes works wait: a work whose frames all have their pictures is
    /// done; one whose time is up fails, naming what its frames wait for.
    pub(in crate::board) fn routes_rendered(&mut self, cx: &mut Context<Self>) {
        if self.routes_ui.rendering.is_empty() {
            return;
        }
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return;
        };
        let nodes = web_nodes(&ds.doc, ds.page);
        let Some(web) = self.web.as_ref() else {
            return;
        };
        let now = Instant::now();
        let mut finished: Vec<(String, WorkStatus)> = Vec::new();
        for rendering in self.routes_ui.rendering.iter_mut() {
            let mine: Vec<_> = (nodes.iter())
                .filter(|n| rendering.frames.contains(&n.id))
                .collect();
            if mine.is_empty() {
                continue;
            }
            let since = *rendering.since.get_or_insert(now);
            let waiting: Vec<FrameStatus> = (mine.iter())
                .filter_map(|node| frame_status(web, node, now))
                .collect();
            if waiting.iter().all(|s| *s == FrameStatus::Remote) {
                finished.push((rendering.work.clone(), WorkStatus::Done));
            } else if now.duration_since(since) >= RENDER_TIME {
                let why = match waiting.contains(&FrameStatus::Stopped) {
                    true => "its dev server stopped".to_string(),
                    false => format!("{} frames weren't pictured in time", waiting.len()),
                };
                finished.push((rendering.work.clone(), WorkStatus::Failed(why)));
            }
        }
        if finished.is_empty() {
            self.routes_wait(cx);
            return;
        }
        for (work, _) in &finished {
            self.routes_ui.rendering.retain(|r| &r.work != work);
        }
        // Said once this frame is painted: a redraw asked for while painting would be lost.
        let view = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = view.update(cx, |this, cx| {
                let now = cx.background_executor().now();
                for (work, status) in finished {
                    this.agents.progressed(&work, None, Some(status), now);
                }
                this.after_agents_changed(cx);
            });
        });
    }

    /// Draws the canvas again in a moment while routes works wait for their pictures, so a work
    /// whose time is up fails though nothing else changes.
    fn routes_wait(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.routes_ui.ticking, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                this.routes_ui.ticking = false;
                cx.notify();
            });
        })
        .detach();
    }
}
