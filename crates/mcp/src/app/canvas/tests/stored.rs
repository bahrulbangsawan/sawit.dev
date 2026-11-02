//! The canvas tools with Sawit closed: they read and edit the stored canvas, the project's own.

use rmcp::handler::server::wrapper::Parameters;
use sawit_core::instances::AppPaths;

use super::*;
use crate::Sawit;
use crate::app::canvas::dispatch::summarize_element;
use crate::app::canvas::{
    CanvasCommandArgs, CanvasCommandsArgs, CanvasDeleteArgs, CanvasElementsArgs, CanvasMermaidArgs,
    CanvasTargetArgs, CanvasUpdateArgs,
};
use crate::app::tests::{answer, native, said};

#[test]
fn summarizes_saved_elements() {
    let e = json!({
        "id": "a1", "type": "arrow", "x": 1, "y": 2, "width": 3, "height": 4,
        "startBinding": {"elementId": "box"}, "endBinding": null, "seed": 99
    });
    assert_eq!(
        summarize_element(&e),
        json!({"id": "a1", "type": "arrow", "x": 1, "y": 2, "width": 3, "height": 4, "startBinding": "box"})
    );
}

/// Regression: with the app closed, a project that only holds a canvas file named with the
/// canvas engine's own extension has a blank canvas. That file and its backup aren't read,
/// moved or written: the first edit makes `canvas.sawit` beside them. Named with `file`, it
/// is refused, and nothing changes.
#[tokio::test]
async fn a_file_by_the_engines_own_name_is_no_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let nyawit = project.join(".nyawit");
    let old = nyawit.join("canvas.figcraft");
    write_board(&old, 2);
    write_board(&nyawit.join("canvas.figcraft.bak"), 5);
    let held = std::fs::read(&old).unwrap();

    let got = server
        .canvas_get_tool(CanvasTargetArgs {
            file: None,
            project: project_arg(&project),
        })
        .await;
    assert_eq!(answer(&got)["elements"], json!([]));
    let added = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![json!({"type": "sticky", "x": 0, "y": 0, "text": "new"})],
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_eq!(answer(&added)["added"], json!(1), "{}", said(&added));
    assert_eq!(layers_in(&board_file(&project)).len(), 1);

    let refused = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![json!({"type": "sticky", "x": 0, "y": 300, "text": "no"})],
            file: Some(old.display().to_string()),
            project: None,
        }))
        .await;
    assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
    assert!(
        said(&refused).contains("name a .sawit or .excalidraw file"),
        "{}",
        said(&refused)
    );
    let canvases: Vec<String> = (names(&nyawit).into_iter())
        .filter(|name| name.starts_with("canvas"))
        .collect();
    assert_eq!(
        canvases,
        ["canvas.figcraft", "canvas.figcraft.bak", "canvas.sawit"]
    );
    assert_eq!(std::fs::read(&old).unwrap(), held);
}

