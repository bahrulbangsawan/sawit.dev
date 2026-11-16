//! The screen tools (`canvas_screen_write`, `canvas_screen_get`, `canvas_screen_export`) and
//! `canvas_begin {kind: "screen"}`: UI screens, HTML pages in the canvas's screens folder shown in
//! frames ([`sawit_board::screens`]).
//!
//! With the app running, each is an op of the canvas there, which finds the screen, writes its
//! files and renders them, so the user watches it build. With the app closed, the screen is found
//! on the stored canvas and its files are written here; nothing renders, and the answer says so.

use std::path::{Path, PathBuf};

use rmcp::model::{CallToolResult, ContentBlock as Content};
use sawit_board::screens::{self, export, files};
use sawit_board::store;
use serde_json::{Value, json};

use super::CanvasScreenExportArgs;
use super::dispatch::{Offered, op_reply};
use super::files::canvas_file;
use super::stored::{CANVAS_LOCK, headless};
use crate::app::pretty;
use crate::{Sawit, blocking, fail, ok};

/// What a screen tool says when the app is closed.
pub(super) const UNRENDERED: &str = "Sawit isn't running, so nothing is rendered: the screen's \
     files and frame are saved, and the canvas renders it when the app opens it";

/// Where a screen op runs with the app closed: the canvas file, its screens folder, and the lock
/// every `sawit-mcp` takes to read or save it.
pub(super) struct Closed {
    pub named: PathBuf,
    pub dir: PathBuf,
    lock: PathBuf,
}

impl Closed {
    /// Runs `op` on the stored canvas ([`headless`]).
    pub fn run(&self, op: &Value) -> Result<Value, String> {
        headless(&self.named, &self.lock, op)
    }

    /// The screen `op`'s `work` or `screen` names, on the stored canvas, with its folder.
    pub fn find(&self, op: &Value) -> Result<Value, String> {
        let mut found = self.run(&json!({
            "op": "screen", "work": op.get("work"), "screen": op.get("screen"),
        }))?;
        if let Some(name) = found["screen"].as_str() {
            found["folder"] = json!(screens::folder(&self.dir, name));
        }
        Ok(found)
    }
}

impl Sawit {
    /// Runs screen op `op` on the running app, or, with it closed, runs `closed` off the async
    /// threads on the stored canvas and the screen's files.
    pub(super) async fn screen_call(
        &self,
        project: Option<String>,
        file: Option<String>,
        op: Value,
        closed: impl FnOnce(&Closed, Value) -> Result<Value, String> + Send + 'static,
    ) -> Result<Value, String> {
        let file = canvas_file(file.as_deref())?;
        let (op, file) = match self.offer_to_app(project.clone(), file, op).await? {
            Offered::Answered(reply) => return op_reply(reply?),
            Offered::NotRunning(op, file) => (op, file),
        };
        let named = match file {
            Some(file) => file,
            None => self.canvas_board_file(project.as_deref())?,
        };
        let at = Closed {
            dir: screens::screens_dir(&store::bound_file(&named)),
            named,
            lock: self.paths.root.join(CANVAS_LOCK),
        };
        blocking(move || closed(&at, op)).await?
    }

    /// `canvas_begin {kind: "screen"}`: the frame, and the screen's folder, which the app makes
    /// itself while it runs.
    pub(super) async fn begin_screen(
        &self,
        project: Option<String>,
        file: Option<String>,
        op: Value,
    ) -> Result<Value, String> {
        self.screen_call(project, file, op, |at, mut op| {
            op["taken"] = json!(screens::folders(&at.dir));
            let mut reply = at.run(&op)?;
            let name = reply["screen"].as_str().unwrap_or_default().to_string();
            let folder = files::make_folder(&at.dir, &name)?;
            reply["folder"] = json!(folder);
            reply["rendered"] = json!(false);
            reply["note"] = json!(UNRENDERED);
            Ok(reply)
        })
        .await
    }

