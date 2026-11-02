//! The document's side of route frames ([`crate::routes`]): the `routes` op lays a dev server's
//! routes out as frames in a section named for the server ([`routes`]), and `begin` with `kind:
//! "routes"` makes that section first, as a work's, so the canvas shows the agent at work on it
//! while the routes are found ([`new_section`]). Finding them is the caller's: the app and
//! `sawit-mcp` know the project and its dev server, and the document doesn't.

use figcraft_doc::{Document, NodeId, NodeKind};
use serde_json::{Value, json};

use super::failure;
use super::work::{on_page, work_frame, work_of};
use crate::doc::BoardDoc;
use crate::routes::{
    DEFAULT_WIDTHS, MOST_ROUTES, MOST_VIEWPORTS, WEB_KEY, WIDTHS, WebData, frame_name, layout,
    localhost, refused, server_name, viewport_for,
};
use crate::screens::Viewport;

/// A routes section's size while its routes are being found.
pub const SECTION_SIZE: (f64, f64) = (1440.0, 900.0);
/// The steps a routes work shows when `begin` names none.
pub const STEPS: [&str; 3] = ["Finding routes", "Adding frames", "Rendering"];
/// How far right of what the page holds a new section goes.
const BESIDE: f64 = 200.0;

/// Where a new section goes on page `page` of `doc` when nothing says: right of what the page
/// holds, level with its top, or at the page's origin on an empty page.
pub(super) fn beside_content(doc: &Document, page: NodeId) -> (f64, f64) {
    doc.page_content_bounds(page)
        .map_or((0.0, 0.0), |held| (held.x1 + BESIDE, held.y0))
}

/// The `node.create` params of a new section named `name` at `[x, y, w, h]` on `page`, with
/// plugin data `data`.
pub(super) fn new_section(page: NodeId, name: &str, [x, y, w, h]: [f64; 4], data: Value) -> Value {
    json!({
        "type": "section", "x": x, "y": y, "width": w, "height": h, "name": name,
        "parent": page, "select": false, "props": {"pluginData": data},
    })
}

/// What a `routes` op asks for, read and checked.
struct Ask {
    server: String,
    name: String,
    /// Each route's path and page.
    routes: Vec<(String, String)>,
    viewports: Vec<Viewport>,
    at: Option<(f64, f64)>,
}

/// The `routes` op (`{server, routes, viewports?, name?, x?, y?, work?}`): a frame for each route
/// (`"/changelog"`, or `{path, url}`) at each viewport (widths, or `{w, h}`; 1440, 768 and 390
/// when none are given), in rows in a section named `name` (the server's host and port when none
/// is given), in one undo step. With `work` (a `begin {kind: "routes"}`'s), the frames go into
/// that work's section, which grows to hold them; else a new section goes at `x`, `y`, or right of
/// what the page on show holds. Pages that aren't on this Mac are refused. It answers `{ok,
/// section, page, name, server, frames: [{id, route, url, viewport}]}`.
pub(super) fn routes(board: &mut BoardDoc, op: &Value) -> Value {
    let laid = read(op).and_then(|ask| lay_out(board, op, &ask));
    laid.unwrap_or_else(|error| failure(format!("routes: {error}")))
}

fn read(op: &Value) -> Result<Ask, String> {
    let server = (op.get("server").and_then(Value::as_str).map(str::trim))
        .filter(|s| !s.is_empty())
        .ok_or("it needs the dev server's `server`, like \"http://localhost:5173\"")?
        .trim_end_matches('/')
        .to_string();
    if !localhost(&server) {
        return Err(refused(&server));
    }
    let listed = op.get("routes").and_then(Value::as_array).ok_or(
        "it needs the `routes` to add, like [\"/\", \"/changelog\"] or [{\"path\": \"/\"}]",
    )?;
    let mut routes: Vec<(String, String)> = Vec::new();
    for item in listed {
        let path = item
            .as_str()
            .or_else(|| item.get("path").and_then(Value::as_str));
        let path = path
            .map(str::trim)
            .filter(|p| p.starts_with('/'))
            .ok_or_else(|| {
                format!("a route is a path starting with /, like \"/changelog\": not {item}")
            })?;
        if path.contains(['[', '$']) || path.split('/').any(|s| s.starts_with(':')) {
            return Err(format!(
                "“{path}” is a dynamic route: give one of its pages, like /blog/hello"
            ));
        }
        let url = match item.get("url").and_then(Value::as_str) {
            Some(url) if localhost(url) => url.to_string(),
            Some(url) => return Err(refused(url)),
            None => sawit_core::browser::routes::url_of(&server, path),
        };
        if !routes.iter().any(|(p, _)| p == path) {
            routes.push((path.to_string(), url));
        }
    }
    if routes.is_empty() {
        return Err("`routes` is empty: name at least one, like [\"/\"]".into());
    }
    if routes.len() > MOST_ROUTES {
        return Err(format!(
            "{} routes is more than one section holds ({MOST_ROUTES}): add them in parts",
            routes.len()
        ));
    }
    let name = (op.get("name").and_then(Value::as_str).map(str::trim))
        .filter(|n| !n.is_empty())
        .map_or_else(|| server_name(&server), str::to_string);
    let number = |key: &str| {
        op.get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
    };
    Ok(Ask {
        name,
        routes,
        viewports: viewports(op.get("viewports"))?,
        at: number("x").zip(number("y")),
        server,
    })
}

