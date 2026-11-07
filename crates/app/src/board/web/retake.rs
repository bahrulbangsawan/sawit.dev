//! A half-drawn snapshot taken again a moment later (B17), and Present's sharper pictures of the
//! screens it shows (B15).

use std::sync::Arc;
use std::time::Duration;

use figcraft_doc::NodeId;
use gpui_kit::*;

use super::{BoardView, Phase};

impl BoardView {
    /// Frame `id`'s snapshot was half-drawn: the frame keeps the picture it had, its view stays
    /// shown off the canvas, and, `after` from now, its page settles again where it is and is snapshotted, unless it
    /// has loaded or gone live meanwhile.
    pub(super) fn web_retake(&mut self, id: NodeId, after: Duration, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        frame.phase = Phase::Idle;
        frame.retake_due = true;
        // Its view stays shown off the canvas meanwhile: a hidden view's timers are throttled
        // and its animations paused, so a page that reveals itself on timers would never finish.
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

    /// Present shows screen frames `ids` (B15): their pictures are taken again at `scale` (or
    /// let be at it already), and later ones are too while it shows; `None` ends that.
    pub(in crate::board) fn web_present(
        &mut self,
        ids: &[NodeId],
        scale: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        web.presenting = scale;
        let Some(scale) = scale else {
            return;
        };
        let mut snap = Vec::new();
        for id in ids {
            let Some(frame) = web.frames.get_mut(id) else {
                continue;
            };
            let sharp = frame
                .texture
                .as_ref()
                .is_some_and(|t| t.scale >= scale * 0.9);
            match (&frame.engine, frame.phase) {
                _ if sharp => {}
                (Some(_), Phase::Idle) => snap.push(*id),
                (None, Phase::Cold) => frame.refresh = true,
                _ => {}
            }
        }
        for id in snap {
            self.web_snapshot(id, "present", cx);
        }
        cx.notify();
    }

    /// Web frame `id`'s picture and the CSS size of the page it pictures.
    pub(in crate::board) fn web_texture_of(
        &self,
        id: NodeId,
    ) -> Option<(Arc<RenderImage>, super::Viewport)> {
        let texture = self.web.as_ref()?.frames.get(&id)?.texture.as_ref()?;
        Some((texture.image.clone(), texture.css))
    }
}
