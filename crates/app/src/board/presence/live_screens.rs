//! Screens built live (30e): an agent's write to a screen reloads its page; once the page has
//! settled its blueprint comes back ([`replay::blueprint`]), and what changed since the last one
//! plays over the screen's frame in the writer's lane, ending on the live page.

use std::time::{Duration, Instant};

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::screens;

use super::Presence;
use super::replay::blueprint::{self, Blueprint};
use crate::agents::Look;
use crate::board::BoardView;

/// How long after a write its screen's next blueprint still plays as the writer's.
const WRITTEN_WITHIN: Duration = Duration::from_secs(30);

impl Presence {
    /// `look`'s agent wrote screen `name` at `now`.
    pub fn screen_written(&mut self, name: &str, look: &Look, now: Instant) {
        self.screen_writers
            .insert(name.to_string(), (look.clone(), now));
    }

    /// Who wrote screen `name` in the last [`WRITTEN_WITHIN`].
    pub fn screen_writer(&self, name: &str, now: Instant) -> Option<&Look> {
        let (look, at) = self.screen_writers.get(name)?;
        (now.saturating_duration_since(*at) < WRITTEN_WITHIN).then_some(look)
    }
}

impl BoardView {
    /// Screen frame `id`'s page answered its blueprint `json`: what changed since its last one
    /// plays over the frame, as its writer's.
    pub(in crate::board) fn screen_blueprint(
        &mut self,
        id: NodeId,
        json: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(blueprint) = Blueprint::parse(json) else {
            return;
        };
        let now = cx.background_executor().now();
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return;
        };
        let (doc, page) = (ds.doc.clone(), ds.page);
        let Some(name) = doc.node(id).and_then(screens::screen_of).map(|s| s.screen) else {
            return;
        };
        let Some(look) = self.agents.screen_writer(&name, now).cloned() else {
            return;
        };
        let Some(b) = doc.world_bounds(id) else {
            return;
        };
        if doc.page_of(id) != Some(page) {
            self.agents.blueprints.insert(id, blueprint);
            return;
        }
        let old = self.agents.blueprints.get(&id).cloned();
        let changed = blueprint::changed(old.as_ref(), &blueprint);
        let tasks = blueprint::tasks(&changed, &blueprint, id, [b.x0, b.y0, b.x1, b.y1]);
        let shown = self.shown && crate::agents::shown(cx);
        let reduce = crate::agents::reduce_motion(cx) || !shown;
        let start = (self.agents.agents.iter())
            .find(|a| a.look.id == look.id)
            .and_then(|a| a.at.map(|s| s.point))
            .unwrap_or((b.x1, b.y0));
        self.agents
            .replays
            .play_screen(&look, page, &doc, tasks, start, now, reduce);
        self.agents.blueprints.insert(id, blueprint);
        self.after_agents_changed(cx);
        cx.notify();
    }
}