/// The viewports `value` names: widths (`[1440, 390]`) or sizes (`[{"w": 1280, "h": 720}]`), none
/// twice; [`DEFAULT_WIDTHS`] when it names none.
pub fn viewports(value: Option<&Value>) -> Result<Vec<Viewport>, String> {
    let listed = match value {
        None | Some(Value::Null) => return Ok(DEFAULT_WIDTHS.map(viewport_for).to_vec()),
        Some(Value::Array(listed)) if !listed.is_empty() => listed,
        Some(_) => {
            return Err("`viewports` must be a list of widths, like [1440, 768, 390]".into());
        }
    };
    let mut found: Vec<Viewport> = Vec::new();
    for item in listed {
        let (w, h) = match item {
            Value::Number(w) => (w.as_f64(), None),
            Value::Object(o) => (
                o.get("w")
                    .or_else(|| o.get("width"))
                    .and_then(Value::as_f64),
                o.get("h")
                    .or_else(|| o.get("height"))
                    .and_then(Value::as_f64),
            ),
            _ => (None, None),
        };
        let size = match (w.filter(|w| WIDTHS.contains(w)), h) {
            (Some(w), None) => Some(viewport_for(w)),
            (Some(w), Some(h)) if (100.0..=10_000.0).contains(&h) => Some(Viewport {
                w: w.round(),
                h: h.round(),
            }),
            _ => None,
        };
        let size = size.ok_or_else(|| {
            format!("a viewport is a width from 200 to 3840, or {{\"w\", \"h\"}}: not {item}")
        })?;
        if !found.contains(&size) {
            found.push(size);
        }
    }
    if found.len() > MOST_VIEWPORTS {
        return Err(format!("at most {MOST_VIEWPORTS} viewports at once"));
    }
    Ok(found)
}

fn lay_out(board: &mut BoardDoc, op: &Value, ask: &Ask) -> Result<Value, String> {
    let doc = board.doc().map_err(|e| e.to_string())?;
    let shown = board.session.page().map_err(|e| e.to_string())?;
    let work = work_of(op)?;
    let section = match work {
        Some(work) => {
            let id = work_frame(&doc, work).ok_or_else(|| {
                format!("work “{work}” isn't on the canvas: its section was deleted")
            })?;
            let node = doc.node(id).ok_or("its section is gone")?;
            if !matches!(node.kind, NodeKind::Section { .. }) {
                return Err(format!("work “{work}” isn't a routes work"));
            }
            Some((id, doc.page_of(id).ok_or("its section is on no page")?))
        }
        None => None,
    };
    let page = section.map_or(shown, |(_, page)| page);
    let (placed, (w, h)) = layout(ask.routes.len(), &ask.viewports);
    let (x, y) = ask.at.unwrap_or_else(|| beside_content(&doc, page));
    let made = on_page(board, page, |board| {
        crate::doc::grouped(&mut board.session, "Add routes", |session| {
            let section = match section {
                Some((id, _)) => {
                    let size =
                        json!({"id": id, "props": {"width": w, "height": h, "name": ask.name}});
                    session.execute("node.set", &size)?;
                    id
                }
                None => {
                    let params = new_section(page, &ask.name, [x, y, w, h], json!({}));
                    let made = session.execute("node.create", &params)?;
                    made.get("id")
                        .and_then(figcraft_engine::params::id_of)
                        .ok_or_else(|| {
                            figcraft_engine::EngineError::Other("the section wasn't made".into())
                        })?
                }
            };
            let mut frames = Vec::new();
            for frame in &placed {
                let (path, url) = &ask.routes[frame.route];
                let data = WebData {
                    url: url.clone(),
                    viewport: frame.viewport,
                    route: Some(path.clone()),
                };
                let params = json!({
                    "x": frame.x, "y": frame.y, "width": frame.viewport.w,
                    "height": frame.viewport.h, "name": frame_name(path, frame.viewport),
                    "parent": section, "select": false,
                    "props": {"pluginData": {(WEB_KEY): data.to_json()}},
                });
                let made = session.execute("frame.create", &params)?;
                let id = made.get("id").and_then(figcraft_engine::params::id_of);
                frames.push(json!({
                    "id": id.map(|id| id.to_string()), "route": path, "url": url,
                    "viewport": frame.viewport,
                }));
            }
            Ok((section, frames))
        })
    });
    let (section, frames) = made.map_err(|e| e.to_string())?;
    let mut reply = json!({
        "ok": true, "section": section.to_string(), "page": page.to_string(), "name": ask.name,
        "server": ask.server, "frames": frames,
    });
    if let Some(work) = work {
        reply["work"] = json!(work);
    }
    Ok(reply)
}
