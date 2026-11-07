//! Convert to Layers (B15) in the app: the screen's page is read where it renders, in its own web
//! view once that is at rest ([`sawit_board::screens::layers::CAPTURE`]), and the `screen_layers`
//! op builds the editable frame beside it in one undo step. The user starts it from the screen's
//! bar, its row in the Layers panel or the Design panel; an agent with `canvas_screen_to_layers`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use figcraft_doc::NodeId;
use futures::channel::oneshot;
use gpui_kit::*;
use sawit_board::screens::layers::CAPTURE;
use serde_json::{Value, json};

use super::ops::REST_WAIT;
use crate::agents::Look;
use crate::board::BoardView;
use crate::workspace::Answer;

/// What Convert to Layers says when the screen's page isn't rendered on the page on show.
const NOT_SHOWN: &str = "the screen renders only while its page of the canvas is on show: show \
     that page, then convert it";

impl BoardView {
    /// The `screen_layers` op an agent sent (`{work? | screen?}`): answered once the page is
    /// read and the layers are made.
    pub(in crate::board) fn agent_screen_layers(
        &mut self,
        op: &Value,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> Answer {
        let find = json!({"op": "screen", "work": op.get("work"), "screen": op.get("screen")});
        let found = self.run_op(&find, cx);
        if found.get("ok") != Some(&Value::Bool(true)) {
            return Answer::Now(found);
        }
        let frame = found["frame"].as_str().and_then(|f| f.parse().ok());
        let Some(frame) = frame.map(NodeId) else {
            return Answer::Now(json!({"ok": false, "error": "screen_layers: no frame"}));
        };
        let now = cx.background_executor().now();
        self.point_at_frame(frame, look, "converting a screen to layers", now);
        self.after_agents_changed(cx);
        Answer::Later(self.convert_screen(frame, Some(look.clone()), cx))
    }

    /// The user's Convert to Layers on screen frame `frame`: the new frame is selected, and a
    /// toast says what was made or why nothing was.
    pub(in crate::board) fn convert_screen_to_layers(
        &mut self,
        frame: NodeId,
        cx: &mut Context<Self>,
    ) {
        self.show_toast("Converting to layers…".to_string(), cx);
        let answer = self.convert_screen(frame, None, cx);
        cx.spawn(async move |this, cx| {
            let reply = answer.await.unwrap_or_else(|_| Err("cancelled".into()));
            let _ = this.update(cx, |view, cx| {
                let text = match &reply {
                    Ok(v) if v["ok"] == json!(true) => {
                        if let Some(id) = v["frame"].as_str().and_then(|f| f.parse().ok())
                            && let Some(board) = view.board_mut()
                        {
                            board.session.set_selection(vec![NodeId(id)]);
                        }
                        converted_text(v)
                    }
                    Ok(v) => format!(
                        "Couldn't convert to layers: {}",
                        v["error"].as_str().unwrap_or("unknown error")
                    ),
                    Err(e) => format!("Couldn't convert to layers: {e}"),
                };
                view.show_toast(text, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads screen frame `frame`'s page once its view is at rest and builds its layers, for an
    /// agent (`look`) or the user.
    fn convert_screen(
        &mut self,
        frame: NodeId,
        look: Option<Look>,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Result<Value, String>> {
        let (reply, answer) = oneshot::channel();
        let name = (self.board())
            .and_then(|b| b.session.ds().ok())
            .and_then(|ds| sawit_board::screens::screen_of(ds.doc.node(frame)?))
            .map(|d| d.screen);
        let Some(name) = name else {
            let _ = reply.send(Ok(failed("it isn't a screen")));
            return answer;
        };
        if !self
            .web
            .as_ref()
            .is_some_and(|w| w.frames.contains_key(&frame))
        {
            let _ = reply.send(Ok(failed(NOT_SHOWN)));
            return answer;
        }
        self.web_want_view(frame, cx);
        cx.spawn(async move |this, cx| {
            let capture = match read_page(&this, frame, cx).await {
                Ok(capture) => capture,
                Err(error) => {
                    let _ = reply.send(Ok(failed(&error)));
                    return;
                }
            };
            let op = json!({"op": "screen_layers", "screen": name, "capture": capture});
            let done = this.update(cx, |view, cx| {
                let reply = match &look {
                    Some(look) => view.run_agent_op(&op, look, None, cx),
                    None => view.run_op(&op, cx),
                };
                cx.notify();
                reply
            });
            let _ = reply.send(done.map_err(|e| e.to_string()));
        })
        .detach();
        answer
    }
}

fn failed(why: &str) -> Value {
    json!({"ok": false, "error": format!("screen_layers: {why}")})
}

/// What the toast says after a conversion.
pub(in crate::board) fn converted_text(v: &Value) -> String {
    let layers = v["layers"].as_u64().unwrap_or(0);
    let skipped = &v["skipped"];
    let left: u64 = ["pseudo", "backgrounds", "images"]
        .iter()
        .filter_map(|k| skipped[k].as_u64())
        .sum();
    let mut text = format!("Converted to {layers} layers. The HTML screen is kept beside it.");
    if left > 0 {
        text.push_str(&format!(
            " {left} decorations couldn't be converted (background images, CSS content, outside pictures)."
        ));
    }
    text
}

/// Waits (at most [`REST_WAIT`]) for frame `frame`'s view to be built and at rest, then reads its
/// page with [`CAPTURE`].
async fn read_page(
    this: &WeakEntity<BoardView>,
    frame: NodeId,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    let started = Instant::now();
    loop {
        let ready = this
            .update(cx, |view, _| view.web_view_ready(frame))
            .map_err(|e| e.to_string())?;
        if ready {
            break;
        }
        if started.elapsed() >= REST_WAIT {
            return Err("its page didn't finish loading; try again in a moment".into());
        }
        cx.background_executor()
            .timer(Duration::from_millis(150))
            .await;
    }
    let (sent, got) = oneshot::channel::<String>();
    let sent = Mutex::new(Some(sent));
    this.update(cx, |view, _| {
        view.web_eval(frame, CAPTURE, move |json| {
            if let Some(sent) = sent.lock().ok().and_then(|mut s| s.take()) {
                let _ = sent.send(json);
            }
        })
    })
    .map_err(|e| e.to_string())??;
    let json = got
        .await
        .map_err(|_| "its page didn't answer".to_string())?;
    if json.is_empty() || json == "null" {
        return Err("its page couldn't be read".into());
    }
    Ok(json)
}
