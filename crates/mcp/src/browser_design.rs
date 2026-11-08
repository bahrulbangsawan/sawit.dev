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
    #[serde(flatten)]
    pub at: crate::browser::At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DesignArgs {
    #[schemars(description = "summary (default), colors, fonts, assets, tokens or variables")]
    pub part: Option<String>,
    #[serde(flatten)]
    pub at: crate::browser::At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExportDesignArgs {
    #[schemars(description = "Also download the page's videos (default false)")]
    #[serde(default)]
    pub include_videos: bool,
    #[schemars(
        description = "Replace what is in the host's export folder: an earlier export, a DESIGN.md edited by hand included. Without it (default false), a folder that holds anything is refused and nothing is written"
    )]
    #[serde(default)]
    pub replace: bool,
    #[serde(flatten)]
    pub at: crate::browser::At,
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
    match v["skipped_videos"].as_u64().unwrap_or(0) {
        0 => {}
        n => out.push_str(&format!(
            "\n{n} video{} not included (include_videos: true downloads them).\n",
            if n == 1 { " was" } else { "s were" }
        )),
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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PanelArgs {
    #[schemars(
        description = "annotate (the notes the user leaves on the page), design (the page's design) or seo (the SEO and agent-readiness scan); leave it out to close the open panel"
    )]
    pub panel: Option<String>,
    #[schemars(
        description = "The panel's tab: design's inspect, colors, fonts, assets or system; seo's overview, previews, checks or agents"
    )]
    pub tab: Option<String>,
    #[serde(flatten)]
    pub at: crate::browser::At,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DevtoolsArgs {
    #[serde(flatten)]
    pub at: crate::browser::At,
}

/// `panel` and `tab` checked against the panels and tabs there are.
pub(crate) fn panel_op(panel: Option<String>, tab: Option<String>) -> Result<BrowserOp, String> {
    let panel = panel
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty());
    let tabs: &[&str] = match panel.as_deref() {
        None | Some("annotate") => &[],
        Some("design") => &["inspect", "colors", "fonts", "assets", "system"],
        Some("seo") => &["overview", "previews", "checks", "agents"],
        Some(other) => {
            return Err(format!(
                "Unknown panel “{other}”: use annotate, design or seo, or leave it out to close."
            ));
        }
    };
    let tab = tab
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty());
    if let Some(t) = &tab
        && !tabs.contains(&t.as_str())
    {
        return Err(match panel.as_deref() {
            Some(p) if !tabs.is_empty() => {
                format!("{p} has no tab “{t}”: use {}.", tabs.join(", "))
            }
            _ => "Only the design and seo panels have tabs.".to_string(),
        });
    }
    Ok(BrowserOp::Panel { panel, tab })
}

