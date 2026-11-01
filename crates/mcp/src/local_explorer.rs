//! Cloudflare Local Explorer: the explorers that Workers dev servers (`wrangler dev`, `vite` with
//! the Cloudflare plugin, `cf dev`) serve at `/cdn-cgi/local/explorer`, the project's Workers with
//! their bindings and whether their dev tool is new enough, and one call to an explorer's API.
//! Only loopback is ever called, only under the API's path, and anything but GET needs
//! `confirm: true`, as it writes to the dev server's data (`sawit_core::cloudflare::explorer`).

use std::path::{Path, PathBuf};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::cloudflare::explorer::{self, Explorer, Method};
use sawit_core::cloudflare::{self, WorkersProject};
use sawit_core::probe;
use serde_json::Value;

use crate::{Sawit, blocking, fail, ok, project_dir, tilde, tri};

/// At most this much of an answer's body is shown; the rest is cut with a note.
const SHOWN: usize = 60_000;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LocalExplorerArgs {
    #[schemars(
        description = "Project folder (absolute or ~/…). Default: the folder sawit-mcp runs in"
    )]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExplorerRequestArgs {
    #[schemars(
        description = "The dev server's port (local_explorer lists them). Default: the one explorer running in `project`"
    )]
    pub port: Option<u16>,
    #[schemars(
        description = "Project folder whose dev server to call, when no port is given. Default: the folder sawit-mcp runs in"
    )]
    pub project: Option<String>,
    #[schemars(
        description = "GET (default), POST, PUT, PATCH or DELETE. Anything but GET can change local data and needs confirm=true"
    )]
    pub method: Option<String>,
    #[schemars(
        description = "Path under /cdn-cgi/local/explorer/api, with its query: e.g. local/workers, storage/kv/namespaces, storage/kv/namespaces/<id>/keys, d1/database, d1/database/<id>/raw, r2/buckets, workflows. Empty: the OpenAPI document (large). No URLs or .. segments"
    )]
    #[serde(default)]
    pub path: String,
    #[schemars(
        description = "Request body: JSON (sent as application/json), or a string sent as it is, e.g. {\"sql\": \"SELECT * FROM users LIMIT 10\"} for d1/database/<id>/raw"
    )]
    pub body: Option<Value>,
    #[schemars(
        description = "Content-Type of a string body. Default: application/json when it parses as JSON, else text/plain"
    )]
    pub content_type: Option<String>,
    #[schemars(
        description = "Must be true for POST, PUT, PATCH and DELETE: they write to the dev server's KV, R2, D1, Durable Objects or Workflows (a binding marked remote reaches your Cloudflare account), which can't be undone"
    )]
    #[serde(default)]
    pub confirm: bool,
}

/// One explorer as `local_explorer` lists it, with the Workers its dev server runs when its API
/// lists them.
fn explorer_lines(e: &Explorer, workers: &[explorer::LocalWorker]) -> Vec<String> {
    let place = e
        .cwd
        .as_deref()
        .map(|c| format!(" · {}", tilde(Path::new(c))))
        .unwrap_or_default();
    let mut out = vec![
        format!(
            "- {} (port {}, PID {} · {}{place})",
            e.url(),
            e.port,
            e.pid,
            crate::ports::clip_line(&e.label, 120)
        ),
        format!(
            "    API: {} ({}{}, {} operations)",
            e.api_url(),
            e.spec.title.as_deref().unwrap_or("OpenAPI"),
            e.spec
                .version
                .as_deref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default(),
            e.spec.operations.len()
        ),
    ];
    for w in workers {
        let bindings = w
            .bindings
            .iter()
            .map(|(kind, name)| format!("{kind} {name}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!(
            "    Worker: {}{}{}",
            w.name,
            if w.is_self {
                " (this dev server's)"
            } else {
                ""
            },
            if bindings.is_empty() {
                String::new()
            } else {
                format!(" · bindings: {bindings}")
            }
        ));
    }
    out
}