#[tokio::test]
async fn the_other_canvas_tools_edit_the_stored_canvas_while_the_app_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);

    // Two boxes and an arrow between them, with the agent's own ids.
    let added = server
            .canvas_add_elements(Parameters(CanvasElementsArgs {
                elements: vec![
                    json!({"type": "rectangle", "id": "api", "x": 0, "y": 0, "width": 160, "height": 60, "label": {"text": "API"}}),
                    json!({"type": "rectangle", "id": "db", "x": 300, "y": 0, "width": 160, "height": 60, "label": {"text": "DB"}}),
                    json!({"type": "arrow", "id": "calls", "x": 160, "y": 30, "start": {"id": "api"}, "end": {"id": "db"}}),
                ],
                file: None,
                project: project_arg(&project),
            }))
            .await;
    assert_eq!(answer(&added)["added"], json!(3), "{}", said(&added));
    assert_eq!(layers_in(&file).len(), 3);

    // The ids come back in the next call, which opens the file again.
    let updated = server
        .canvas_update(Parameters(CanvasUpdateArgs {
            elements: vec![json!({"id": "db", "text": "Database"})],
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_eq!(answer(&updated)["updated"], json!(1), "{}", said(&updated));
    let layers = layers_in(&file);
    let texts: Vec<&str> = layers.iter().filter_map(|l| l["text"].as_str()).collect();
    assert!(
        texts.contains(&"Database") && texts.contains(&"API"),
        "{texts:?}"
    );
    let arrow = layers
        .iter()
        .find(|l| l["type"] == json!("connector"))
        .unwrap();
    assert!(
        arrow["from"].is_string() && arrow["to"].is_string(),
        "{arrow}"
    );

    // A diagram from Mermaid is drawn as shapes the next call can edit.
    let drawn = server
        .canvas_add_diagram(Parameters(CanvasMermaidArgs {
            source: "graph TD; A-->B".into(),
            x: Some(0.0),
            y: Some(400.0),
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert!(
        answer(&drawn)["added"].as_u64().unwrap() >= 3,
        "{}",
        said(&drawn)
    );
    let after_mermaid = layers_in(&file).len();
    assert!(after_mermaid > 3, "{after_mermaid}");

    // A command without parameters.
    let zoomed = server
        .canvas_command_tool(CanvasCommandArgs {
            command: "view.zoomFit".into(),
            params: None,
            file: None,
            project: project_arg(&project),
        })
        .await;
    assert_ne!(zoomed.is_error, Some(true), "{}", said(&zoomed));

    let deleted = server
        .canvas_delete(Parameters(CanvasDeleteArgs {
            ids: vec!["calls".into(), "nope".into()],
            file: None,
            project: project_arg(&project),
        }))
        .await;
    let deleted = answer(&deleted);
    assert_eq!(deleted["deleted"], json!(1), "{deleted}");
    assert_eq!(deleted["missing"], json!(["nope"]), "{deleted}");
    assert_eq!(layers_in(&file).len(), after_mermaid - 1);

    // Clearing the canvas keeps what it held, in the `.bak` beside it.
    let cleared = server
        .canvas_clear(Parameters(CanvasTargetArgs {
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    assert!(layers_in(&file).is_empty());
    let kept = std::fs::read(file.with_extension("sawit.bak")).unwrap();
    let kept = BoardDoc::from_bytes(&kept).unwrap();
    let kept = ops::run(&mut { kept }, &json!({"op": "get"}));
    assert_eq!(
        kept["elements"].as_array().unwrap().len(),
        after_mermaid - 1
    );
}

#[tokio::test]
async fn parallel_calls_each_land_in_the_stored_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());

    // An agent's parallel tool calls: each opens the file, adds its box and saves.
    let calls = (0..8).map(|n| {
            server.canvas_add_elements(Parameters(CanvasElementsArgs {
                elements: vec![json!({"type": "rectangle", "id": format!("box-{n}"), "x": n * 200, "y": 0, "width": 100, "height": 60})],
                file: None,
                project: project_arg(&project),
            }))
        });
    for result in futures::future::join_all(calls).await {
        assert_ne!(result.is_error, Some(true), "{}", said(&result));
    }

    assert_eq!(layers_in(&board_file(&project)).len(), 8);
}

#[tokio::test]
async fn canvas_commands_lists_every_command_and_filters_with_their_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let list = |filter: Option<&str>| {
        server.canvas_commands(Parameters(CanvasCommandsArgs {
            filter: filter.map(str::to_string),
            file: None,
            project: project_arg(&project),
        }))
    };

    // The list reaches the agent whole, with no cut: the board's own commands are the last
    // that the engine registers, so a list that doesn't fit would lose exactly those.
    let all = list(None).await;
    assert!(!said(&all).contains("truncated"), "{}", said(&all).len());
    let all = answer(&all);
    let commands = all["commands"].as_array().unwrap();
    assert!(commands.len() > 500, "{}", commands.len());
    let ids: Vec<&str> = commands.iter().filter_map(|c| c["id"].as_str()).collect();
    assert!(ids.contains(&"board.sticky") && ids.contains(&"edit.undo"));
    // It is what `canvas_command` runs: what Sawit does itself isn't there.
    assert!(!ids.contains(&"file.save"));
    assert!(
        commands
            .iter()
            .all(|c| c["label"].is_string() && c.get("params").is_none())
    );

    // With a filter, the commands that match come with the parameters they take.
    let board = answer(&list(Some(" BOARD. ")).await);
    let board = board["commands"].as_array().unwrap();
    assert!(!board.is_empty() && board.len() < 50, "{}", board.len());
    let sticky = board
        .iter()
        .find(|c| c["id"] == json!("board.sticky"))
        .unwrap();
    assert!(
        sticky["params"].as_str().unwrap().contains("text"),
        "{sticky}"
    );
    // A part of a label counts too, and a filter that matches nothing is an empty list.
    let by_label = answer(&list(Some("sticky note")).await);
    assert!(
        by_label["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == json!("board.sticky"))
    );
    assert_eq!(
        answer(&list(Some("no such command")).await)["commands"],
        json!([])
    );
    // Blank is no filter.
    assert_eq!(answer(&list(Some("  ")).await), all);
}

/// Q12: an agent started in a subfolder of a project (`web/` of a repository) names no
/// project, and with the app closed its canvas tools draw on that project's canvas, the one
/// the app shows, as the app's own path does: they made a canvas of the subfolder's own,
/// which the app never shows. A diagram it renders first goes to that project's `.nyawit/`
/// too: in the subfolder's own, it made the folder that then took the canvas. A canvas above
/// the repository is left alone. The folder an agent runs in is its process's, so the calls
/// run in a process of their own: this test binary, run again in the subfolder for this test
/// alone.
#[test]
fn an_agent_in_a_subfolder_draws_on_its_projects_canvas() {
    const AGENT_HOME: &str = "SAWIT_TEST_AGENT_IN_A_SUBFOLDER";
    if let Some(home) = std::env::var_os(AGENT_HOME) {
        // The agent's side, run in the subfolder.
        let server = Sawit::new(AppPaths { root: home.into() });
        let render = {
            let server = server.clone();
            async move {
                let args = crate::diagrams::RenderArgs {
                    source: "flowchart LR\n  a --> b".into(),
                    output_path: None,
                    theme: crate::diagrams::Theme::Light,
                    inline: false,
                };
                server.render_diagram_tool(args, "render_diagram").await
            }
        };
        let rendered = within_seconds(render).expect("the render answered");
        assert_ne!(rendered.is_error, Some(true), "{}", said(&rendered));
        let sticky = json!({"type": "sticky", "x": 0, "y": 0, "text": "from web"});
        let add = async move {
            let elements = vec![sticky];
            let args = CanvasElementsArgs {
                elements,
                file: None,
                project: None,
            };
            server.canvas_add_elements(Parameters(args)).await
        };
        let added = within_seconds(add).expect("the call answered");
        assert_ne!(added.is_error, Some(true), "{}", said(&added));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    let web = project.join("web");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    std::fs::create_dir_all(&web).unwrap();
    // A canvas above the repository, as an agent run in the folder that holds it left one.
    let above = board_file(dir.path());
    write_board(&above, 1);
    let left = std::fs::read(&above).unwrap();
    let this_test =
        "app::canvas::tests::stored::an_agent_in_a_subfolder_draws_on_its_projects_canvas";
    let ran = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", this_test, "--nocapture"])
        .current_dir(&web)
        .env(AGENT_HOME, dir.path().join("home"))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&ran.stdout);
    assert!(
        ran.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(
        out.contains("1 passed"),
        "the agent's side didn't run: {out}"
    );
    assert!(texts_in(&board_file(&project)).contains("from web"));
    assert!(
        !web.join(".nyawit").exists(),
        "a .nyawit/ of the subfolder's own"
    );
    let diagrams = names(&sawit_core::nyawit::diagrams_dir(&project));
    assert!(
        diagrams.len() == 1 && diagrams[0].ends_with(".svg"),
        "{diagrams:?}"
    );
    assert_eq!(std::fs::read(&above).unwrap(), left, "the canvas above");
}

/// canvas_get and canvas_clear act on the page on show, the one last saved with the app
/// closed, as their descriptions say: they said "the canvas", and an agent took the layers of
/// one page for all of it. The other pages are where the descriptions point: page.list lists
/// them, and node.get reads a page's layers.
#[tokio::test]
async fn the_page_tools_act_on_the_page_on_show_and_reach_the_others_by_command() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_board(&board_file(&project), 2);
    let command = |command: &str, params: Value| {
        let run = server.canvas_command_tool(CanvasCommandArgs {
            command: command.into(),
            params: Some(params),
            file: None,
            project: project_arg(&project),
        });
        async {
            let ran = run.await;
            answer(&ran)["result"].clone()
        }
    };
    let steps = json!({"commands": [
        {"command": "page.add", "params": {"name": "Two"}},
        {"command": "board.sticky", "params": {"x": 0, "y": 0, "text": "on two"}},
        {"command": "page.select", "params": {"index": 0}},
    ]});
    command("command.batch", steps).await;
    let window = || CanvasTargetArgs {
        file: None,
        project: project_arg(&project),
    };

    let got = answer(&server.canvas_get_tool(window()).await);
    let texts: Vec<&str> = (got["elements"].as_array().unwrap().iter())
        .filter_map(|layer| layer["text"].as_str())
        .collect();
    assert_eq!(texts, ["note 0", "note 1"]);
    let pages = command("page.list", json!({})).await;
    let pages = pages.as_array().unwrap();
    assert_eq!(pages.len(), 2, "{pages:?}");
    let two = pages.iter().find(|page| page["name"] == "Two").unwrap()["id"].clone();
    let on_two = || async { command("node.get", json!({"id": two})).await["children"].clone() };
    assert_eq!(on_two().await.as_array().map(Vec::len), Some(1));

    let cleared = server.canvas_clear(Parameters(window())).await;
    assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
    assert!(answer(&server.canvas_get_tool(window()).await)["elements"] == json!([]));
    assert_eq!(on_two().await.as_array().map(Vec::len), Some(1));
}
