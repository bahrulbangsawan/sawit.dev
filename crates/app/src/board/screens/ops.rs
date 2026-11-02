//! What agents ask of screens while the app runs, through the canvas's agent ops
//! ([`BoardView::agent_op`]): `begin` with `kind: "screen"` makes the frame and the folder at once;
//! `screen` finds a screen; `screen_write` writes its files and reloads it, so the user watches it
//! build; `screen_get` reads it, with a PNG of its latest picture made off the UI thread; and
//! `export_svg` and `export_png` paint the screens' pictures in their frames ([`super::pictures`]).

use std::path::PathBuf;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_board::screens::{self, files};
use serde_json::{Value, json};

use super::pictures::png_of;
use crate::agents::Look;
use crate::board::BoardView;
use crate::workspace::Answer;

/// An op's failure, `<name>: <error>`.
fn failed(name: &str, error: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": format!("{name}: {error}")})
}

impl BoardView {
    /// `look`'s agent's op `op`, when it is a screen's: its answer. Any other op comes back to
    /// run as every op does.
    pub(in crate::board) fn agent_screen_op(
        &mut self,
        op: Value,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> Result<Answer, Value> {
        let name = op.get("op").and_then(Value::as_str).unwrap_or_default();
        let kind = op.get("kind").and_then(Value::as_str).map(str::trim);
        match name {
            "begin" if kind.is_some_and(|k| k.eq_ignore_ascii_case("screen")) => {
                Ok(Answer::Now(self.begin_screen(op, look, cx)))
            }
            "screen" => Ok(Answer::Now(self.find_screen(&op, cx))),
            "routes" => Ok(self.agent_routes_op(op, look, cx)),
            "screen_write" => Ok(Answer::Now(self.write_screen(&op, look, cx))),
            "screen_get" => Ok(self.get_screen(&op, look, cx)),
            "export_svg" | "export_png" => {
                let pictures = self.screen_pictures();
                if pictures.is_empty() {
                    return Err(op);
                }
                Ok(self.export_with_screens(op, look, pictures, cx))
            }
            _ => Err(op),
        }
    }

    /// `begin {kind: "screen"}`: the frame, as any work's, with a folder name no screen folder
    /// has, then the folder.
    fn begin_screen(&mut self, mut op: Value, look: &Look, cx: &mut Context<Self>) -> Value {
        let dir = self.screens_dir();
        op["taken"] = json!(screens::folders(&dir));
        let mut reply = self.run_agent_op(&op, look, None, cx);
        let Some(name) = reply
            .get("screen")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return reply;
        };
        match files::make_folder(&dir, &name) {
            Ok(folder) => reply["folder"] = json!(folder),
            Err(error) => reply["note"] = json!(format!("its folder wasn't made: {error}")),
        }
        reply
    }

    /// The screen `op`'s `work` or `screen` names, with its folder: the `screen` op's answer.
    fn find_screen(&mut self, op: &Value, cx: &mut Context<Self>) -> Value {
        let find = json!({"op": "screen", "work": op.get("work"), "screen": op.get("screen")});
        let mut found = self.run_op(&find, cx);
        if let Some(name) = found.get("screen").and_then(Value::as_str) {
            found["folder"] = json!(screens::folder(&self.screens_dir(), name));
        }
        found
    }

    /// `screen_write`: the screen's files are written, and the frames that show it reload now;
    /// the agent shows at the screen, at work on it.
    fn write_screen(&mut self, op: &Value, look: &Look, cx: &mut Context<Self>) -> Value {
        let write = match files::Write::from_op(op) {
            Ok(write) => write,
            Err(error) => return failed("screen_write", error),
        };
        let mut found = self.find_screen(op, cx);
        if found.get("ok") != Some(&Value::Bool(true)) {
            return found;
        }
        let name = found["screen"].as_str().unwrap_or_default().to_string();
        let folder = PathBuf::from(found["folder"].as_str().unwrap_or_default());
        let written = match files::apply(&folder, &write) {
            Ok(written) => written,
            Err(error) => return failed("screen_write", error),
        };
        if let Some(web) = self.web.as_mut() {
            web.wrote(&name);
        }
        self.web_reload_screen(&name, cx);
        self.agent_at_screen(op, &found, look, cx);
        found["written"] = json!(written);
        found["rendered"] = json!(true);
        found
    }

    /// Shows `look`'s agent at work on the screen `found`: at its work's corner when `op` names
    /// one, else at the frame's.
    fn agent_at_screen(&mut self, op: &Value, found: &Value, look: &Look, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let title = found["name"].as_str().unwrap_or("a screen");
        let doing = format!("writing {title}");
        match op.get("work").and_then(Value::as_str) {
            Some(work) => {
                self.agents.saw(look, &doing, now);
                let undo = self.undo_state();
                self.work_op_ran(work, look, undo, now);
            }
            None => {
                let frame = found["frame"].as_str().and_then(|f| f.parse().ok());
                if let Some(frame) = frame {
                    self.point_at_frame(frame, look, &doing, now);
                }
            }
        }
        self.after_agents_changed(cx);
    }

    /// `screen_get`: the screen's HTML, its files and where it is, and a PNG of its latest
    /// picture, encoded off the UI thread, when it has one.
    fn get_screen(&mut self, op: &Value, look: &Look, cx: &mut Context<Self>) -> Answer {
        let mut found = self.find_screen(op, cx);
        if found.get("ok") != Some(&Value::Bool(true)) {
            return Answer::Now(found);
        }
        let folder = PathBuf::from(found["folder"].as_str().unwrap_or_default());
        found["files"] = json!(files::list(&folder));
        found["html"] = json!(files::page(&folder));
        let frame = found["frame"].as_str().and_then(|f| f.parse().ok());
        if let Some(frame) = frame {
            let now = cx.background_executor().now();
            self.point_at_frame(frame, look, "reading a screen", now);
            self.after_agents_changed(cx);
        }
        let picture = frame.and_then(|frame| self.screen_picture(frame));
        let Some(picture) = picture else {
            found["rendered"] = json!(false);
            found["note"] = json!(
                "The screen has no picture yet: it renders once its index.html is written and \
                 its page has loaded, while its page of the canvas is on show"
            );
            return Answer::Now(found);
        };
        let size = picture.size(0);
        found["picture"] = json!({"width": size.width.0, "height": size.height.0});
        let encoded = cx.background_spawn(async move { png_of(&picture) });
        let (reply, answer) = oneshot::channel();
        cx.spawn(async move |_, _| {
            let png = encoded.await;
            found["rendered"] = json!(png.is_some());
            if let Some(png) = png {
                found["png_base64"] = json!(sawit_core::browser::base64_encode(&png));
            }
            let _ = reply.send(Ok(found));
        })
        .detach();
        Answer::Later(answer)
    }
}
