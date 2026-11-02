//! Screenshots, one at a time: the overlay hidden, WebKit's capture of the view or of an
//! element's box, and the overlay shown again whatever happened.

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::ViewportMode;
use serde_json::{Value, json};

use super::engine::Capture;
use super::{BrowserView, OpResult};

/// How long a shot waits for the overlay to hide, and then for WebKit's capture, so a
/// lost callback can't keep the overlay hidden and the next shots waiting.
const SHOT_STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// `fut`'s output, or `None` when `timer` fires first.
pub(super) async fn within<F: std::future::Future + Unpin>(
    fut: F,
    timer: impl std::future::Future<Output = ()> + Unpin,
) -> Option<F::Output> {
    match futures::future::select(fut, timer).await {
        futures::future::Either::Left((out, _)) => Some(out),
        futures::future::Either::Right(_) => None,
    }
}

impl BrowserView {
    /// Takes a screenshot of the page without the overlay in it: hides the
    /// overlay, captures, and shows the overlay again whatever happened. The
    /// capture is then brought to the page's CSS size off the UI thread: that
    /// takes the CPU a while for a retina capture of a whole view.
    pub(crate) fn shoot(
        &mut self,
        css_rect: Option<(f64, f64, f64, f64)>,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let turn = self.shots.enter();
        cx.spawn(async move |this, cx| {
            // Another shot has the overlay hidden: wait for it to finish.
            if let Some(turn) = turn {
                let _ = turn.await;
            }
            let keep = this.update(cx, |_, cx| crate::agents::in_screenshots(cx));
            let [hide, show] = shoot_ops(keep.unwrap_or(false));
            let hidden = this.update(cx, |this, _| this.page_op(hide));
            // A page without an agent has no overlay to hide; one that doesn't answer
            // is shot as it is.
            if let Ok(hidden) = hidden {
                let timer = cx.background_executor().timer(SHOT_STEP_TIMEOUT);
                let _ = within(hidden, timer).await;
            }
            let (tx, rx) = oneshot::channel();
            let started = this.update(cx, |this, _| this.snap(css_rect, tx));
            let captured = if started.is_ok() {
                let timer = cx.background_executor().timer(SHOT_STEP_TIMEOUT);
                match within(rx, timer).await {
                    Some(shot) => {
                        shot.unwrap_or_else(|_| Err("The screenshot was cancelled.".into()))
                    }
                    None => Err(format!(
                        "WebKit didn't capture the page within {} s.",
                        SHOT_STEP_TIMEOUT.as_secs()
                    )),
                }
            } else {
                Err("The browser isn't open.".into())
            };
            let _ = this.update(cx, |this, _| drop(this.page_op(show)));
            let result = match captured {
                Ok(capture) => {
                    let scaled = cx
                        .background_executor()
                        .spawn(async move { capture.at_1x() });
                    scaled.await
                }
                Err(e) => Err(e),
            };
            let _ = this.update(cx, |this, _| this.shots.leave());
            let _ = reply.send(result);
        })
        .detach();
    }

    /// Captures the view, or `rect` of the page.
    fn snap(
        &self,
        rect: Option<(f64, f64, f64, f64)>,
        reply: oneshot::Sender<Result<Capture, String>>,
    ) {
        match &self.engine {
            Some(engine) => {
                let width = shot_width(rect, self.bounds, &self.viewport);
                engine.snapshot(rect, width, self.scale(), reply)
            }
            None => {
                let _ = reply.send(Err("The browser isn't open.".into()));
            }
        }
    }
}

/// The width a capture is scaled to: the element's, else the view's.
fn shot_width(
    rect: Option<(f64, f64, f64, f64)>,
    bounds: Option<Bounds<Pixels>>,
    mode: &ViewportMode,
) -> f64 {
    match (rect, mode) {
        (Some((_, _, w, _)), _) => w,
        (None, ViewportMode::Fixed { width, .. }) => f64::from(*width),
        (None, ViewportMode::Responsive) => {
            bounds.map_or(1024., |b| f64::from(f32::from(b.size.width)))
        }
    }
}

/// Around every screenshot: hide the overlay, then show it again. Agents' cursors hide with it,
/// unless the user keeps them in agents' screenshots (`keep_agents`).
pub(super) fn shoot_ops(keep_agents: bool) -> [Value; 2] {
    let mut hide = overlay_op(true);
    if keep_agents {
        hide["agents"] = json!(true);
    }
    [hide, overlay_op(false)]
}

pub(super) fn overlay_op(hidden: bool) -> Value {
    json!({ "op": "overlay", "hidden": hidden })
}

/// Lets one screenshot at a time hide the overlay: `enter` returns `None`
/// when it is free, else a receiver that fires on this shot's turn; every
/// shot calls `leave` when done.
#[derive(Default)]
pub(super) struct ShotGate {
    busy: bool,
    waiting: std::collections::VecDeque<oneshot::Sender<()>>,
}

impl ShotGate {
    pub(super) fn enter(&mut self) -> Option<oneshot::Receiver<()>> {
        if !self.busy {
            self.busy = true;
            return None;
        }
        let (tx, rx) = oneshot::channel();
        self.waiting.push_back(tx);
        Some(rx)
    }

    pub(super) fn leave(&mut self) {
        // A waiter that went away is skipped.
        while let Some(next) = self.waiting.pop_front() {
            if next.send(()).is_ok() {
                return;
            }
        }
        self.busy = false;
    }
}
