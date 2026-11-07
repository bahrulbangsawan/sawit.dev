//! Code mode: the whole Sawit API behind two tools. `search_api` finds functions by keyword and
//! shows their typed signatures; `run` runs a short JavaScript that calls them
//! (`sawit.git_status({repo})`) and returns only what the script returns, so an agent loads two
//! tool schemas instead of every one, and a chain of calls costs it one answer.
//!
//! Every function is a tool the server already has, called through the same router, so nothing
//! an agent can do with the tools is lost, and the signatures come from the same definitions.
//! Which tools `tools/list` shows is a setting ([`ToolsMode`]): all of them (the default, with
//! these two among them), or only these two.

pub mod api;
pub mod script;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock as Content, JsonObject,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, schemars, tool, tool_router};
use serde_json::Value;

use crate::{MAX_OUTPUT, Sawit, clip, fail};
pub use api::{Api, CODE_MODE_TOOLS};

/// The environment variable that picks the tools `tools/list` shows: `code` for code mode's two,
/// anything else (or unset) for all of them.
pub const TOOLS_ENV: &str = "SAWIT_MCP_TOOLS";

/// Which tools the server lists. Every tool can be called in either mode; code mode only keeps
/// the list (and so the agent's context) small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolsMode {
    /// Every tool, `search_api` and `run` among them.
    #[default]
    All,
    /// Only `search_api` and `run`.
    Code,
}

impl ToolsMode {
    /// The mode `SAWIT_MCP_TOOLS` asks for.
    pub fn from_env() -> Self {
        Self::parse(std::env::var(TOOLS_ENV).ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("code" | "code-mode" | "code_mode") => Self::Code,
            _ => Self::All,
        }
    }
}

/// What the server says when an agent connects in code mode (instead of [`crate::INSTRUCTIONS`],
/// which names tools this mode doesn't list).
pub const CODE_INSTRUCTIONS: &str = "\
Sawit is a macOS developer workbench. Its whole API is behind two tools:
- search_api finds functions by what you want to do (\"git commit\", \"terminal read\", \"canvas screen\") and shows their typed signatures; with no query, every function by area.
- run runs JavaScript that calls them: `const s = sawit.git_status({repo: \"/path\"}); return s;`. Calls are synchronous (await is fine), a failed call throws, and only what the script returns (and console.log) comes back, so chain several calls in one run.
Before you design, build or review UI on the Sawit canvas, read sawit.canvas_design_guide() and follow it. Build UI as editable layers (canvas_begin, then canvas_command with the file's components).
Databases, instances, ports, SSH, git, prompts, files, dev tools, MCP servers, skills and New Project work without the app; app_*, terminal_*, browser_*, canvas_* and keymap_* need it (launch_app starts it; app_state shows windows, tabs and terminals). terminal_run returns the tab and pane it used: pass both to terminal_read. Browser snapshot refs reset when the page changes. Tools that destroy something need confirm: true, which you pass only after the user says yes. Only the user signs in to GitHub and moves the keyboard.";

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchApiArgs {
    #[schemars(
        description = "Words for what you want to do (\"git commit\", \"read terminal output\", \"browser screenshot\"), or a function's exact name. Empty: every function's name, by area"
    )]
    pub query: Option<String>,
    #[schemars(
        description = "How many matches get their full typed signature (default 8, max 40); the next ones are listed on one line each"
    )]
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunArgs {
    #[schemars(
        description = "JavaScript: the body of an async function. Call Sawit as sawit.<function>({...args}) or sawit.call(\"name\", {...}); each returns the tool's answer (parsed JSON when it is JSON, else its text) and throws an Error when the tool fails. `return` what you want back; a script that is one expression returns its value. console.log lines come back too. No TypeScript annotations, imports, fetch, timers or file access: only sawit.*"
    )]
    pub code: String,
    #[schemars(
        description = "Seconds the whole script may take, Sawit's calls included (default 120, max 600)"
    )]
    pub timeout_secs: Option<f64>,
}

/// Images the script's calls returned (screenshots, exports), by number; only those its result
/// names are sent back.
type Images = Arc<Mutex<Vec<(String, String)>>>;

