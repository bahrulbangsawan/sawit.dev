//! Watching an agent draw (B4.1): when an agent session starts working on the canvas, the view
//! glides once to fit the page's content; and while **Follow agent** is on (the toolbar's toggle,
//! shown while an agent works), the view glides to each shape an agent adds or changes, as it
//! happens. Changes within [`GROUP`] go in one move, the view only pans while the shape fits and
//! zooms out only when it would be off screen, and a pan or zoom by hand turns follow off. Under
//! Reduce Motion the view jumps instead of gliding. Agents can read the state (`app_state`), but
//! only the user turns it on or off.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use figcraft_engine::ViewState;
use gpui_kit::*;
use serde_json::{Value, json};

use super::BoardView;

/// Changes this close together go in one move, so the view doesn't shake.
pub(super) const GROUP: Duration = Duration::from_millis(150);
/// How long a glide takes.
const GLIDE: Duration = Duration::from_millis(420);
/// The room kept around a followed shape, in screen px.
const MARGIN: f64 = 64.0;
/// How long after an agent's last change the toolbar still shows the toggle.
pub(super) const ACTIVE_FOR: Duration = Duration::from_secs(20);

/// Where the view is: its centre on the page and its zoom.
pub(super) type Place = [f64; 3];

/// A glide of the view from one place to another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Glide {
    pub from: Place,
    pub to: Place,
    pub start: Instant,
}

impl Glide {
    /// Where the glide is at `now`, and whether it is over.
    pub fn at(&self, now: Instant) -> (Place, bool) {
        let t = now.saturating_duration_since(self.start).as_secs_f64() / GLIDE.as_secs_f64();
        if t >= 1.0 {
            return (self.to, true);
        }
        // Ease out (cubic): fast start, gentle landing.
        let e = 1.0 - (1.0 - t).powi(3);
        // The zoom moves in log space, so zooming out and in feel alike.
        let zoom = (self.from[2].ln() + (self.to[2].ln() - self.from[2].ln()) * e).exp();
        let lerp = |a: f64, b: f64| a + (b - a) * e;
        (
            [
                lerp(self.from[0], self.to[0]),
                lerp(self.from[1], self.to[1]),
                zoom,
            ],
            false,
        )
    }
}

/// The follow state of a canvas.
#[derive(Debug)]
pub(super) struct Follow {
    /// The toolbar's toggle: on at first and with each new agent session, off after a pan or
    /// zoom by hand or a click on it.
    pub on: bool,
    /// The agent sessions whose start the view was fitted for.
    pub fitted: HashSet<SharedString>,
    /// The union of the changes waiting to be followed, and when the first of them came.
    pub pending: Option<([f64; 4], Instant)>,
    pub glide: Option<Glide>,
    /// Where follow last put the view: a view anywhere else was moved by hand.
    pub ours: Option<Place>,
    /// When an agent last changed the canvas: the toggle shows for [`ACTIVE_FOR`] after.
    pub active: Option<Instant>,
    /// A wake for the pending move is on its way.
    pub waking: bool,
}

impl Default for Follow {
    fn default() -> Self {
        Follow {
            on: true,
            fitted: HashSet::new(),
            pending: None,
            glide: None,
            ours: None,
            active: None,
            waking: false,
        }
    }
}

impl Follow {
    /// Whether an agent is at work, so that the toggle shows.
    pub fn agent_active(&self, now: Instant) -> bool {
        self.active
            .is_some_and(|at| now.saturating_duration_since(at) < ACTIVE_FOR)
    }

    /// The state agents read (`app_state`).
    pub fn state(&self, now: Instant) -> Value {
        json!({"on": self.on, "agent_active": self.agent_active(now)})
    }

    /// The user moved the view: follow stops, and nothing it had planned happens.
    pub fn moved_by_hand(&mut self) {
        self.on = false;
        self.glide = None;
        self.pending = None;
        self.ours = None;
    }
}

/// The place of `view`.
pub(super) fn place_of(view: &ViewState) -> Place {
    [view.cx, view.cy, view.zoom]
}