    pub async fn canvas_screen_write_tool(
        &self,
        a: super::CanvasScreenWriteArgs,
    ) -> CallToolResult {
        let op = json!({
            "op": "screen_write", "work": a.work, "screen": a.screen, "html": a.html,
            "edits": a.edits, "files": a.files,
        });
        let op = super::with_lane(op, a.agent.as_ref());
        if let Err(e) = names_one(&op).and_then(|()| files::Write::from_op(&op).map(drop)) {
            return fail(format!("canvas_screen_write: {e}"));
        }
        let (project, file) = (a.project.clone(), a.file.clone());
        let written = self.screen_call(a.project, a.file, op, |at, op| {
            let write = files::Write::from_op(&op)?;
            let mut found = at.find(&op)?;
            let folder = PathBuf::from(found["folder"].as_str().unwrap_or_default());
            found["written"] = json!(files::apply(&folder, &write)?);
            found["rendered"] = json!(false);
            found["note"] = json!(UNRENDERED);
            Ok(found)
        });
        let mut v = match written.await {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        // What the standard's machine checks find in the page as written, also shown on its
        // frame in the app.
        let shown = v["rendered"] == json!(true);
        if let Some(checks) = self.screen_checks(project, file, &v, shown).await {
            v["design_checks"] = checks;
        }
        ok(pretty(&v))
    }

    pub async fn canvas_screen_get_tool(&self, a: super::CanvasScreenGetArgs) -> CallToolResult {
        let op = json!({"op": "screen_get", "work": a.work, "screen": a.screen});
        if names_one(&op).is_err() {
            let listed = self
                .canvas(a.project, a.file, json!({"op": "screen"}))
                .await;
            return match listed {
                Ok(v) => ok(pretty(&v)),
                Err(e) => fail(e),
            };
        }
        let (project, file) = (a.project.clone(), a.file.clone());
        let got = self.screen_call(a.project, a.file, op, |at, op| {
            let mut found = at.find(&op)?;
            let folder = PathBuf::from(found["folder"].as_str().unwrap_or_default());
            found["files"] = json!(files::list(&folder));
            found["html"] = json!(files::page(&folder));
            found["rendered"] = json!(false);
            found["note"] = json!(UNRENDERED);
            Ok(found)
        });
        match got.await {
            Ok(mut v) => {
                // The design standard's machine checks of its page, as a write answers them (B12).
                if let Some(checks) = self.screen_checks(project, file, &v, false).await {
                    v["design_checks"] = checks;
                }
                let png = v.get_mut("png_base64").map(Value::take);
                if let Some(map) = v.as_object_mut() {
                    map.remove("png_base64");
                }
                let mut content = Vec::new();
                if let Some(Value::String(png)) = png.filter(|p| p.as_str() != Some("")) {
                    content.push(Content::image(png, "image/png"));
                }
                content.push(Content::text(pretty(&v)));
                CallToolResult::success(content)
            }
            Err(e) => fail(e),
        }
    }

    pub async fn canvas_screen_export_tool(&self, a: CanvasScreenExportArgs) -> CallToolResult {
        let react = match a.format.as_deref().map(str::trim).unwrap_or_default() {
            "" | "html" | "HTML" => false,
            "react" | "React" | "tsx" => true,
            other => return fail(format!("Unknown format “{other}”: use html or react.")),
        };
        if let Some(id) = a.id.clone().filter(|id| !id.trim().is_empty()) {
            return self.artboard_export(a, id, react).await;
        }
        let op = json!({"op": "screen", "work": a.work, "screen": a.screen});
        if let Err(e) = names_one(&op) {
            return fail(format!("canvas_screen_export: {e} (or an artboard's `id`)"));
        }
        let named = match crate::new_output_path(
            "canvas_screen_export",
            a.path.as_deref(),
            a.overwrite.unwrap_or(false),
        ) {
            Ok(named) => named,
            Err(e) => return fail(e),
        };
        let project = crate::agent_project(a.project.as_deref());
        let cache = self.paths.cache_dir();
        let found = self.screen_call(a.project, a.file, op, |at, op| at.find(&op));
        let found = match found.await {
            Ok(found) => found,
            Err(e) => return fail(e),
        };
        let name = found["screen"].as_str().unwrap_or_default().to_string();
        let folder = PathBuf::from(found["folder"].as_str().unwrap_or_default());
        let exported = blocking(move || {
            let path =
                named.unwrap_or_else(|| default_export(project.as_deref(), &cache, &name, react));
            match react {
                true => export::write_react(&folder, &name, &path),
                false => export::write_html(&folder, &path),
            }
        })
        .await;
        match exported {
            Ok(Ok(written)) => ok(pretty(&json!({
                "ok": true, "screen": found["screen"], "format": if react { "react" } else { "html" },
                "files": written.files, "notes": written.notes,
            }))),
            Ok(Err(e)) | Err(e) => fail(e),
        }
    }
}

impl Sawit {
    /// `canvas_screen_export {id}`: artboard `id` as HTML, or as React (`react`), written where
    /// `a.path` says or in the project's exports folder.
    async fn artboard_export(
        &self,
        a: CanvasScreenExportArgs,
        id: String,
        react: bool,
    ) -> CallToolResult {
        let named = match crate::new_output_path(
            "canvas_screen_export",
            a.path.as_deref(),
            a.overwrite.unwrap_or(false),
        ) {
            Ok(named) => named,
            Err(e) => return fail(e),
        };
        let component = named
            .as_deref()
            .filter(|_| react)
            .and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned());
        let op = json!({
            "op": "frame_code", "id": id, "format": if react { "react" } else { "html" },
            "component": component,
        });
        let project = crate::agent_project(a.project.as_deref());
        let cache = self.paths.cache_dir();
        let code = match self.canvas(a.project, a.file, op).await {
            Ok(code) => code,
            Err(e) => return fail(e),
        };
        let name = code["name"].as_str().unwrap_or("frame").to_string();
        let written = blocking(move || {
            let file = sawit_board::frame_code::file_name(&name, react);
            let path = named.unwrap_or_else(|| exports_path(project.as_deref(), &cache, &file));
            let mut files = Vec::new();
            let text = |key: &str| code[key].as_str().unwrap_or_default().to_string();
            write_file(&path, &text(if react { "tsx" } else { "html" }))?;
            files.push(path.clone());
            if react {
                let css = path.with_file_name(text("css_file"));
                write_file(&css, &text("css"))?;
                files.push(css);
            }
            Ok::<_, String>((files, code))
        })
        .await;
        match written {
            Ok(Ok((files, code))) => ok(pretty(&json!({
                "ok": true, "frame": code["frame"], "name": code["name"],
                "format": if react { "react" } else { "html" }, "files": files,
                "notes": code["notes"],
            }))),
            Ok(Err(e)) | Err(e) => fail(e),
        }
    }
}

