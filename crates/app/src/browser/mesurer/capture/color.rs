//! mesurer's `EyeDropper`: `color.sample` opens macOS's colour sampler and replies
//! `{sRGBHex}`, or `null` when the user cancels it (the world's AbortError, which mesurer takes
//! quietly); `color.cancel` (its signal aborted) answers the sample waiting with `null`. One
//! sampler at a time: a second `color.sample` joins the one open, and the first is answered
//! `null`. macOS has no way to close its sampler from here: it stays until the user picks or
//! presses Esc, and what it picks then goes to the sample waiting, if any.
//!
//! The sampler reads the screen, so Sawit's overlay is hidden while it is open: the picker's
//! hover box would tint the very element under the loupe. It is shown again in the browser's turn
//! among screenshots, never in the middle of an agent's shot that has it hidden.

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::NOT_OPEN;
use sawit_core::browser::mesurer::files::srgb_hex;
use serde_json::{Value, json};

use super::super::super::BrowserView;
use super::super::super::engine::WorldReply;
use super::super::super::shots::{overlay_op, within};

/// How long showing the overlay again may wait for the page.
const SHOW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(super) fn sample(view: &mut BrowserView, reply: WorldReply, cx: &mut Context<BrowserView>) {
    let Some(engine) = view.engine.clone() else {
        reply.err(NOT_OPEN);
        return;
    };
    let state = &mut view.mesurer.capture;
    if let Some(earlier) = state.sampling.replace(reply) {
        earlier.ok_value(&Value::Null);
    }
    if state.sampler_open {
        return;
    }
    state.sampler_open = true;
    drop(view.page_op(overlay_op(true)));
    let (tx, rx) = oneshot::channel();
    engine.sample_color(move |picked| {
        let _ = tx.send(picked);
    });
    cx.spawn(async move |this, cx| {
        let picked = rx.await.ok().flatten();
        let turn = this.update(cx, |this, _| {
            let state = &mut this.mesurer.capture;
            state.sampler_open = false;
            if let Some(reply) = state.sampling.take() {
                reply.ok_value(&match picked {
                    Some([red, green, blue]) => json!({ "sRGBHex": srgb_hex(red, green, blue) }),
                    None => Value::Null,
                });
            }
            this.shots.enter()
        });
        let Ok(turn) = turn else { return };
        if let Some(turn) = turn {
            let _ = turn.await;
        }
        if let Ok(shown) = this.update(cx, |this, _| this.page_op(overlay_op(false))) {
            let _ = within(shown, cx.background_executor().timer(SHOW_TIMEOUT)).await;
        }
        let _ = this.update(cx, |this, _| this.shots.leave());
    })
    .detach();
}

pub(super) fn cancel(view: &mut BrowserView, reply: WorldReply) {
    let state = &mut view.mesurer.capture;
    if let Some(waiting) = state.sampling.take() {
        waiting.ok_value(&Value::Null);
    }
    reply.ok_value(&json!({ "open": state.sampler_open }));
}