impl Sawit {
    /// The API code mode offers: every tool but its own two.
    pub fn api() -> &'static Api {
        static API: std::sync::OnceLock<Api> = std::sync::OnceLock::new();
        API.get_or_init(|| Api::new(&Sawit::router().list_all()))
    }

    fn search_api_tool(&self, a: SearchApiArgs) -> CallToolResult {
        let api = Self::api();
        let query = a.query.unwrap_or_default();
        if query.trim().is_empty() {
            return crate::ok(api.overview());
        }
        let limit = a.limit.unwrap_or(8).clamp(1, 40);
        let found = api.search(&query);
        if found.is_empty() {
            return crate::ok(format!(
                "No function matches “{}”. Try fewer or other words, or search_api with no query for every function.",
                query.trim()
            ));
        }
        let mut out = format!(
            "{} function{} match “{}”. Call them in run as sawit.<name>(args).\n\n",
            found.len(),
            if found.len() == 1 { "" } else { "s" },
            query.trim()
        );
        for f in found.iter().take(limit) {
            out.push_str(&f.signature());
            out.push_str("\n\n");
        }
        let rest: Vec<String> = found
            .iter()
            .skip(limit)
            .take(30)
            .map(|f| format!("- {}", f.summary()))
            .collect();
        if !rest.is_empty() {
            out.push_str("Also (search by name for the signature):\n");
            out.push_str(&rest.join("\n"));
            out.push('\n');
        }
        crate::ok(out.trim_end().to_string())
    }

    async fn run_tool(&self, a: RunArgs, context: RequestContext<RoleServer>) -> CallToolResult {
        if a.code.trim().is_empty() {
            return fail("The script is empty.");
        }
        let limits = script::Limits {
            time: Duration::from_secs_f64(a.timeout_secs.unwrap_or(120.0).clamp(1.0, 600.0)),
            ..Default::default()
        };
        let names: Vec<String> = Self::api()
            .functions
            .iter()
            .map(|f| f.name.clone())
            .collect();
        let images: Images = Default::default();
        let host = self.script_host(context, images.clone());
        let code = a.code;
        let ran =
            tokio::task::spawn_blocking(move || script::run(&code, &names, host, limits)).await;
        let outcome = match ran {
            Ok(outcome) => outcome,
            Err(e) => return fail(format!("The script stopped: {e}")),
        };
        reply(outcome, &images.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// What a script's `sawit.<name>(args)` does: the tool `name`, called through the router as
    /// a client's call would be, on this thread (the script's), waiting for its answer.
    fn script_host(&self, context: RequestContext<RoleServer>, images: Images) -> script::Host {
        let server = self.clone();
        let runtime = tokio::runtime::Handle::current();
        Box::new(move |name: &str, args: Value| {
            if CODE_MODE_TOOLS.contains(&name) {
                return Err(format!("{name} can't be called from a script."));
            }
            if Self::api().get(name).is_none() {
                return Err(format!(
                    "There is no function {name}. search_api finds the functions by keyword."
                ));
            }
            let args: JsonObject = match args {
                Value::Object(map) => map,
                Value::Null => JsonObject::new(),
                other => {
                    return Err(format!("{name} takes an object of arguments, not {other}."));
                }
            };
            let params = CallToolRequestParams::new(name.to_string()).with_arguments(args);
            let context = context.clone();
            let answer = runtime.block_on(async {
                let call = ToolCallContext::new(&server, params, context);
                server.tool_router.call(call).await
            });
            match answer {
                Ok(CallToolResponse::Complete(result)) => tool_value(name, result, &images),
                Ok(_) => Err(format!(
                    "{name} asked for input a script can't give; call it as a tool instead."
                )),
                Err(e) => Err(format!("{name}: {}", e.message)),
            }
        })
    }
}

/// A tool's answer as the script sees it: its structured content, or its text (parsed when it
/// is JSON), with any image as `{image: "#n", mime_type}`; a tool error as `Err`.
fn tool_value(name: &str, result: CallToolResult, images: &Images) -> Result<Value, String> {
    let mut texts = Vec::new();
    let mut pictures = Vec::new();
    for block in result.content {
        match block {
            Content::Text(t) => texts.push(t.text),
            Content::Image(i) => {
                let mut all = images.lock().unwrap_or_else(|e| e.into_inner());
                all.push((i.data, i.mime_type.clone()));
                pictures.push(serde_json::json!({
                    "image": format!("#{}", all.len()),
                    "mime_type": i.mime_type,
                }));
            }
            _ => {}
        }
    }
    let text = texts.join("\n");
    if result.is_error == Some(true) {
        return Err(if text.is_empty() {
            format!("{name} failed.")
        } else {
            text
        });
    }
    let value = match result.structured_content {
        Some(v) => v,
        None => parse_text(&text),
    };
    if pictures.is_empty() {
        return Ok(value);
    }
    Ok(serde_json::json!({ "text": value, "images": pictures }))
}

/// A tool's text as a value: the JSON object or array it is, else the text.
fn parse_text(text: &str) -> Value {
    let trimmed = text.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(v) = serde_json::from_str::<Value>(text)
    {
        return v;
    }
    Value::String(text.to_string())
}

/// The run's answer: what the script returned (a string as it is, anything else as JSON), its
/// console, and the images it returned; or why it stopped.
fn reply(outcome: script::Outcome, images: &[(String, String)]) -> CallToolResult {
    let mut text = String::new();
    let mut attached = Vec::new();
    if let Some(error) = &outcome.error {
        text.push_str(&format!(
            "The script failed after {} Sawit call{}: {error}",
            outcome.calls,
            if outcome.calls == 1 { "" } else { "s" }
        ));
    } else {
        match &outcome.value {
            None => text.push_str("(the script returned nothing)"),
            Some(Value::String(s)) => text.push_str(s),
            Some(v) => {
                collect_images(v, &mut attached);
                text.push_str(&serde_json::to_string_pretty(v).unwrap_or_default());
            }
        }
    }
    if !outcome.logs.is_empty() {
        text.push_str("\n\nConsole:\n");
        text.push_str(&outcome.logs.join("\n"));
    }
    let mut content = vec![Content::text(clip(text, MAX_OUTPUT))];
    for n in attached {
        if let Some((data, mime)) = n.checked_sub(1).and_then(|i| images.get(i)) {
            content.push(Content::image(data.clone(), mime.clone()));
        }
    }
    if outcome.error.is_some() {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

/// The numbers of the images (`{image: "#n"}`) a value names, each once.
fn collect_images(value: &Value, out: &mut Vec<usize>) {
    match value {
        Value::Object(map) => {
            if let Some(n) = map
                .get("image")
                .and_then(Value::as_str)
                .and_then(|s| s.strip_prefix('#'))
                .and_then(|n| n.parse().ok())
                && !out.contains(&n)
            {
                out.push(n);
            }
            map.values().for_each(|v| collect_images(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_images(v, out)),
        _ => {}
    }
}

#[tool_router(router = code_mode_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Find Sawit API functions for code mode: words for what you want to do (\"git commit\", \"terminal read\", \"canvas screen\") give the matching functions with their typed signatures; no query lists every function by area. Call them with run. Every function is one of Sawit's tools, with the same arguments"
    )]
    async fn search_api(&self, Parameters(a): Parameters<SearchApiArgs>) -> CallToolResult {
        self.search_api_tool(a)
    }

    #[tool(
        description = "Run a short JavaScript that calls Sawit's API (sawit.<function>({...}), found with search_api) and get back only what it returns, plus its console.log lines: chain several calls (run a command, read its output, take a screenshot) in one step. Calls are synchronous and throw on failure. Sandboxed: no files, network, imports or timers except through sawit.*; time and memory are limited. Images a call returns come back when the script returns them. Each call does what the tool of the same name does, confirmations included"
    )]
    async fn run(
        &self,
        Parameters(a): Parameters<RunArgs>,
        context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        self.run_tool(a, context).await
    }
}

#[cfg(test)]
mod tests;
