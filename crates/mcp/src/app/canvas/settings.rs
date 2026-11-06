//! canvas_settings: how the canvas plays agents' builds (30f). With the app running, its
//! `settings` op changes them at once; with it closed, they are written to the file the app reads
//! them from when it starts (`agent-cursors.json` in Sawit's folder).

use rmcp::model::CallToolResult;
use serde_json::{Map, Value, json};

use super::CanvasSettingsArgs;
use super::dispatch::Offered;
use crate::app::pretty;
use crate::{Sawit, blocking, fail, ok};

impl Sawit {
    pub(super) async fn canvas_settings_tool(&self, a: CanvasSettingsArgs) -> CallToolResult {
        let build = a
            .build_animation
            .as_deref()
            .map(str::trim)
            .map(str::to_lowercase);
        if let Some(b) = build
            .as_deref()
            .filter(|b| !matches!(*b, "full" | "fast" | "off"))
        {
            return fail(format!("build_animation: full, fast or off, not “{b}”"));
        }
        let op = json!({
            "op": "settings", "build_animation": build, "sound": a.sound,
            "subagent_cursors": a.subagent_cursors,
        });
        let file = match super::files::canvas_file(a.file.as_deref()) {
            Ok(file) => file,
            Err(e) => return fail(e),
        };
        match self.offer_to_app(a.project, file, op).await {
            Ok(Offered::Answered(reply)) => match reply {
                Ok(v) if v.get("ok") == Some(&json!(true)) => ok(pretty(&v)),
                Ok(v) => fail(
                    v.get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("refused")
                        .to_string(),
                ),
                Err(e) => fail(e),
            },
            Ok(Offered::NotRunning(op, _)) => {
                let file = self.paths.root.join("agent-cursors.json");
                let saved = blocking(move || save(&file, &op)).await;
                match saved {
                    Ok(Ok(mut v)) => {
                        v["note"] = json!("Sawit isn't running: saved for its next start");
                        ok(pretty(&v))
                    }
                    Ok(Err(e)) | Err(e) => fail(e),
                }
            }
            Err(e) => fail(e),
        }
    }
}

/// Merges the settings `op` gives into `file`; answers them all.
fn save(file: &std::path::Path, op: &Value) -> Result<Value, String> {
    let mut saved: Map<String, Value> = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    for (key, to) in [
        ("build_animation", "build_animation"),
        ("sound", "build_sound"),
        ("subagent_cursors", "subagent_cursors"),
    ] {
        if let Some(value) = op.get(key).filter(|v| !v.is_null()) {
            saved.insert(to.to_string(), value.clone());
        }
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Unable to create {}: {e}", dir.display()))?;
    }
    let text = Value::Object(saved.clone()).to_string();
    std::fs::write(file, text).map_err(|e| format!("Unable to write {}: {e}", file.display()))?;
    Ok(json!({
        "ok": true,
        "build_animation": saved.get("build_animation").cloned().unwrap_or(json!("full")),
        "build_sound": saved.get("build_sound").cloned().unwrap_or(json!(false)),
        "subagent_cursors": saved.get("subagent_cursors").cloned().unwrap_or(json!(true)),
    }))
}