/// Whether two places differ by more than rounding.
pub(super) fn differs(a: Place, b: Place) -> bool {
    (a[0] - b[0]).abs() > 0.01 || (a[1] - b[1]).abs() > 0.01 || (a[2] / b[2] - 1.0).abs() > 1e-4
}

/// Where the view goes to show page rect `rect` (x0, y0, x1, y1), from `view`: where it is when
/// the rect is on screen with [`MARGIN`] around it; else centred on it at the same zoom when it
/// fits; else zoomed out until it fits (never zoomed in). `None` when it stays.
pub(super) fn to_show(view: &ViewState, rect: [f64; 4]) -> Option<Place> {
    let (vw, vh) = (view.vw.max(1.0), view.vh.max(1.0));
    let (w, h) = ((rect[2] - rect[0]).abs(), (rect[3] - rect[1]).abs());
    let room = |zoom: f64| {
        (
            (vw - 2.0 * MARGIN).max(1.0) / zoom,
            (vh - 2.0 * MARGIN).max(1.0) / zoom,
        )
    };
    let (rw, rh) = room(view.zoom);
    let (x0, y0) = view.screen_to_page(MARGIN, MARGIN);
    let shown = rect[0] >= x0 && rect[1] >= y0 && rect[0] + w <= x0 + rw && rect[1] + h <= y0 + rh;
    if shown {
        return None;
    }
    let centre = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
    let zoom = if w <= rw && h <= rh {
        view.zoom
    } else {
        let fit = ((vw - 2.0 * MARGIN).max(1.0) / w.max(1.0))
            .min((vh - 2.0 * MARGIN).max(1.0) / h.max(1.0));
        fit.clamp(ViewState::MIN_ZOOM, view.zoom)
    };
    Some([centre.0, centre.1, zoom])
}

