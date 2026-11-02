//! canvas_add_routes with Sawit closed: the routes it is given laid out in a section of the saved
//! canvas, and what it refuses before anything is made.

use rmcp::handler::server::wrapper::Parameters;

use super::*;
use crate::app::canvas::CanvasAddRoutesArgs;
use crate::app::tests::{answer, native, said};

fn add(project: &Path, url: &str, routes: &[&str]) -> CanvasAddRoutesArgs {
    CanvasAddRoutesArgs {
        url: Some(url.into()),
        routes: Some(routes.iter().map(|r| r.to_string()).collect()),
        viewports: None,
        file: None,
        project: project_arg(project),
    }
}

/// A section named for the server holds a frame per route and viewport, each with its page; the
/// answer says nothing is rendered, and the work it ran as is done.
#[tokio::test]
async fn with_sawit_closed_routes_are_saved_as_frames() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 1);
    let mut args = add(&project, "http://localhost:5173/x", &["/", "/pricing", "/"]);
    args.viewports = Some(vec![json!(1440), json!({"w": 390, "h": 844})]);
    let reply = answer(&server.canvas_add_routes(Parameters(args)).await);
    assert_eq!(reply["name"], "localhost:5173", "{reply}");
    assert_eq!(reply["server"], "http://localhost:5173");
    assert_eq!(reply["rendered"], false);
    let frames = reply["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 4, "a route named twice is added once");
    assert_eq!(frames[3]["url"], "http://localhost:5173/pricing");
    assert_eq!(frames[3]["viewport"], json!({"w": 390.0, "h": 844.0}));
    let layers = layers_in(&file);
    let section = (layers.iter())
        .find(|l| l["id"] == reply["section"])
        .expect("the section is saved");
    assert_eq!(section["type"], "section", "{section}");
    let board = store::open(&file).unwrap();
    let doc = board.doc().unwrap();
    let id = frames[2]["id"].as_str().unwrap().parse().unwrap();
    let frame = doc.node(id).unwrap();
    assert_eq!(frame.name, "/pricing / 1440");
    let data = sawit_board::routes::web_of(frame).unwrap();
    assert_eq!(data.route.as_deref(), Some("/pricing"));
}

/// A server elsewhere, a dynamic route and a route that isn't a path are refused, and nothing is
/// made.
#[tokio::test]
async fn remote_servers_and_dynamic_routes_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 1);
    let refused = |result: rmcp::model::CallToolResult| {
        assert_eq!(result.is_error, Some(true), "{}", said(&result));
        said(&result)
    };
    let remote = refused(
        server
            .canvas_add_routes(Parameters(add(&project, "https://example.com", &["/"])))
            .await,
    );
    assert!(remote.contains("example.com isn't on this Mac"), "{remote}");
    let dynamic = refused(
        server
            .canvas_add_routes(Parameters(add(
                &project,
                "http://localhost:3000",
                &["/blog/[slug]"],
            )))
            .await,
    );
    assert!(dynamic.contains("dynamic route"), "{dynamic}");
    let bare = refused(
        server
            .canvas_add_routes(Parameters(add(
                &project,
                "http://localhost:3000",
                &["pricing"],
            )))
            .await,
    );
    assert!(bare.contains("starting with /"), "{bare}");
    let mut wide = add(&project, "http://localhost:3000", &["/"]);
    wide.viewports = Some(vec![json!(9000)]);
    let wide = refused(server.canvas_add_routes(Parameters(wide)).await);
    assert!(wide.contains("200 to 3840"), "{wide}");
    assert_eq!(layers_in(&file).len(), 1, "only the sticky the canvas had");
}
