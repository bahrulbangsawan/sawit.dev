//! `canvas_export`: the page on show as one SVG or PNG file, with the design lint's summary of
//! it (B12), so an agent checks its own work before it says it is done.

use std::path::Path;

use rmcp::model::{CallToolResult, ContentBlock as Content};
use sawit_board::store;
use serde_json::json;

use super::CanvasExportArgs;
use super::files::{canvas_file, export_path};
use crate::app::{base64_decode, pretty};
use crate::{Sawit, fail, tri};

impl Sawit {
    pub(super) async fn canvas_export_tool(&self, a: CanvasExportArgs) -> CallToolResult {
        let (lint_project, lint_file) = (a.project.clone(), a.file.clone());
        let lint = a.lint != Some(false);
        let format = a.format.as_deref().map(str::trim).unwrap_or_default();
        let png = match format.to_lowercase().as_str() {
            "svg" | "" => false,
            "png" => true,
            _ => return fail(format!("Unknown format “{format}”: use svg or png.")),
        };
        // Read as every tool that writes a file reads its path (MCP-11): absolute, and never a
        // canvas file, since nothing else holds the board when the app is closed.
        let named = tri!(crate::output_path(
            "canvas_export",
            a.output_path.as_deref()
        ));
        let op = json!({ "op": if png { "export_png" } else { "export_svg" } });
        let project = crate::agent_project(a.project.as_deref());
        // Named after the canvas file: `canvas-…` for the project's, `home-…` for home.sawit.
        let canvas = tri!(canvas_file(a.file.as_deref()));
        let stem = canvas.as_deref().map(store::bound_file);
        let stem = stem.as_deref().and_then(Path::file_stem);
        let stem = stem.map_or_else(|| "canvas".into(), |stem| stem.to_string_lossy());
        let value = match self.canvas(a.project, a.file, op).await {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        let bytes = if png {
            match base64_decode(
                value["png_base64"].as_str().unwrap_or_default(),
                "The canvas",
            ) {
                Ok(b) => b,
                Err(e) => return fail(e),
            }
        } else {
            value["svg"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec()
        };
        let path = named.unwrap_or_else(|| {
            export_path(
                project.as_deref(),
                &self.paths.cache_dir(),
                &stem,
                if png { "png" } else { "svg" },
            )
        });
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return fail(format!("Unable to create {}: {e}", dir.display()));
        }
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                let said = format!(
                    "Canvas exported to {} ({} bytes)",
                    path.display(),
                    bytes.len()
                );
                let mut sent = vec![Content::text(said)];
                let op = json!({"op": "lint"});
                if lint && let Ok(found) = self.canvas(lint_project, lint_file, op).await {
                    let first: Vec<_> = (found["findings"].as_array().into_iter().flatten())
                        .take(5)
                        .map(|f| json!({"rule": f["rule"], "id": f["id"], "fix": f["fix"]}))
                        .collect();
                    let summary = json!({"lint": {
                        "errors": found["errors"], "warnings": found["warnings"], "first": first,
                        "more": "canvas_lint lists every finding with its fix",
                    }});
                    sent.push(Content::text(pretty(&summary)));
                }
                CallToolResult::success(sent)
            }
            Err(e) => fail(format!("Unable to write {}: {e}", path.display())),
        }
    }
}
