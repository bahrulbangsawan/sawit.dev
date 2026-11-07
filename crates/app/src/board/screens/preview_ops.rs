//! Preview as `app_state` reports it, and the `web` op's ways to drive it for the e2e specs
//! (`preview_set`, `preview_custom`), as the user's clicks and typing would.

use gpui_kit::*;
use sawit_core::browser::DEVICES;

use super::preview::{Size, Zoom, custom_size};
use crate::board::BoardView;

impl BoardView {
    /// What `app_state` says of Preview: its size, zoom, device frame and where its page is.
    pub(in crate::board) fn preview_state(&self) -> serde_json::Value {
        let Some(p) = self.screen_ui.preview.as_ref() else {
            return serde_json::Value::Null;
        };
        let size = match p.size {
            Size::Made => "made".to_string(),
            Size::Window => "window".to_string(),
            Size::Device(i) => DEVICES.get(i).map_or("", |d| d.name).to_string(),
            Size::Custom(w, h) => format!("custom {w}x{h}"),
        };
        serde_json::json!({
            "size": size, "css": p.css_size((1.0, 1.0)), "rotated": p.rotated,
            "zoom": if p.zoom == Zoom::Actual { "100%" } else { "fit" },
            "device_frame": p.frame_shown(), "custom_open": p.custom.is_some(),
            "placed": p.placed.map(|(f, s)| serde_json::json!([f.x, f.y, f.width, f.height, s])),
        })
    }

    /// The `web` op's `preview_set` (e2e): `size` ("made", "window", a device's name or
    /// `[w, h]`), `zoom` ("fit" or "100%"), `frame` and `rotated`, as their controls set them.
    pub(in crate::board) fn preview_set(
        &mut self,
        op: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        if self.screen_ui.preview.is_none() {
            return Err("Preview isn't open".into());
        }
        let size = match op.get("size") {
            None => None,
            Some(serde_json::Value::Array(wh)) => {
                let n = |i: usize| wh.get(i).and_then(|v| v.as_u64()).map(|v| v as u32);
                let (w, h) = custom_size(n(0), n(1))?;
                Some(Size::Custom(w, h))
            }
            Some(v) => Some(match v.as_str().unwrap_or("") {
                "made" => Size::Made,
                "window" => Size::Window,
                name => Size::Device(
                    DEVICES
                        .iter()
                        .position(|d| d.name == name)
                        .ok_or(format!("no device {name}"))?,
                ),
            }),
        };
        let zoom = op.get("zoom").and_then(|v| v.as_str()).map(|z| match z {
            "100%" | "actual" => Zoom::Actual,
            _ => Zoom::Fit,
        });
        let framed = op.get("frame").and_then(|v| v.as_bool());
        let rotated = op.get("rotated").and_then(|v| v.as_bool());
        self.preview_change(
            |p| {
                if let Some(size) = size {
                    p.size = size;
                }
                if let Some(zoom) = zoom {
                    p.zoom = zoom;
                }
                if let Some(framed) = framed {
                    p.framed = framed;
                }
                if let Some(rotated) = rotated {
                    p.rotated = rotated;
                }
            },
            cx,
        );
        Ok(self.preview_state())
    }
}

impl BoardView {
    /// The `web` op's `preview_custom` (e2e): Custom size opened, `w` and `h` typed, and Apply
    /// pressed if `apply`, as the user would.
    pub(in crate::board) fn preview_custom_typed(
        &mut self,
        (w, h, apply): (String, String, bool),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .screen_ui
            .preview
            .as_ref()
            .is_some_and(|p| p.custom.is_none())
        {
            self.preview_custom_start(window, cx);
        }
        let Some(edit) = self
            .screen_ui
            .preview
            .as_ref()
            .and_then(|p| p.custom.as_ref())
        else {
            return;
        };
        let (wi, hi) = (edit.w.clone(), edit.h.clone());
        wi.update(cx, |input, cx| input.set_value(w, window, cx));
        hi.update(cx, |input, cx| input.set_value(h, window, cx));
        if apply {
            self.preview_custom_apply(window, cx);
        }
    }
}
