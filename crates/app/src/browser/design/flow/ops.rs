//! Agents' design ops (`Design`, `Inspect`, `ExportDesign`, `ExportStatus`), each answer within
//! a tool's output cap.

use futures::channel::oneshot;
use gpui_kit::Context;
use sawit_core::site_design::inspect::{StyleInfo, contrast, css_rule};
use sawit_core::site_design::scan::Scan;
use sawit_core::site_design::tailwind::classes;
use sawit_core::site_design::tokens::Tokens;
use serde_json::{Value, json};

use super::export::{ExportJob, earlier_export, export_host, folder_for};
use super::{Ready, extractable};
use crate::browser::{BrowserView, OpResult};

const NO_EXPORT: &str = "No export has run in this browser tab.";

const PARTS: [&str; 6] = [
    "summary",
    "colors",
    "fonts",
    "assets",
    "tokens",
    "variables",
];

const FORMATS: [&str; 4] = ["json", "css", "tailwind", "html"];

/// Tool output cap.
const MAX_OUTPUT: usize = 60_000;

const CUT_NOTE: &str = "Cut at 60 KB; ask for one part.";

const STATUS_CUT_NOTE: &str =
    "Cut at 60 KB; the full lists are in the folder's README.md and assets.json.";

impl BrowserView {
    /// `Design`: the scan, then one part of it.
    pub(in crate::browser) fn design_op(
        &mut self,
        part: Option<String>,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let part = part.unwrap_or_else(|| "summary".into());
        if !PARTS.contains(&part.as_str()) {
            let _ = reply.send(design_part(&empty_ready(), &part));
            return;
        }
        let ready = self.scan_design(cx);
        cx.spawn(async move |_, _| {
            let _ = reply.send(ready.await.and_then(|r| design_part(&r, &part)));
        })
        .detach();
    }

    /// `Inspect`: an agent's ask, answered whatever the panel inspects meanwhile.
    pub(in crate::browser) fn inspect_op(
        &mut self,
        target: Value,
        format: Option<String>,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let format = format.unwrap_or_else(|| "json".into());
        if !FORMATS.contains(&format.as_str()) {
            let _ = reply.send(Err(format!(
                "Unknown format “{format}”. Use json, css, tailwind or html."
            )));
            return;
        }
        let info = self.style_info(target, format == "html", cx);
        cx.spawn(async move |_, _| {
            let _ = reply.send(info.await.and_then(|i| inspect_answer(&i, &format)));
        })
        .detach();
    }

    /// `ExportDesign`: `{"export": "started"|"running", "folder"}`. An agent's export replaces
    /// what an earlier one left in the folder only when it says so, with `replace`
    /// ([`earlier_export`]); joining the export that runs replaces nothing more.
    pub(in crate::browser) fn export_op(
        &mut self,
        include_videos: bool,
        replace: bool,
        cx: &mut Context<Self>,
    ) -> OpResult {
        let running = self.design.export_running();
        let url = self.current_url();
        if !running && !replace && extractable(&url) {
            let folder = folder_for(self.project.as_deref(), &export_host(&url));
            if let Some(held) = earlier_export(&folder) {
                return Err(held);
            }
        }
        let folder = self.start_export(include_videos, cx)?;
        Ok(json!({
            "export": if running { "running" } else { "started" },
            "folder": folder,
        }))
    }

    /// `ExportStatus`.
    pub(in crate::browser) fn export_status_op(&self) -> OpResult {
        self.design
            .export
            .as_ref()
            .map(export_status)
            .ok_or_else(|| NO_EXPORT.to_string())
    }
}

fn empty_ready() -> Ready {
    Ready {
        scan: Scan::default(),
        tokens: Tokens::default(),
    }
}

/// One part of the design, at most [`MAX_OUTPUT`] bytes.
pub(super) fn design_part(ready: &Ready, part: &str) -> Result<Value, String> {
    let (s, t) = (&ready.scan, &ready.tokens);
    let mut v = match part {
        "summary" => json!({
            "url": s.url,
            "title": s.title,
            "truncated": s.truncated,
            "counts": {
                "colors": s.colors.len(),
                "text_styles": s.texts.len(),
                "font_faces": s.font_faces.len(),
                "assets": s.assets.len(),
                "variables": s.variables.len(),
                "components": s.components.len(),
            },
            "tokens": t,
        }),
        "colors" => json!({ "colors": s.colors }),
        "fonts" => json!({
            "families": t.families,
            "font_faces": s.font_faces,
            "text_styles": s.texts,
        }),
        "assets" => json!({ "assets": s.assets }),
        "tokens" => json!({ "tokens": t }),
        "variables" => json!({ "variables": s.variables }),
        _ => {
            return Err(format!(
                "Unknown part “{part}”. Use summary, colors, fonts, assets, tokens or variables."
            ));
        }
    };
    cap(&mut v, CUT_NOTE);
    Ok(v)
}