#[tool_router(router = browser_design_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Open a side panel of the browser tab, as its toolbar buttons do: annotate (the user's notes on the page), design (the page's design, on a tab: inspect, colors, fonts, assets, system) or seo (the scan, on a tab: overview, previews, checks, agents); with no panel, close the open one. For the user to see: agents read notes, design and SEO through browser_annotations, browser_design and browser_seo, panel or not. Answers the panel open now. Needs the app running"
    )]
    async fn browser_panel(&self, Parameters(a): Parameters<PanelArgs>) -> CallToolResult {
        let op = tri!(panel_op(a.panel, a.tab));
        let v = tri!(self.browser(&a.at, op).await);
        ok(pretty(&v))
    }

    #[tool(
        description = "Open Safari's Web Inspector on the browser tab's page, as the toolbar's Inspect Element button does, for the user to debug with. Agents read the page with browser_snapshot, browser_console, browser_network and browser_evaluate. Needs the app running"
    )]
    async fn browser_devtools(&self, Parameters(a): Parameters<DevtoolsArgs>) -> CallToolResult {
        let v = tri!(self.browser(&a.at, BrowserOp::Devtools).await);
        ok(pretty(&v))
    }

    #[tool(
        description = "Design details of one element in the browser tab: box model, typography, colors with contrast, :hover/:focus states; format json (default), css, tailwind or html"
    )]
    async fn browser_inspect(&self, Parameters(a): Parameters<InspectArgs>) -> CallToolResult {
        let op = tri!(inspect_op(a.reference, a.selector, a.format));
        let v = tri!(self.browser(&a.at, op).await);
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
            self.browser(&a.at, BrowserOp::Design { part: a.part })
                .await
        );
        ok(clip_out(pretty(&v)))
    }

    #[tool(
        description = "Export the page's design system to the host's folder in the browser's project, .nyawit/design/<host>/ (a dev server's keeps its port, localhost-5173; for the browser of no project ~/Downloads/sawit-design/<host>/): DESIGN.md, tokens.json (DTCG), theme.css (Tailwind v4), shadcn.css, components and assets. DESTRUCTIVE: it replaces everything an earlier export left in that folder, a DESIGN.md edited by hand included, so a folder that holds anything needs replace: true; without it the answer lists what is there and nothing is written. Waits for downloads (up to 10 minutes)"
    )]
    async fn browser_export_design(
        &self,
        Parameters(a): Parameters<ExportDesignArgs>,
    ) -> CallToolResult {
        tri!(
            self.browser(
                &a.at,
                BrowserOp::ExportDesign {
                    include_videos: a.include_videos,
                    replace: a.replace,
                }
            )
            .await
        );
        let deadline = Instant::now() + EXPORT_LIMIT;
        loop {
            tokio::time::sleep(EXPORT_POLL).await;
            let v = tri!(self.browser(&a.at, BrowserOp::ExportStatus).await);
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
        assert!(!t.contains("not included"));
        let v = json!({"done": true, "folder": "/f", "files": [], "failed": [],
            "skipped_videos": 2});
        let t = export_report(&v).unwrap();
        assert!(!t.contains("downloads failed"));
        assert!(t.contains("2 videos were not included (include_videos: true downloads them)."));
        let e = export_report(&json!({"done": true, "folder": "/f", "error": "disk full"}));
        assert!(e.unwrap_err().starts_with("The export failed: disk full"));
    }

    /// browser_export_design says where it writes and that it replaces what an earlier export
    /// left there, a hand-edited DESIGN.md included, which it does only with `replace: true`; the
    /// op it sends carries that, and replaces nothing by default.
    #[test]
    fn the_export_says_what_it_replaces_and_asks_for_replace() {
        let tools = Sawit::router().list_all();
        let tool = tools
            .iter()
            .find(|tool| tool.name == "browser_export_design")
            .expect("browser_export_design");
        let sent = serde_json::to_string(tool).unwrap();
        for said in [
            ".nyawit/design/<host>/",
            "localhost-5173",
            "~/Downloads/sawit-design/<host>/",
            "DESTRUCTIVE: it replaces everything an earlier export left in that folder, a DESIGN.md edited by hand included",
            "needs replace: true",
        ] {
            assert!(sent.contains(said), "no {said:?} in {sent}");
        }
        let args: ExportDesignArgs = serde_json::from_value(json!({})).unwrap();
        assert!(!args.replace && !args.include_videos);
        let args: ExportDesignArgs = serde_json::from_value(json!({"replace": true})).unwrap();
        assert!(args.replace);
    }

    #[test]
    fn output_is_clip_out() {
        assert!(clip_out("é".repeat(40_000)).len() < 60_100);
    }
}

#[cfg(test)]
mod panel_tests {
    use sawit_core::browser::BrowserOp;

    use super::panel_op;

    #[test]
    fn a_panel_and_its_tab_are_checked() {
        let op = panel_op(Some("SEO".into()), Some("agents".into())).unwrap();
        let want = BrowserOp::Panel {
            panel: Some("seo".into()),
            tab: Some("agents".into()),
        };
        assert_eq!(format!("{op:?}"), format!("{want:?}"));
        let closed = panel_op(None, None).unwrap();
        assert_eq!(
            format!("{closed:?}"),
            format!(
                "{:?}",
                BrowserOp::Panel {
                    panel: None,
                    tab: None
                }
            )
        );
        let e = panel_op(Some("seo".into()), Some("colors".into())).unwrap_err();
        assert!(e.contains("overview"), "{e}");
        let e = panel_op(Some("ruler".into()), None).unwrap_err();
        assert!(e.contains("annotate, design or seo"), "{e}");
    }
}
