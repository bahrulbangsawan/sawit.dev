//! The page as WebKit draws it, for mesurer: `capture.visible` (its region screenshots crop it)
//! and `capture.frame` (its recordings' frames). Each waits for its turn among the browser's
//! screenshots ([`super::super::super::shots::ShotGate`]), so no agent's shot has the overlay or
//! mesurer hidden meanwhile: what mesurer captures is what the user sees. Encoded off the main
//! thread.

use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::{NOT_OPEN, base64_encode};
use serde_json::{Value, json};

use super::super::super::BrowserView;
use super::super::super::engine::{Engine, WorldReply};
use super::super::super::shots::within;

/// How long a capture waits for WebKit.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);
/// A frame's JPEG quality when the world gives none.
const FRAME_QUALITY: f64 = 0.8;

/// The view as it shows, at the display's scale: replies `{png_base64, width, height}`.
pub(super) fn visible(view: &mut BrowserView, reply: WorldReply, cx: &mut Context<BrowserView>) {
    capture(
        view,
        reply,
        cx,
        |engine, tx| engine.capture_visible(tx),
        |pixels| {
            let (width, height) = pixels.size();
            let png = pixels.png()?;
            Ok(json!({ "png_base64": base64_encode(&png), "width": width, "height": height }))
        },
    );
}

/// A recording's frame, at most `max_width` pixels wide: replies `{jpeg_base64, width, height,
/// snap_ms, encode_ms}`.
pub(super) fn frame(
    view: &mut BrowserView,
    max_width: Option<u32>,
    quality: Option<f64>,
    reply: WorldReply,
    cx: &mut Context<BrowserView>,
) {
    let quality = quality.filter(|q| q.is_finite()).unwrap_or(FRAME_QUALITY);
    capture(
        view,
        reply,
        cx,
        move |engine, tx| engine.capture_frame(max_width, tx),
        move |pixels| {
            let (width, height) = pixels.size();
            let jpeg = pixels.jpeg(quality)?;
            Ok(json!({ "jpeg_base64": base64_encode(&jpeg), "width": width, "height": height }))
        },
    );
}

/// Takes the capture `start` asks WebKit for in the browser's turn among screenshots, encodes
/// it off the main thread with `encode`, and replies with that and how long each took.
fn capture<P: Send + 'static>(
    view: &mut BrowserView,
    reply: WorldReply,
    cx: &mut Context<BrowserView>,
    start: impl FnOnce(&Engine, oneshot::Sender<Result<P, String>>) + 'static,
    encode: impl FnOnce(P) -> Result<Value, String> + Send + 'static,
) {
    if view.engine.is_none() {
        reply.err(NOT_OPEN);
        return;
    }
    let turn = view.shots.enter();
    cx.spawn(async move |this, cx| {
        if let Some(turn) = turn {
            let _ = turn.await;
        }
        let began = Instant::now();
        let (tx, rx) = oneshot::channel();
        let started = this.update(cx, |this, _| match &this.engine {
            Some(engine) => {
                start(engine, tx);
                true
            }
            None => false,
        });
        let pixels = if matches!(started, Ok(true)) {
            let timer = cx.background_executor().timer(CAPTURE_TIMEOUT);
            match within(rx, timer).await {
                Some(Ok(pixels)) => pixels,
                Some(Err(_)) => Err("The capture was cancelled.".into()),
                None => Err("WebKit didn't capture the page in time.".into()),
            }
        } else {
            Err(NOT_OPEN.into())
        };
        // The next shot may go while this one is encoded.
        let _ = this.update(cx, |this, _| this.shots.leave());
        let snapped = began.elapsed();
        let encoded = match pixels {
            Ok(pixels) => {
                let encoding = cx.background_executor().spawn(async move {
                    let at = Instant::now();
                    let mut value = encode(pixels)?;
                    value["encode_ms"] = json!(at.elapsed().as_secs_f64() * 1000.);
                    Ok::<_, String>(value)
                });
                encoding.await
            }
            Err(e) => Err(e),
        };
        match encoded {
            Ok(mut value) => {
                value["snap_ms"] = json!(snapped.as_secs_f64() * 1000.);
                reply.ok_value(&value);
            }
            Err(e) => reply.err(e),
        }
    })
    .detach();
}
