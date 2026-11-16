//! canvas_screen_export of an artboard: a frame of layers, by id or name, as HTML or React.

use rmcp::handler::server::wrapper::Parameters;

use super::*;
use crate::app::canvas::CanvasScreenExportArgs;
use crate::app::tests::{answer, native};

fn export(project: &Path, id: &str, format: &str) -> CanvasScreenExportArgs {
    CanvasScreenExportArgs {
        work: None,
        screen: None,
        id: Some(id.into()),
        format: Some(format.into()),
        path: None,
        overwrite: None,
        file: None,
        tab: None,
        project: project_arg(project),
    }
}

#[tokio::test]
async fn an_artboard_exports_as_html_and_react_with_the_app_closed() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let frame = json!({"x": 0, "y": 0, "width": 320, "height": 200, "name": "Plan card"});
    board.execute("frame.create", &frame).unwrap();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, board.to_bytes().unwrap()).unwrap();

    let html = answer(
        &server
            .canvas_screen_export(Parameters(export(&project, "Plan card", "html")))
            .await,
    );
    let written = project.join(".nyawit/exports/plan-card.html");
    assert_eq!(html["files"], json!([written]), "{html}");
    let text = std::fs::read_to_string(&written).unwrap();
    assert!(text.contains("data-name=\"Plan card\"") && text.contains("Mechanical reference"));

    let react = answer(
        &server
            .canvas_screen_export(Parameters(export(&project, "Plan card", "react")))
            .await,
    );
    let tsx = project.join(".nyawit/exports/PlanCard.tsx");
    let css = project.join(".nyawit/exports/PlanCard.module.css");
    assert_eq!(react["files"], json!([tsx, css]), "{react}");
    assert!(
        std::fs::read_to_string(&tsx)
            .unwrap()
            .contains("import styles from \"./PlanCard.module.css\";")
    );

    let missing = server
        .canvas_screen_export(Parameters(export(&project, "Nothing", "html")))
        .await;
    assert_eq!(missing.is_error, Some(true));
}

/// canvas_handoff writes the same folder as Send to Agent, and answers its files and prompt; with
/// an agent and the app closed, the command that would start it.
#[tokio::test]
async fn an_artboard_hands_off_with_the_app_closed() {
    use crate::app::canvas::CanvasHandoffArgs;
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let frame = json!({"x": 0, "y": 0, "width": 393, "height": 852, "name": "Settings"});
    board.execute("frame.create", &frame).unwrap();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, board.to_bytes().unwrap()).unwrap();
    let args = CanvasHandoffArgs {
        id: Some(json!("Settings")),
        work: None,
        screen: None,
        agent: Some("claude-code".into()),
        file: None,
        project: project_arg(&project),
    };
    let done = answer(&server.canvas_handoff(Parameters(args)).await);
    let out = project.join(".nyawit/handoff/settings");
    assert_eq!(done["folder"], json!(out), "{done}");
    for name in [
        "design.png",
        "settings.html",
        "Settings.tsx",
        "spec.md",
        "prompt.md",
    ] {
        assert!(out.join(name).is_file(), "no {name}: {done}");
    }
    assert!(
        done["prompt"]
            .as_str()
            .unwrap()
            .starts_with("Build the screen \"Settings\"")
    );
    let prompt = out.join("prompt.md");
    assert_eq!(
        done["command"],
        format!("claude \"$(cat -- {})\"", prompt.display())
    );
    assert!(
        done["note"].as_str().unwrap().contains("wasn't opened"),
        "{done}"
    );
    let nothing = CanvasHandoffArgs {
        id: None,
        work: None,
        screen: None,
        agent: None,
        file: None,
        project: project_arg(&project),
    };
    assert_eq!(
        server.canvas_handoff(Parameters(nothing)).await.is_error,
        Some(true)
    );
}