impl BoardView {
    /// An agent (`session`) changed the canvas at page rect `rect` (`None` when it changed nothing
    /// that shows): the view fits the page for a new session, and follows the change while follow
    /// is on.
    pub(in crate::board) fn follow_saw(
        &mut self,
        session: &SharedString,
        rect: Option<[f64; 4]>,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        self.follow.active = Some(now);
        if !self.follow.fitted.contains(session) {
            self.follow.fitted.insert(session.clone());
            self.follow.on = true;
            self.follow.pending = None;
            if let Some(to) = self.fit_place() {
                self.follow_to(to, cx);
            }
            cx.notify();
            return;
        }
        let Some(rect) = rect.filter(|_| self.follow.on) else {
            cx.notify();
            return;
        };
        self.follow.pending = Some(match self.follow.pending {
            Some((r, since)) => (
                [
                    r[0].min(rect[0]),
                    r[1].min(rect[1]),
                    r[2].max(rect[2]),
                    r[3].max(rect[3]),
                ],
                since,
            ),
            None => (rect, now),
        });
        if !std::mem::replace(&mut self.follow.waking, true) {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(GROUP).await;
                let _ = this.update(cx, |this, cx| {
                    this.follow.waking = false;
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Where the view would be with the page's content fitted.
    fn fit_place(&self) -> Option<Place> {
        let ds = self.board()?.session.ds().ok()?;
        let mut view = ds.view.clone();
        sawit_board::view::zoom_to_fit(&mut view, &ds.doc, ds.page);
        Some(place_of(&view))
    }

    /// Glides the view to `to`, or jumps there under Reduce Motion.
    fn follow_to(&mut self, to: Place, cx: &mut Context<Self>) {
        let Some(from) = self.view_mut().map(|v| place_of(v)) else {
            return;
        };
        if !differs(from, to) {
            return;
        }
        if crate::agents::reduce_motion(cx) {
            self.set_place(to);
            self.follow.glide = None;
        } else {
            self.follow.glide = Some(Glide {
                from,
                to,
                start: Instant::now(),
            });
        }
        cx.notify();
    }

    fn set_place(&mut self, place: Place) {
        if let Some(view) = self.view_mut() {
            (view.cx, view.cy, view.zoom) = (place[0], place[1], place[2]);
        }
        self.follow.ours = Some(place);
    }

    /// Each frame, before the canvas renders: a view moved by hand stops follow; a glide moves
    /// the view on; changes grouped long enough are followed.
    pub(in crate::board) fn follow_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(now_place) = self.view_mut().map(|v| place_of(v)) else {
            return;
        };
        if let Some(ours) = self.follow.ours
            && differs(ours, now_place)
        {
            self.follow.moved_by_hand();
            cx.notify();
            return;
        }
        let now = Instant::now();
        if let Some(glide) = self.follow.glide {
            let (place, done) = glide.at(now);
            self.set_place(place);
            if done {
                self.follow.glide = None;
            } else {
                window.request_animation_frame();
            }
            return;
        }
        let due = (self.follow.pending)
            .filter(|(_, since)| now.saturating_duration_since(*since) >= GROUP);
        if let Some((rect, _)) = due {
            self.follow.pending = None;
            let to = self.view_mut().and_then(|view| to_show(view, rect));
            if let Some(to) = to {
                self.follow_to(to, cx);
                window.request_animation_frame();
            }
        }
    }

    /// What `app_state` says of this canvas's view: Follow agent (which agents read, never set),
    /// and whether Present or Preview shows.
    pub fn describe_view(&self) -> Value {
        json!({
            "follow": self.follow.state(Instant::now()),
            "presenting": self.player.is_some(),
            "preview": self.preview_open(),
        })
    }

    /// The toolbar's Follow agent toggle was clicked.
    pub(in crate::board) fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        self.follow.on = !self.follow.on;
        if !self.follow.on {
            self.follow.glide = None;
            self.follow.pending = None;
        }
        self.follow.ours = None;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use figcraft_engine::ViewState;
    use serde_json::json;

    use super::{Follow, GLIDE, Glide, to_show};

    fn view(cx: f64, cy: f64, zoom: f64) -> ViewState {
        let mut v = ViewState::default();
        (v.cx, v.cy, v.zoom, v.vw, v.vh) = (cx, cy, zoom, 1000.0, 800.0);
        v
    }

    #[test]
    fn a_shape_on_screen_leaves_the_view_alone() {
        let v = view(0.0, 0.0, 1.0);
        assert_eq!(to_show(&v, [-100.0, -100.0, 100.0, 100.0]), None);
    }

    #[test]
    fn a_shape_off_screen_is_panned_to_at_the_same_zoom() {
        let v = view(0.0, 0.0, 1.0);
        let to = to_show(&v, [2000.0, 0.0, 2100.0, 100.0]).unwrap();
        assert_eq!(to, [2050.0, 50.0, 1.0]);
    }

    #[test]
    fn a_shape_too_big_zooms_out_never_in() {
        let v = view(0.0, 0.0, 1.0);
        let to = to_show(&v, [0.0, 0.0, 4000.0, 300.0]).unwrap();
        assert!(to[2] < 1.0 && to[2] > 0.2, "{to:?}");
        // A small shape far away at a low zoom keeps the zoom.
        let far = view(0.0, 0.0, 0.25);
        assert_eq!(to_show(&far, [9000.0, 0.0, 9010.0, 10.0]).unwrap()[2], 0.25);
    }

    #[test]
    fn a_glide_lands_where_it_was_going() {
        let start = Instant::now();
        let glide = Glide {
            from: [0.0, 0.0, 1.0],
            to: [100.0, 50.0, 0.5],
            start,
        };
        let (mid, done) = glide.at(start + GLIDE / 2);
        assert!(
            !done && mid[0] > 50.0 && mid[2] < 1.0 && mid[2] > 0.5,
            "{mid:?}"
        );
        assert_eq!(glide.at(start + GLIDE), ([100.0, 50.0, 0.5], true));
    }

    #[test]
    fn a_move_by_hand_stops_following() {
        let mut follow = Follow {
            pending: Some(([0.0; 4], Instant::now())),
            ..Follow::default()
        };
        follow.moved_by_hand();
        assert!(!follow.on && follow.pending.is_none());
        assert_eq!(follow.state(Instant::now())["on"], json!(false));
    }
}
