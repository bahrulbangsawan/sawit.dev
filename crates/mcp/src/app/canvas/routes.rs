//! `canvas_add_routes`: a dev server's routes laid out as live frames, a row per route and a frame
//! per viewport, in a section named for the server ([`sawit_board::routes`]).
//!
//! It runs as an agent's work, so the canvas shows it while it runs: `begin {kind: "routes"}`
//! makes the section with its steps, the routes are found here (from the app's files, and the
//! server's links for samples of dynamic routes: `sawit_core::browser::routes`), then the `routes`
//! op lays the frames out in the section. With the app running, the canvas pictures each frame and
//! marks the work done once all are; with it closed, the frames are saved and nothing renders.

use std::path::{Path, PathBuf};

use rmcp::model::CallToolResult;
use sawit_board::ops::{ROUTE_STEPS, route_viewports};
use sawit_board::routes::{MOST_ROUTES, localhost, refused, server_name};
use sawit_core::browser::routes::{CRAWL, Found, Route, discover};
use sawit_core::browser::{DevServer, dev_server_list};
use serde_json::{Value, json};

use super::CanvasAddRoutesArgs;
use crate::app::pretty;
use crate::{Sawit, fail, ok};

/// What the answer says with the app closed.
const UNRENDERED: &str = "Sawit isn't running, so nothing is rendered: the section and its frames \
     are saved, and the canvas renders each page when the app opens it";

/// The dev server to add the routes of, and the folder its app is in.
struct Server {
    url: String,
    folder: PathBuf,
}

impl Sawit {
    pub async fn canvas_add_routes_tool(&self, a: CanvasAddRoutesArgs) -> CallToolResult {
        match self.add_routes(a).await {
            Ok(reply) => ok(pretty(&reply)),
            Err(e) => fail(format!("canvas_add_routes: {e}")),
        }
    }

    async fn add_routes(&self, a: CanvasAddRoutesArgs) -> Result<Value, String> {
        let viewports = route_viewports(a.viewports.map(Value::from).as_ref())?;
        let paths = a.routes.as_deref().map(given_routes).transpose()?;
        let server = find_server(a.url.as_deref(), a.project.as_deref()).await?;
        let (project, file) = (a.project.clone(), a.file.clone());
        let name = server_name(&server.url);
        let begin = json!({"op": "begin", "name": name, "kind": "routes", "steps": ROUTE_STEPS});
        let begun = self.canvas(project.clone(), file.clone(), begin).await?;
        let work = begun["work"].as_str().unwrap_or_default().to_string();
        let section = begun["frame"].clone();
        let progress = |step: Option<&str>, status: Option<&str>, message: Option<String>| json!({"op": "progress", "work": work, "step": step, "status": status, "message": message});
        let shown = self
            .canvas(
                project.clone(),
                file.clone(),
                progress(Some(ROUTE_STEPS[0]), None, None),
            )
            .await?
            .get("shown")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let found = match paths {
            Some(paths) => Found {
                routes: paths,
                ..Found::default()
            },
            None => discover(&server.folder, &server.url, &CRAWL).await,
        };
        if found.routes.is_empty() {
            let why = match &found.crawl_error {
                Some(error) => format!("no routes were found: {error}"),
                None => {
                    "no routes were found in the app's files or its links: name them in `routes`"
                        .into()
                }
            };
            let _ = self
                .canvas(
                    project.clone(),
                    file.clone(),
                    progress(None, Some("failed"), Some(why.clone())),
                )
                .await;
            let delete = json!({"op": "delete", "ids": [section]});
            let _ = self.canvas(project, file, delete).await;
            return Err(why);
        }
        let left_out = found.routes.len().saturating_sub(MOST_ROUTES);
        let routes: Vec<&str> = (found.routes.iter())
            .take(MOST_ROUTES)
            .map(|r| r.path.as_str())
            .collect();
        self.canvas(
            project.clone(),
            file.clone(),
            progress(Some(ROUTE_STEPS[1]), None, None),
        )
        .await?;
        let op = json!({
            "op": "routes", "work": work, "server": server.url, "name": name, "routes": routes,
            "viewports": viewports,
        });
        let laid = match self.canvas(project.clone(), file.clone(), op).await {
            Ok(laid) => laid,
            Err(error) => {
                // Nothing was laid out: the empty section goes.
                let delete = json!({"op": "delete", "ids": [section]});
                let _ = self.canvas(project, file, delete).await;
                return Err(error);
            }
        };
        if !shown {
            let _ = self
                .canvas(project, file, progress(None, Some("done"), None))
                .await;
        }
        Ok(answer(laid, &server, &found, left_out, shown))
    }
}

