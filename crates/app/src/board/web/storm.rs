//! The reload-storm guard: a route frame whose page refreshes itself (a reload, or a change of its
//! DOM) more than [`LIMIT`] times within [`WINDOW`] stops being refreshed for [`PAUSE`], keeping its
//! last picture and saying it is paused, then takes the refresh it missed and goes on. A dev server
//! thrashing through hot updates, or a page that reloads itself in a loop, costs a settle and a
//! snapshot at most a few times a second.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The most refreshes a frame takes within [`WINDOW`]: one more pauses it.
pub(super) const LIMIT: usize = 4;
/// The window refreshes are counted over.
pub(super) const WINDOW: Duration = Duration::from_secs(1);
/// How long a paused frame stays paused.
pub(super) const PAUSE: Duration = Duration::from_secs(8);

/// A frame's recent refreshes, and the pause they led to.
#[derive(Debug, Default)]
pub(in crate::board) struct Storm {
    hits: VecDeque<Instant>,
    paused_until: Option<Instant>,
}

impl Storm {
    /// A refresh of the frame's page at `now`: whether it goes ahead. While the frame is paused
    /// none does; the one past [`LIMIT`] in [`WINDOW`] pauses it for [`PAUSE`].
    pub(super) fn allow(&mut self, now: Instant) -> bool {
        if self.paused(now) {
            return false;
        }
        while self
            .hits
            .front()
            .is_some_and(|at| now.duration_since(*at) >= WINDOW)
        {
            self.hits.pop_front();
        }
        self.hits.push_back(now);
        if self.hits.len() > LIMIT {
            self.hits.clear();
            self.paused_until = Some(now + PAUSE);
            return false;
        }
        true
    }

    /// Whether the frame is paused at `now`.
    pub(in crate::board) fn paused(&self, now: Instant) -> bool {
        self.paused_until.is_some_and(|until| now < until)
    }

    /// How long the pause has left at `now`, while there is one.
    pub(super) fn left(&self, now: Instant) -> Option<Duration> {
        self.paused_until
            .filter(|until| now < *until)
            .map(|until| until - now)
    }

    /// Ends a pause that is over at `now`: whether one just ended, so the refresh it held back
    /// can come.
    pub(super) fn resume(&mut self, now: Instant) -> bool {
        match self.paused_until {
            Some(until) if now >= until => {
                self.paused_until = None;
                true
            }
            _ => false,
        }
    }

    /// Lets go of the pause, as a reload asked for by hand does.
    pub(super) fn clear(&mut self) {
        self.hits.clear();
        self.paused_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn a_few_refreshes_a_second_go_ahead() {
        let mut storm = Storm::default();
        let start = Instant::now();
        // Four in a second, then a fifth once the first has left the window.
        for n in 0..4 {
            assert!(storm.allow(start + MS(n * 200)), "refresh {n}");
        }
        assert!(storm.allow(start + MS(1000)));
        assert!(!storm.paused(start + MS(1000)));
        // Steady refreshes, two a second, never pause.
        for n in 0..40 {
            assert!(storm.allow(start + MS(2000 + n * 500)), "steady {n}");
        }
    }

    #[test]
    fn a_storm_pauses_then_resumes_on_its_own() {
        let mut storm = Storm::default();
        let start = Instant::now();
        for n in 0..4 {
            assert!(storm.allow(start + MS(n * 50)));
        }
        // The fifth within a second pauses the frame.
        let fifth = start + MS(200);
        assert!(!storm.allow(fifth));
        assert!(storm.paused(fifth));
        assert_eq!(storm.left(fifth), Some(PAUSE));
        // Refreshes during the pause are held back, and don't lengthen it.
        assert!(!storm.allow(fifth + MS(3000)));
        assert!(!storm.resume(fifth + MS(3000)));
        assert_eq!(storm.left(fifth + MS(3000)), Some(PAUSE - MS(3000)));
        // Once it is over the frame resumes, once, and refreshes go ahead again.
        let after = fifth + PAUSE;
        assert!(!storm.paused(after));
        assert!(storm.resume(after));
        assert!(!storm.resume(after));
        assert!(storm.allow(after));
    }

    #[test]
    fn a_reload_by_hand_ends_the_pause() {
        let mut storm = Storm::default();
        let start = Instant::now();
        for n in 0..5 {
            storm.allow(start + MS(n));
        }
        assert!(storm.paused(start + MS(10)));
        storm.clear();
        assert!(!storm.paused(start + MS(10)));
        assert!(storm.allow(start + MS(10)));
    }
}
