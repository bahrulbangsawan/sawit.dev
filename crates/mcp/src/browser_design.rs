//! `browser_inspect`, `browser_design`, `browser_export_design`: the design
//! of the page in the app's browser tab (see `sawit_core::browser`).
//!
//! The export runs in the app in the background; each control request has a
//! 30 s timeout, so it is started once and polled here.

use std::time::{Duration, Instant};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::browser::BrowserOp;
use serde_json::Value;

use crate::app::pretty;
use crate::browser::{required, target};
use crate::{Sawit, clip, fail, ok, tri};

fn clip_out(text: String) -> String {
    clip(text, 60_000)
}

const FORMATS: [&str; 4] = ["json", "css", "tailwind", "html"];
const EXPORT_POLL: Duration = Duration::from_millis(500);
const EXPORT_LIMIT: Duration = Duration::from_secs(600);

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InspectArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Element ref from browser_snapshot, e.g. e12"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "json (default), css, tailwind or html")]
    pub format: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DesignArgs {
    #[schemars(description = "summary (default), colors, fonts, assets, tokens or variables")]
    pub part: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExportDesignArgs {
    #[schemars(description = "Also download the page's videos (default false)")]
    #[serde(default)]
    pub include_videos: bool,
    pub project: Option<String>,
}

/// The op for `browser_inspect`, or what's wrong with the arguments.
fn inspect_op(
    reference: Option<String>,
    selector: Option<String>,
    format: Option<String>,
) -> Result<BrowserOp, String> {
    let target = required(target(reference, selector))?;
    if let Some(f) = format.as_deref()
        && !FORMATS.contains(&f)
    {
        return Err(format!(
            "Unknown format “{f}”. Use json, css, tailwind or html."
        ));
    }
    Ok(BrowserOp::Inspect { target, format })
}

/// The finished export as text (folder, failed downloads, files), or the
/// job's error.
fn export_report(v: &Value) -> Result<String, String> {
    let folder = v["folder"].as_str().unwrap_or("");
    if let Some(e) = v["error"].as_str() {
        let mut out = format!("The export failed: {e}");
        if !folder.is_empty() {
            out.push_str(&format!("\nFolder: {folder}"));
        }
        return Err(out);
    }
    let mut out = format!("Exported to {folder}\n");
    if let Some(e) = v["screenshot_error"].as_str() {
        out.push_str(&format!("No screenshot.png: {e}\n"));
    }
    let failed = v["failed"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if !failed.is_empty() {
        out.push_str(&format!("\n{} downloads failed:\n", failed.len()));
        for f in failed {
            let what = f["url"]
                .as_str()
                .or_else(|| f["label"].as_str())
                .unwrap_or("asset");
            let why = f["error"].as_str().unwrap_or("failed");
            out.push_str(&format!("{what} — {why}\n"));
        }
    }
    let files: Vec<&str> = v["files"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    out.push_str(&format!("\n{} files:\n", files.len()));
    for f in &files {
        out.push_str(&format!("{f}\n"));
    }
    Ok(clip_out(out))
}

#[tool_router(router = browser_design_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Design details of one element in the browser tab: box model, typography, colors with contrast, :hover/:focus states; format json (default), css, tailwind or html"
    )]
    async fn browser_inspect(&self, Parameters(a): Parameters<InspectArgs>) -> CallToolResult {
        let op = tri!(inspect_op(a.reference, a.selector, a.format));
        let v = tri!(self.browser(&a.project, op).await);
        let text = match (
            v["css"].as_str(),
            v["tailwind"].as_str(),
            v["html"].as_str(),
        ) {
            (Some(s), _, _) | (_, Some(s), _) | (_, _, Some(s)) => {
                format!("{}\n{s}", v["selector"].as_str().unwrap_or(""))
            }
            _ => pretty(&v),
        };
        ok(clip_out(text))
    }

    #[tool(
        description = "The design system of the page in the browser tab: colors, fonts, assets, CSS variables and derived tokens (palette ramps, type scale, spacing, radius, shadows); part summary (default), colors, fonts, assets, tokens or variables"
    )]
    async fn browser_design(&self, Parameters(a): Parameters<DesignArgs>) -> CallToolResult {
        let v = tri!(
            self.browser(&a.project, BrowserOp::Design { part: a.part })
                .await
        );
        ok(clip_out(pretty(&v)))
    }

    #[tool(
        description = "Export the page's design system to .nyawit/design/<host>/: DESIGN.md, tokens.json (DTCG), theme.css (Tailwind v4), shadcn.css, components and assets. Waits for downloads (up to 10 minutes)"
    )]
    async fn browser_export_design(
        &self,
        Parameters(a): Parameters<ExportDesignArgs>,
    ) -> CallToolResult {
        tri!(
            self.browser(
                &a.project,
                BrowserOp::ExportDesign {
                    include_videos: a.include_videos
                }
            )
            .await
        );
        let deadline = Instant::now() + EXPORT_LIMIT;
        loop {
            tokio::time::sleep(EXPORT_POLL).await;
            let v = tri!(self.browser(&a.project, BrowserOp::ExportStatus).await);
            if v["done"] == true {
                return match export_report(&v) {
                    Ok(t) => ok(t),
                    Err(e) => fail(e),
                };
            }
            if Instant::now() >= deadline {
                let progress = match (v["progress"][0].as_u64(), v["progress"][1].as_u64()) {
                    (Some(d), Some(t)) => format!(" ({d} of {t} downloads)"),
                    _ => String::new(),
                };
                return fail(format!(
                    "The export is still running after 10 minutes{progress}. It keeps going in the app; folder: {}",
                    v["folder"].as_str().unwrap_or("")
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inspect_needs_a_target() {
        assert_eq!(
            inspect_op(None, Some("  ".into()), None),
            Err("Give a ref (from browser_snapshot) or a CSS selector.".into())
        );
    }

    #[test]
    fn format_is_checked() {
        assert_eq!(
            inspect_op(Some("e1".into()), None, Some("scss".into())),
            Err("Unknown format “scss”. Use json, css, tailwind or html.".into())
        );
        assert!(inspect_op(None, Some("h1".into()), Some("css".into())).is_ok());
    }

    #[test]
    fn export_report_lists_files_and_failures() {
        let v = json!({
            "done": true, "folder": "/p/.nyawit/design/a.dev",
            "files": ["DESIGN.md"], "screenshot_error": "The tab moved.",
            "failed": [{"url": "https://a.dev/x.png", "error": "HTTP 404"}],
        });
        let t = export_report(&v).unwrap();
        assert!(t.find("downloads failed") < t.find("files:"));
        assert!(t.contains("Exported to /p/.nyawit/design/a.dev"));
        assert!(t.contains("DESIGN.md"));
        assert!(t.contains("https://a.dev/x.png — HTTP 404"));
        assert!(t.contains("The tab moved."));
        let e = export_report(&json!({"done": true, "folder": "/f", "error": "disk full"}));
        assert!(e.unwrap_err().starts_with("The export failed: disk full"));
    }

    #[test]
    fn output_is_clip_out() {
        assert!(clip_out("é".repeat(40_000)).len() < 60_100);
    }
}