/// The routes an agent names, checked: paths of pages, none twice.
fn given_routes(paths: &[String]) -> Result<Vec<Route>, String> {
    let mut routes: Vec<Route> = Vec::new();
    for path in paths.iter().map(|p| p.trim()) {
        if path.contains(['[', '$']) || path.split('/').any(|s| s.starts_with(':')) {
            return Err(format!(
                "“{path}” is a dynamic route: give one of its pages, like /blog/hello"
            ));
        }
        if !path.starts_with('/') {
            return Err(format!(
                "a route is a path starting with /, like \"/pricing\": not “{path}”"
            ));
        }
        if !routes.iter().any(|r| r.path == path) {
            routes.push(Route {
                path: path.to_string(),
                file: None,
                sample_of: None,
            });
        }
    }
    match routes.is_empty() {
        true => Err("`routes` is empty: name at least one, like [\"/\"]".into()),
        false => Ok(routes),
    }
}

/// The dev server at `url`, or the one running in the project; its app's folder is the folder
/// the server runs in, when Sawit finds it running.
async fn find_server(url: Option<&str>, project: Option<&str>) -> Result<Server, String> {
    let folder = crate::agent_project(project)
        .map(|f| sawit_core::files::resolve(&f))
        .ok_or("no project: name its folder in `project`, or give the dev server's `url`")?;
    let jobs = sawit_core::probe::listening_jobs()
        .await
        .unwrap_or_default();
    let servers = dev_server_list(&jobs, Path::new("/"));
    if let Some(url) = url.map(str::trim).filter(|u| !u.is_empty()) {
        if !localhost(url) {
            return Err(refused(url));
        }
        let origin = origin_of(url);
        let port = |u: &str| sawit_core::browser::routes::host_port(u).map(|(_, p)| p);
        let running = servers.iter().find(|s| port(&s.url) == port(&origin));
        return Ok(Server {
            folder: running.map_or(folder, |s| s.folder.clone()),
            url: origin,
        });
    }
    let mine: Vec<&DevServer> = servers
        .iter()
        .filter(|s| s.folder.starts_with(&folder))
        .collect();
    match mine.as_slice() {
        [] => Err(format!(
            "no dev server is running in {}: start one (npm run dev, say), or give its `url`",
            folder.display()
        )),
        [one] => Ok(Server {
            url: one.url.clone(),
            folder: one.folder.clone(),
        }),
        several => {
            let named: Vec<String> = (several.iter())
                .map(|s| format!("{} ({} in {})", s.url, s.label, s.folder.display()))
                .collect();
            Err(format!(
                "{} dev servers run in {}: {}. Give the `url` of one",
                several.len(),
                folder.display(),
                named.join(", ")
            ))
        }
    }
}

/// `url` without its path: `http://localhost:5173/x` is `http://localhost:5173`.
fn origin_of(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    format!("{scheme}://{host}")
}

/// What `canvas_add_routes` answers: the section and its frames, where the routes came from, the
/// dynamic routes skipped, and whether the canvas renders them now.
fn answer(mut laid: Value, server: &Server, found: &Found, left_out: usize, shown: bool) -> Value {
    if let Some(reply) = laid.as_object_mut() {
        reply.remove("ok");
    }
    laid["folder"] = json!(server.folder);
    laid["framework"] = json!(found.framework.map(|f| f.name()));
    let sampled: Vec<Value> = (found.routes.iter())
        .filter_map(|r| Some(json!({"route": r.path, "sample_of": r.sample_of.as_ref()?})))
        .collect();
    if !sampled.is_empty() {
        laid["samples"] = json!(sampled);
    }
    if !found.skipped.is_empty() {
        let skipped: Vec<Value> = (found.skipped.iter())
            .map(|s| json!({"route": s.route, "file": s.file}))
            .collect();
        laid["skipped"] = json!(skipped);
        laid["skipped_why"] = json!(
            "dynamic routes with no page of theirs linked from the site: add one of their pages by \
             its path in `routes`"
        );
    }
    if let Some(crawl) = &found.crawl {
        laid["crawl"] = json!({"pages_fetched": crawl.fetched, "capped": crawl.capped});
    }
    if left_out > 0 {
        laid["left_out"] = json!(format!(
            "{left_out} more routes were found than one section holds ({MOST_ROUTES}): add them \
             by path in another call"
        ));
    }
    laid["rendered"] = json!(shown);
    laid["note"] = json!(match shown {
        true =>
            "The canvas renders each frame from its live page and marks the work done once all \
                 are pictured; frames refresh when the page changes, and say so when the server \
                 stops",
        false => UNRENDERED,
    });
    laid
}
