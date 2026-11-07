//! Covers never outlive their build (B17): a newer write of a screen takes out what an older one
//! still covers, and a cover lasts [`COVER_LIMIT`] after its op at most.

use std::time::Instant;

use figcraft_doc::NodeId;

use super::{Replays, Timed};

/// The longest a screen element's cover can last after its write was played, whatever happens to
/// its lane: past it the live page shows (B17).
pub(in crate::board) const COVER_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether task `t`, a screen element's cover, has outlived [`COVER_LIMIT`] at `now`.
pub(in crate::board) fn cover_expired(t: &Timed, now: Instant) -> bool {
    t.task.cover && now.saturating_duration_since(t.made) >= COVER_LIMIT
}

impl Replays {
    /// Takes out every cover over the screen frames `frames`, in every lane.
    pub fn drop_covers(&mut self, frames: &[NodeId]) {
        for lane in &mut self.lanes {
            lane.tasks
                .retain(|t| !(t.task.cover && frames.contains(&t.task.node)));
        }
        self.lanes.retain(|l| !l.tasks.is_empty());
    }
}