/// One Worker of the project as `local_explorer` lists it: its config, dev tool and bindings,
/// what it needs for Local Explorer, and the script that starts it.
pub(crate) fn worker_lines(w: &WorkersProject, root: &Path) -> Vec<String> {
    let rel = w
        .root
        .strip_prefix(root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map(|r| format!("{}/", r.display()))
        .unwrap_or_default();
    let tool = match &w.version {
        Some(v) => format!("{} {v}", w.tool.name()),
        None => w.tool.name().to_string(),
    };
    let mut out = vec![format!(
        "- {} · {rel}{} · {tool}",
        w.name.as_deref().unwrap_or("(unnamed Worker)"),
        w.config_name(),
    )];
    if !w.bindings.is_empty() {
        let bindings = w
            .bindings
            .iter()
            .map(|b| {
                let what = b.name.as_deref().or(b.id.as_deref());
                match what {
                    Some(what) => format!("{} {} ({what})", b.kind.label(), b.binding),
                    None => format!("{} {}", b.kind.label(), b.binding),
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!("    bindings: {bindings}"));
    }
    if let Some(note) = cloudflare::requirement_note(w, root) {
        out.push(format!("    {note}"));
    }
    match cloudflare::dev_scripts(w).first() {
        Some(s) => out.push(format!(
            "    starts with: `{}` (runs `{}`; Run panel: app_run_script name={})",
            sawit_core::scripts::run_command(&package_tool(w, root), &s.name),
            s.body,
            run_name(w, root, &s.name)
        )),
        None => out.push(match w.tool {
            cloudflare::DevTool::VitePlugin => "    starts with: `npx vite`".into(),
            cloudflare::DevTool::CfCli => "    starts with: `cf dev`".into(),
            cloudflare::DevTool::Wrangler => "    starts with: `npx wrangler dev`".into(),
        }),
    }
    out
}

/// The package manager that runs `w`'s scripts, as the Run panel has it.
fn package_tool(w: &WorkersProject, root: &Path) -> String {
    let manifest = std::fs::read_to_string(w.root.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    sawit_core::scripts::package_manager(&w.root, root, &manifest).to_string()
}

/// The Run panel's name for script `name` of `w`: `dev`, or `@m/api · dev` in a workspace package.
fn run_name(w: &WorkersProject, root: &Path, name: &str) -> String {
    if w.root == root {
        return name.to_string();
    }
    let label = std::fs::read_to_string(w.root.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|m| m.get("name")?.as_str().map(str::to_string))
        .unwrap_or_else(|| {
            w.root
                .strip_prefix(root)
                .map(|r| r.display().to_string())
                .unwrap_or_default()
        });
    format!("“{label} · {name}”")
}

/// What `local_explorer_request` shows of an answer: its status and type, then its body, JSON
/// laid out, cut at [`SHOWN`] bytes with a note; a binary body by its size alone.
pub(crate) fn answer_text(r: &explorer::Response) -> String {
    let kind = r.content_type.as_deref().unwrap_or("no content type");
    let mut out = format!("HTTP {} · {kind} · {} bytes", r.status, r.body.len());
    if r.truncated {
        out.push_str(&format!(
            " (cut at {} MB by Sawit)",
            explorer::BODY_LIMIT >> 20
        ));
    }
    if r.body.is_empty() {
        return out;
    }
    let Ok(text) = std::str::from_utf8(&r.body) else {
        out.push_str("\n(binary body, not shown)");
        return out;
    };
    let shown = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| text.to_string());
    out.push('\n');
    if shown.len() <= SHOWN {
        out.push_str(&shown);
    } else {
        let mut end = SHOWN;
        while !shown.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&shown[..end]);
        out.push_str(&format!(
            "\n… (answer cut: {end} of {} bytes shown; ask for less, e.g. with ?per_page= or a LIMIT)",
            shown.len()
        ));
    }
    out
}

/// The body of a call: a JSON value as JSON, a string as it is.
fn body_of(body: Option<&Value>, content_type: Option<&str>) -> Option<(Vec<u8>, String)> {
    let body = body.filter(|b| !b.is_null())?;
    Some(match body {
        Value::String(text) => {
            let kind =
                content_type.map(str::to_string).unwrap_or_else(|| {
                    match serde_json::from_str::<Value>(text) {
                        Ok(_) => "application/json".into(),
                        Err(_) => "text/plain; charset=utf-8".into(),
                    }
                });
            (text.clone().into_bytes(), kind)
        }
        other => (
            other.to_string().into_bytes(),
            content_type.unwrap_or("application/json").to_string(),
        ),
    })
}

impl Sawit {
    async fn local_explorer_tool(&self, project: Option<String>) -> CallToolResult {
        let folder = project_dir(project.as_deref());
        if let Some(f) = &folder
            && project.is_some()
            && !f.is_dir()
        {
            return fail(format!("{} is not a folder.", f.display()));
        }
        let workers = match folder.clone() {
            Some(f) => tri!(blocking(move || cloudflare::detect_project(&f)).await),
            None => Vec::new(),
        };
        let jobs = tri!(probe::listening_jobs().await.map_err(|e| e.to_string()));
        let explorers = explorer::find(&jobs).await;
        let (mine, others): (Vec<&Explorer>, Vec<&Explorer>) = explorers
            .iter()
            .partition(|e| folder.as_deref().is_some_and(|f| e.runs_in(f)));

        let mut out = Vec::new();
        let here = folder
            .as_deref()
            .map(tilde)
            .unwrap_or_else(|| "this Mac".into());
        if mine.is_empty() {
            out.push(format!("No Local Explorer is running for {here}."));
        } else {
            out.push(format!("Local Explorer running for {here}:"));
            for e in &mine {
                let workers = match e.spec.has("GET /local/workers") {
                    true => explorer::local_workers(e.addr).await,
                    false => Vec::new(),
                };
                out.extend(explorer_lines(e, &workers));
            }
        }
        if let Some(root) = folder.as_deref() {
            if workers.is_empty() {
                out.push(format!(
                    "{here} has no Cloudflare Worker: no wrangler.toml, wrangler.json(c), cloudflare.config.ts or {} in package.json.",
                    cloudflare::VITE_PLUGIN
                ));
            } else {
                out.push(String::new());
                out.push("Workers in this project:".into());
                for w in &workers {
                    out.extend(worker_lines(w, root));
                }
            }
        }
        if !others.is_empty() {
            out.push(String::new());
            out.push("Local Explorer running elsewhere on this Mac:".into());
            for e in &others {
                out.extend(explorer_lines(e, &[]));
            }
        }
        out.push(String::new());
        out.push(if explorers.is_empty() {
            "Start a Workers dev server (wrangler dev, vite with @cloudflare/vite-plugin, or cf dev; Wrangler 4.118.0+ or the Vite plugin 1.50.0+) and Local Explorer is at http://localhost:<port>/cdn-cgi/local/explorer.".to_string()
        } else {
            "Open one in Sawit's browser with browser_navigate (its URL), or read and change its data with local_explorer_request (GET reads; POST, PUT, PATCH and DELETE need confirm=true).".to_string()
        });
        ok(out.join("\n"))
    }

    async fn local_explorer_request_tool(&self, a: ExplorerRequestArgs) -> CallToolResult {
        // Checked before anything is looked up, let alone sent.
        let method = tri!(Method::parse(a.method.as_deref().unwrap_or("GET")));
        tri!(explorer::check_confirmed(method, a.confirm));
        let path = tri!(explorer::api_path(&a.path));
        let body = body_of(a.body.as_ref(), a.content_type.as_deref());
        if body.is_some() && method == Method::Get {
            return fail("A GET has no body. Use POST (with confirm=true) to send one.");
        }

        let jobs = tri!(probe::listening_jobs().await.map_err(|e| e.to_string()));
        let explorers = explorer::find(&jobs).await;
        let target = tri!(pick(&explorers, a.port, a.project.as_deref()));
        let answer = tri!(
            explorer::request(
                target.addr,
                method,
                &path,
                body.as_ref().map(|(b, k)| (b.as_slice(), k.as_str()))
            )
            .await
        );
        let text = format!(
            "{} http://localhost:{}{path}\n{}",
            method.as_str(),
            target.port,
            answer_text(&answer)
        );
        if answer.status >= 400 {
            fail(text)
        } else {
            ok(text)
        }
    }
}

/// The explorer a call goes to: the one on `port`, else the one running in `project` (or the
/// folder sawit-mcp runs in), else the only one on this Mac. A port that serves none is refused,
/// so a call only ever reaches a dev server's Local Explorer.
fn pick<'a>(
    explorers: &'a [Explorer],
    port: Option<u16>,
    project: Option<&str>,
) -> Result<&'a Explorer, String> {
    let ports = || {
        explorers
            .iter()
            .map(|e| format!(":{}", e.port))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Some(port) = port {
        return explorers.iter().find(|e| e.port == port).ok_or_else(|| {
            format!(
                "Nothing on port {port} serves Local Explorer{}. See local_explorer.",
                match explorers.is_empty() {
                    true => String::new(),
                    false => format!(" (running: {})", ports()),
                }
            )
        });
    }
    let folder: Option<PathBuf> = project_dir(project);
    let mine: Vec<&Explorer> = match &folder {
        Some(f) => explorer::running_in(explorers, f),
        None => Vec::new(),
    };
    match (mine.as_slice(), explorers) {
        ([one], _) => Ok(one),
        ([], [one]) if project.is_none() => Ok(one),
        ([], []) => Err(
            "No Local Explorer is running. Start the project's Workers dev server (wrangler dev, vite or cf dev) first; local_explorer says how.".into(),
        ),
        ([], _) => Err(format!(
            "No Local Explorer runs in {}; pass the port of one (running: {}).",
            folder.as_deref().map(tilde).unwrap_or_default(),
            ports()
        )),
        (several, _) => Err(format!(
            "Several dev servers in this project serve Local Explorer ({}): pass the port.",
            several
                .iter()
                .map(|e| format!(":{}", e.port))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[tool_router(router = local_explorer_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Cloudflare Local Explorer: the explorers running Workers dev servers (wrangler dev, vite with the Cloudflare plugin, cf dev) serve at http://localhost:<port>/cdn-cgi/local/explorer, with their API URL, port, process and Workers; and the project's Workers (wrangler.toml/json/jsonc, cloudflare.config.ts, @cloudflare/vite-plugin) with their bindings (KV, R2, D1, Durable Objects, Workflows, send_email), whether the installed Wrangler (4.118.0+) or Vite plugin (1.50.0+) has Local Explorer, the upgrade command when not, and the script that starts the dev server when none runs. Only asks dev servers on loopback"
    )]
    async fn local_explorer(&self, Parameters(a): Parameters<LocalExplorerArgs>) -> CallToolResult {
        self.local_explorer_tool(a.project).await
    }

    #[tool(
        description = "Call a running Cloudflare Local Explorer's API (http://localhost:<port>/cdn-cgi/local/explorer/api/<path>) on a Workers dev server on this Mac: list KV namespaces and keys, read values, run D1 SQL, list R2 objects, Durable Objects and Workflow instances, query observability (local/observability/query). GET reads. POST, PUT, PATCH and DELETE can change the dev server's local data (and, through a binding marked remote, your Cloudflare account's), so they need confirm=true. Loopback only; the path stays under the API (no URLs or ..). Long answers are cut"
    )]
    async fn local_explorer_request(
        &self,
        Parameters(a): Parameters<ExplorerRequestArgs>,
    ) -> CallToolResult {
        self.local_explorer_request_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sawit() -> Sawit {
        Sawit::new(sawit_core::instances::AppPaths {
            root: std::env::temp_dir(),
        })
    }

    fn said(r: &CallToolResult) -> String {
        r.content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|t| t.text.clone())
            .collect()
    }

    /// A write is refused before any dev server is looked for, let alone called; so is a path
    /// that leaves the API, and a URL.
    #[tokio::test]
    async fn writes_and_paths_out_of_the_api_are_refused_first() {
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            let a: ExplorerRequestArgs = serde_json::from_value(serde_json::json!({
                "port": 1,
                "method": method,
                "path": "storage/kv/namespaces/x/values/k",
            }))
            .unwrap();
            let r = sawit().local_explorer_request_tool(a).await;
            assert_eq!(r.is_error, Some(true));
            assert!(said(&r).contains("confirm: true"), "{}", said(&r));
        }
        for path in [
            "../../etc/passwd",
            "%2e%2e/x",
            "http://example.com/x",
            "//evil/x",
        ] {
            let a: ExplorerRequestArgs =
                serde_json::from_value(serde_json::json!({ "port": 1, "path": path })).unwrap();
            let r = sawit().local_explorer_request_tool(a).await;
            assert_eq!(r.is_error, Some(true));
            assert!(said(&r).starts_with("Refused path"), "{}", said(&r));
        }
    }

    /// A call goes to an explorer the probe found: a port that serves none is refused, and with no
    /// port the project's one is picked, or the only one on this Mac.
    #[test]
    fn calls_go_to_an_explorer_found() {
        let spec = explorer::parse_spec(br#"{"openapi":"3.0.3","paths":{}}"#).unwrap();
        let at = |port: u16, cwd: &str| Explorer {
            port,
            addr: format!("127.0.0.1:{port}").parse().unwrap(),
            pid: u32::from(port),
            label: "wrangler dev".into(),
            cwd: Some(cwd.into()),
            spec: spec.clone(),
        };
        let both = [at(8787, "/w/shop/api"), at(5173, "/w/blog")];
        assert_eq!(pick(&both, Some(5173), None).unwrap().port, 5173);
        let refused = pick(&both, Some(3000), None).unwrap_err();
        assert!(refused.contains("Nothing on port 3000"), "{refused}");
        assert_eq!(pick(&both, None, Some("/w/shop")).unwrap().port, 8787);
        assert!(pick(&both, None, Some("/w/other")).is_err());
        assert!(
            pick(&[], None, Some("/w/shop"))
                .unwrap_err()
                .contains("No Local Explorer is running")
        );
        let two = [at(8787, "/w/shop/api"), at(8788, "/w/shop/mail")];
        assert!(
            pick(&two, None, Some("/w/shop"))
                .unwrap_err()
                .contains("pass the port")
        );
    }

    /// Long answers are cut with a note, JSON is laid out, binary bodies aren't dumped.
    #[test]
    fn answers_are_shown_cut_and_laid_out() {
        let r = |body: Vec<u8>, kind: &str| explorer::Response {
            status: 200,
            content_type: Some(kind.into()),
            body,
            truncated: false,
        };
        let json = answer_text(&r(br#"{"result":[1]}"#.to_vec(), "application/json"));
        assert!(
            json.starts_with("HTTP 200 · application/json · 14 bytes\n{\n"),
            "{json}"
        );
        let big = answer_text(&r(vec![b'x'; SHOWN * 2], "text/plain"));
        assert!(big.contains("answer cut"), "{}", &big[big.len() - 200..]);
        assert!(big.len() < SHOWN + 500);
        let binary = answer_text(&r(vec![0xff, 0xfe, 0x00], "application/octet-stream"));
        assert!(binary.ends_with("(binary body, not shown)"));
        assert_eq!(
            body_of(Some(&serde_json::json!({"sql": "select 1"})), None),
            Some((br#"{"sql":"select 1"}"#.to_vec(), "application/json".into()))
        );
        assert_eq!(
            body_of(Some(&Value::String("hello".into())), None)
                .unwrap()
                .1,
            "text/plain; charset=utf-8"
        );
    }
}
