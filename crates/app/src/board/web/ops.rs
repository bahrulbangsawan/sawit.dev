//! The `web` control op, `{"op": "web", "action": …}`, which drives and measures the spike over
//! the control socket:
//!
//! - `status`: the frames, their views and textures, the live frame, the readings since `since`
//!   (an index into the log) and the last bench's report.
//! - `enter` / `exit`: live mode for `id` (default the first web frame) / out of it.
//! - `snapshot`: snapshots `id` (or every frame with a view, `all`) now, `repeat` times back to
//!   back; `scale` fixes the points per CSS px of this and later snapshots.
//! - `settle`: shows `id`'s view off the canvas (or keeps it hidden, `visible: false`) for `ms`,
//!   then reads its probe and snapshots it, as after a load: for CPU readings.
//! - `probe`, `reload`: reads `id`'s `__probe` (or what the script `js` gives) / reloads its page
//!   (which settles it again).
//! - `view`: puts the view at `zoom`, centred on page point (`x`, `y`).
//! - `config`: sets `lag` (the live view a frame late) and `scale` (viewscale, magnification or
//!   pagezoom) from now on.
//! - `bench`: `kind` pan, zoom or still, for `frames` frames at `speed`; `textures` false paints
//!   the canvas alone; `marker` paints a bar beside the live view.

use figcraft_doc::NodeId;
use gpui_kit::*;
use serde_json::{Value, json};

use super::bench::{Bench, Kind};
use super::{BoardView, LiveScale, Phase, Settle, WebMsg, web_nodes};

impl BoardView {
    /// Runs a `web` op, answered at once; what happens later goes to the log `status` reads.
    pub(in crate::board) fn web_op(&mut self, op: &Value, cx: &mut Context<Self>) -> Value {
        if self.web.is_none() {
            return json!({"ok": false, "error": "web frames are off (SAWIT_WEB_FRAMES=1)"});
        }
        let action = op.get("action").and_then(Value::as_str).unwrap_or("status");
        let id = self.web_target(op);
        let reply = match (action, id) {
            ("status", _) => return self.web_status(op),
            ("exit", _) => {
                self.web_exit(cx);
                Ok(json!("left"))
            }
            ("view", _) => self.web_view(op, cx),
            ("bench", _) => self.web_bench(op, cx),
            ("config", _) => self.web_config(op),
            (_, None) => Err("no web frame on this page".to_string()),
            ("enter", Some(id)) => self.web_enter(id, cx).map(|state| json!(state)),
            ("snapshot", Some(id)) => self.web_snap_op(op, id, cx),
            ("settle", Some(id)) => self.web_settle_op(op, id, cx),
            ("probe", Some(id)) => self.web_probe(id, op),
            ("reload", Some(id)) => self.web_reload(id),
            (other, _) => Err(format!("unknown web action {other}")),
        };
        match reply {
            Ok(value) => json!({"ok": true, "result": value}),
            Err(error) => json!({"ok": false, "error": error}),
        }
    }

    /// The frame an op names by `id` (a node id or an agent's id), else the first on the page.
    fn web_target(&self, op: &Value) -> Option<NodeId> {
        let ds = self.board()?.session.ds().ok()?;
        let urls = self.web.as_ref().is_some_and(|web| web.config.urls);
        let nodes = web_nodes(&ds.doc, ds.page, urls);
        let named = match op.get("id") {
            Some(Value::Number(n)) => n.as_u64().map(NodeId),
            Some(Value::String(s)) => sawit_board::skeleton::resolve(&ds.doc, s),
            _ => return nodes.first().map(|node| node.id),
        };
        named.filter(|id| nodes.iter().any(|node| node.id == *id))
    }

    fn web_status(&self, op: &Value) -> Value {
        let Some(web) = self.web.as_ref() else {
            return json!({"ok": false});
        };
        let since = op.get("since").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut frames: Vec<Value> = (web.frames.iter())
            .map(|(id, frame)| {
                let texture = frame.texture.as_ref().map(|texture| {
                    let size = texture.image.size(0);
                    json!({"px": [size.width.0, size.height.0], "points_per_css": texture.scale})
                });
                json!({"id": id.0, "source": format!("{:?}", frame.spec.source), "phase": format!("{:?}", frame.phase),
                    "view": frame.engine.is_some(), "pid": frame.pid, "texture": texture,
                    "probe": frame.probe})
            })
            .collect();
        frames.sort_by_key(|frame| frame["id"].as_u64());
        let view = self
            .board()
            .and_then(|board| board.session.ds().ok())
            .map(|ds| {
                json!({"zoom": ds.view.zoom, "cx": ds.view.cx, "cy": ds.view.cy,
                   "selection": ds.selection.iter().map(|id| id.0).collect::<Vec<_>>()})
            });
        let canvas = web.canvas;
        json!({"ok": true, "config": format!("{:?}", web.config), "frames": frames,
            "live": web.live.as_ref().map(|live| json!({"id": live.id.0, "placed": live.placed.map(|(f, z)| json!([f.x, f.y, f.width, f.height, z])), "hidden": live.hidden})),
            "canvas": [f32::from(canvas.origin.x), f32::from(canvas.origin.y), f32::from(canvas.size.width), f32::from(canvas.size.height)],
            "view": view, "bench_running": web.bench.is_some(), "last_bench": web.last_bench,
            "log_len": web.log.len(), "log": web.log.iter().skip(since).collect::<Vec<_>>()})
    }

