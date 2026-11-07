//! A half-drawn snapshot taken again a moment later (B17).

use std::time::Duration;

use figcraft_doc::NodeId;
use gpui_kit::*;

use super::{BoardView, Phase, Settle};

impl BoardView {
    /// Frame `id`'s snapshot was half-drawn: its view hides (the frame keeps the picture it had)
    /// and, `after` from now, its page settles again where it is and is snapshotted, unless it
    /// has loaded or gone live meanwhile.
    pub(super) fn web_retake(&mut self, id: NodeId, after: Duration, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let (settle, live) = (web.config.settle, web.live.as_ref().map(|l| l.id));
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        frame.phase = Phase::Idle;
        frame.retake_due = true;
        if let Some(engine) = frame.engine.as_ref().filter(|_| settle != Settle::Hidden)
            && live != Some(id)
        {
            engine.set_visible(false);
        }
        let token = frame.settle;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(frame) = this.web.as_mut().and_then(|w| w.frames.get_mut(&id)) {
                    frame.retake_due = false;
                }
                let ready = (this.web.as_ref()).is_some_and(|web| {
                    web.live.as_ref().is_none_or(|live| live.id != id)
                        && web.frames.get(&id).is_some_and(|frame| {
                            frame.engine.is_some()
                                && frame.phase == Phase::Idle
                                && frame.settle == token
                        })
                });
                if ready {
                    this.web_resettle(id, "half_drawn", cx);
                }
            });
        })
        .detach();
    }
}