fn size_of(v: &Value) -> usize {
    serde_json::to_vec(v).map_or(0, |b| b.len())
}

/// The JSON pointer and serialized size of the biggest non-empty array in `v`.
fn biggest_array(v: &Value, path: &str, best: &mut Option<(String, usize)>) {
    match v {
        Value::Array(items) => {
            if !items.is_empty() {
                let size = size_of(v);
                if best.as_ref().is_none_or(|(_, s)| size > *s) {
                    *best = Some((path.to_string(), size));
                }
            }
            for (i, item) in items.iter().enumerate() {
                biggest_array(item, &format!("{path}/{i}"), best);
            }
        }
        Value::Object(map) => {
            for (k, item) in map {
                let k = k.replace('~', "~0").replace('/', "~1");
                biggest_array(item, &format!("{path}/{k}"), best);
            }
        }
        _ => {}
    }
}

/// Cuts the biggest lists of the object `v` until it fits [`MAX_OUTPUT`] with `note` added.
fn cap(v: &mut Value, note: &str) {
    if size_of(v) <= MAX_OUTPUT {
        return;
    }
    let budget = MAX_OUTPUT - note.len() - 16;
    loop {
        let size = size_of(v);
        if size <= budget {
            break;
        }
        let mut best = None;
        biggest_array(v, "", &mut best);
        let Some((path, array_size)) = best else {
            // Nothing left to cut (one huge value): say so instead.
            *v = json!({});
            break;
        };
        let Some(Value::Array(items)) = v.pointer_mut(&path) else {
            break;
        };
        let per_item = (array_size / items.len()).max(1);
        let remove = (size - budget).div_ceil(per_item);
        items.truncate(items.len().saturating_sub(remove.max(1)));
    }
    v["note"] = note.into();
}

pub(super) fn inspect_answer(info: &StyleInfo, format: &str) -> OpResult {
    match format {
        "json" => {
            let mut v = serde_json::to_value(info).map_err(|e| e.to_string())?;
            if let Some(map) = v.as_object_mut() {
                map.remove("html");
            }
            v["contrast"] = contrast(info).map_or(Value::Null, |c| {
                json!({
                    "ratio": (c.ratio * 100.).round() / 100.,
                    "large": c.large,
                    "aa": c.aa,
                    "aaa": c.aaa,
                })
            });
            Ok(v)
        }
        "css" => Ok(json!({ "selector": info.selector, "css": css_rule(info) })),
        "tailwind" => Ok(json!({ "selector": info.selector, "tailwind": classes(info).join(" ") })),
        "html" => Ok(json!({ "selector": info.selector, "html": info.html })),
        _ => Err(format!(
            "Unknown format “{format}”. Use json, css, tailwind or html."
        )),
    }
}

pub(super) fn export_status(job: &ExportJob) -> Value {
    let (files, failed, skipped_videos, error) = match &job.result {
        Some(Ok(report)) => (
            report.files.as_slice(),
            report.failed.as_slice(),
            report.skipped_videos,
            None,
        ),
        Some(Err(e)) => (&[][..], &[][..], 0, Some(e.as_str())),
        None => (&[][..], &[][..], 0, None),
    };
    let screenshot_error = match &job.result {
        Some(Ok(report)) => report.screenshot_error.as_deref(),
        _ => None,
    };
    let folder = match &job.result {
        Some(Ok(report)) => &report.folder,
        _ => &job.folder,
    };
    let mut v = json!({
        "done": job.result.is_some(),
        "progress": [job.done, job.total],
        "folder": folder,
        "files": files,
        "failed": failed,
        "skipped_videos": skipped_videos,
        "screenshot_error": screenshot_error,
        "error": error,
    });
    cap(&mut v, STATUS_CUT_NOTE);
    v
}
