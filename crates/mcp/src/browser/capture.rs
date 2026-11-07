//! Captures from the agent's arguments: the record op, and screenshots at one or more sizes.

use rmcp::model::{CallToolResult, ContentBlock as Content};
use sawit_core::browser::BrowserOp;

use super::mesurer::screenshot_mesurer;
use super::{RecordArgs, ScreenshotArgs, target};
use crate::{Sawit, fail, tri};

/// The record op from the agent's arguments.
pub(super) fn record_op(a: RecordArgs) -> Result<BrowserOp, String> {
    let output_path = match a.output_path.as_deref() {
        Some(p) => {
            crate::output_path("browser_record_start", Some(p))?.map(|p| p.display().to_string())
        }
        None => None,
    };
    Ok(BrowserOp::RecordStart {
        target: target(a.reference, a.selector),
        mode: a.mode,
        device: a.device,
        width: a.width,
        height: a.height,
        resolution: a.resolution,
        max_width: a.max_width,
        fps: a.fps,
        format: a.format,
        clip: a.clip,
        scroll_speed: a.scroll_speed,
        duration: a.duration,
        max_seconds: a.max_seconds,
        output_path,
    })
}

impl Sawit {
    /// `browser_screenshot` with sizes: one Capture per device (or one for the size asked),
    /// each answered as a preview image and a line with its file and sizes.
    pub(super) async fn sized_screenshot(&self, a: ScreenshotArgs) -> CallToolResult {
        let output = tri!(crate::output_path(
            "browser_screenshot",
            a.output_path.as_deref()
        ));
        let devices: Vec<Option<String>> = if a.devices.is_empty() {
            vec![a.device.clone()]
        } else {
            a.devices.iter().cloned().map(Some).collect()
        };
        if devices.len() > 1 && output.is_some() {
            return fail(
                "output_path names one file: leave it out with devices (each gets its own name).",
            );
        }
        let mesurer = tri!(screenshot_mesurer(a.mesurer.as_deref()));
        let mut content = Vec::new();
        let mut lines = Vec::new();
        for device in devices {
            let op = BrowserOp::Capture {
                target: target(a.reference.clone(), a.selector.clone()),
                full_page: a.full_page,
                device: device.clone(),
                width: a.width,
                height: a.height,
                scale: a.scale,
                format: a.format.clone(),
                quality: a.quality,
                clip: a.clip,
                output_path: output.as_ref().map(|p| p.display().to_string()),
                mesurer: mesurer.clone(),
            };
            let v = tri!(self.browser(&a.at, op).await);
            if let Some(preview) = v["preview_png_base64"].as_str() {
                content.push(Content::image(preview.to_string(), "image/png"));
            }
            let mut line = format!(
                "{}: {}×{} px ({}×{} CSS px at {}×) written to {}",
                v["device"].as_str().unwrap_or("page"),
                v["width"],
                v["height"],
                v["css_width"],
                v["css_height"],
                v["scale"],
                v["path"].as_str().unwrap_or_default()
            );
            if v["width"].as_u64().unwrap_or(0) > 1568 {
                line.push_str(" (the image above is a smaller preview)");
            }
            for note in v["notes"].as_array().into_iter().flatten() {
                line.push_str(&format!(". {}", note.as_str().unwrap_or_default()));
            }
            lines.push(line);
        }
        content.push(Content::text(lines.join("\n")));
        CallToolResult::success(content)
    }
}