/// Writes `text` at `path`, its folder made if need be.
fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Unable to create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("Unable to write {}: {e}", path.display()))
}

/// Refuses a screen op that names neither its `work` nor its `screen`.
fn names_one(op: &Value) -> Result<(), String> {
    let given = |key: &str| op[key].as_str().is_some_and(|v| !v.trim().is_empty());
    match given("work") || given("screen") {
        true => Ok(()),
        false => Err("name the screen: its `work` (from canvas_begin) or `screen`".into()),
    }
}

/// Where an export goes when the agent names no path: the project's `.nyawit/exports/`
/// (`pricing-page.html`, `PricingPage.tsx`), else Sawit's cache.
fn default_export(project: Option<&Path>, cache: &Path, name: &str, react: bool) -> PathBuf {
    let file = match react {
        true => format!("{}.tsx", screens::component_name(name)),
        false => format!("{name}.html"),
    };
    exports_path(project, cache, &file)
}

/// File `file` in the project's `.nyawit/exports/`, else in Sawit's cache.
fn exports_path(project: Option<&Path>, cache: &Path, file: &str) -> PathBuf {
    match project {
        Some(project) => {
            let dir = sawit_core::nyawit::exports_dir(project);
            let _ = sawit_core::nyawit::ensure(project, &dir);
            dir.join(file)
        }
        None => cache.join("canvas").join(file),
    }
}