    fn web_view(&mut self, op: &Value, cx: &mut Context<Self>) -> Result<Value, String> {
        let number = |key: &str| op.get(key).and_then(Value::as_f64);
        let view = self.view_mut().ok_or("no view")?;
        if let Some(zoom) = number("zoom") {
            view.zoom = zoom.clamp(0.02, 64.0);
        }
        if let (Some(x), Some(y)) = (number("x"), number("y")) {
            (view.cx, view.cy) = (x, y);
        }
        let reply = json!({"zoom": view.zoom, "cx": view.cx, "cy": view.cy});
        cx.notify();
        Ok(reply)
    }

    fn web_config(&mut self, op: &Value) -> Result<Value, String> {
        let web = self.web.as_mut().ok_or("off")?;
        if let Some(lag) = op.get("lag").and_then(Value::as_bool) {
            web.config.live_lag = lag;
        }
        match op.get("scale").and_then(Value::as_str) {
            Some("viewscale") => web.config.live_scale = LiveScale::ViewScale,
            Some("magnification") => web.config.live_scale = LiveScale::Magnification,
            Some("pagezoom") => web.config.live_scale = LiveScale::PageZoom,
            _ => {}
        }
        if let Some(live) = web.live.as_mut() {
            live.next = None;
        }
        Ok(json!(format!("{:?}", web.config)))
    }

    fn web_bench(&mut self, op: &Value, cx: &mut Context<Self>) -> Result<Value, String> {
        let kind = match op.get("kind").and_then(Value::as_str).unwrap_or("pan") {
            "zoom" => Kind::Zoom,
            "still" => Kind::Still,
            _ => Kind::Pan,
        };
        let frames = op.get("frames").and_then(Value::as_u64).unwrap_or(240) as u32;
        let speed = op.get("speed").and_then(Value::as_f64).unwrap_or(8.0);
        let flag = |key: &str, default| op.get(key).and_then(Value::as_bool).unwrap_or(default);
        let web = self.web.as_mut().ok_or("off")?;
        web.bench = Some(Bench::new(
            kind,
            frames,
            speed,
            flag("textures", true),
            flag("marker", false),
        ));
        cx.notify();
        Ok(json!("started"))
    }

    fn web_snap_op(
        &mut self,
        op: &Value,
        id: NodeId,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let repeat = op.get("repeat").and_then(Value::as_u64).unwrap_or(1).max(1) as u32;
        let web = self.web.as_mut().ok_or("off")?;
        // From now on, snapshots are taken at this many points per CSS px.
        if let Some(scale) = op.get("scale").and_then(Value::as_f64) {
            web.config.scale = Some(scale);
        }
        let all = op.get("all").and_then(Value::as_bool).unwrap_or(false);
        let ids: Vec<NodeId> = (web.frames.iter())
            .filter(|(fid, frame)| {
                (all || **fid == id) && frame.engine.is_some() && frame.phase == Phase::Idle
            })
            .map(|(fid, _)| *fid)
            .collect();
        for fid in &ids {
            if let Some(frame) = web.frames.get_mut(fid) {
                frame.repeat = repeat - 1;
            }
        }
        for fid in &ids {
            self.web_snapshot(*fid, "op", cx);
        }
        Ok(json!({"snapshotting": ids.iter().map(|id| id.0).collect::<Vec<_>>(), "repeat": repeat}))
    }

    fn web_settle_op(
        &mut self,
        op: &Value,
        id: NodeId,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let wait = op.get("ms").and_then(Value::as_u64).unwrap_or(1000);
        let web = self.web.as_mut().ok_or("off")?;
        let visible = op
            .get("visible")
            .and_then(Value::as_bool)
            .unwrap_or(web.config.settle != Settle::Hidden);
        let frame = web.frames.get_mut(&id).ok_or("no frame")?;
        let engine = frame.engine.as_ref().ok_or("no view (pool let go of it)")?;
        if frame.phase != Phase::Idle {
            return Err(format!("busy: {:?}", frame.phase));
        }
        engine.set_visible(visible);
        frame.phase = Phase::Settling;
        frame.settle = frame.settle.wrapping_add(1);
        let token = frame.settle;
        self.web_settle_later(id, token, wait, cx);
        Ok(json!({"settling_ms": wait, "visible": visible}))
    }

    fn web_probe(&mut self, id: NodeId, op: &Value) -> Result<Value, String> {
        let web = self.web.as_ref().ok_or("off")?;
        let to_view = web.to_view.clone();
        let engine = (web.frames.get(&id))
            .and_then(|frame| frame.engine.as_ref())
            .ok_or("no view")?;
        let script = op.get("js").and_then(Value::as_str).unwrap_or(
            "JSON.stringify(Object.assign({visibility: document.visibilityState}, window.__probe || {}))",
        );
        engine.eval_with_callback(script, move |json| {
            let _ = to_view.unbounded_send(WebMsg::Probe(id, json));
        })?;
        Ok(json!("probing"))
    }

    fn web_reload(&mut self, id: NodeId) -> Result<Value, String> {
        let web = self.web.as_mut().ok_or("off")?;
        let frame = web.frames.get_mut(&id).ok_or("no frame")?;
        let engine = frame.engine.as_ref().ok_or("no view")?;
        engine.reload();
        frame.phase = Phase::Loading;
        frame.built = Some(std::time::Instant::now());
        Ok(json!("reloading"))
    }
}
