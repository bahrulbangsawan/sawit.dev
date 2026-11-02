//! canvas_begin, canvas_progress and the canvas tools' `work`, with Sawit closed (on the saved
//! file) and running (a stand-in answers on its socket).

use rmcp::handler::server::wrapper::Parameters;
use sawit_core::agents::work::WORK_KEY;

use super::running::{fake_app, next};
use super::*;
use crate::app::canvas::{
    CanvasBeginArgs, CanvasCommandArgs, CanvasElementsArgs, CanvasProgressArgs, CanvasUpdateArgs,
};
use crate::app::tests::{answer, native, said};

pub(super) fn begin(project: &Path, name: &str, frame: Value) -> CanvasBeginArgs {
    CanvasBeginArgs {
        name: name.into(),
        frame,
        steps: Some(vec!["Laying out".into(), "Writing copy".into()]),
        kind: None,
        file: None,
        project: project_arg(project),
    }
}

fn progress(project: &Path, work: &str, status: Option<&str>) -> CanvasProgressArgs {
    CanvasProgressArgs {
        work: work.into(),
        step: Some("Writing copy".into()),
        status: status.map(Into::into),
        message: None,
        file: None,
        project: project_arg(project),
    }
}

/// With Sawit closed, a work's frame is made in the saved file at once, what an op of the work
/// draws goes into it, from its corner, and progress changes nothing and says so.
#[tokio::test]
async fn with_sawit_closed_a_work_is_built_in_the_saved_file() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 1);
    let frame = json!({"x": 400, "y": 0, "width": 300, "height": 200});
    let begun = server
        .canvas_begin(Parameters(begin(&project, "Pricing card", frame)))
        .await;
    let begun = answer(&begun);
    let work = begun["work"].as_str().unwrap().to_string();
    let card = layers_in(&file)
        .into_iter()
        .find(|l| l["id"] == begun["frame"]);
    let card = card.expect("the frame is saved");
    assert_eq!(
        (card["type"].clone(), card["x"].clone()),
        (json!("frame"), json!(400.0))
    );
    let saved = store::open(&file).unwrap().doc().unwrap();
    let frame = begun["frame"].as_str().unwrap();
    assert_eq!(
        saved.node(frame.parse().unwrap()).unwrap().plugin_data[WORK_KEY],
        work
    );

    let text = json!({"type": "text", "id": "price", "x": 16, "y": 16, "text": "$24"});
    let added = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![text],
            file: None,
            project: project_arg(&project),
            work: Some(work.clone()),
        }))
        .await;
    assert_eq!(answer(&added)["work"], json!(work), "{}", said(&added));
    let saved = store::open(&file).unwrap().doc().unwrap();
    let price = answer(&added)["ids"][0].as_str().unwrap().parse().unwrap();
    let parent = saved.node(price).unwrap().parent.map(|p| p.to_string());
    assert_eq!(parent.as_deref(), Some(frame));
    let grown = server
        .canvas_update(Parameters(CanvasUpdateArgs {
            elements: vec![json!({"id": "price", "append": " per month"})],
            file: None,
            project: project_arg(&project),
            work: Some(work.clone()),
        }))
        .await;
    assert_ne!(grown.is_error, Some(true), "{}", said(&grown));
    let saved = store::open(&file).unwrap().doc().unwrap();
    let grown = saved
        .node(price)
        .and_then(|n| Some(n.kind.text()?.characters.clone()));
    assert_eq!(
        grown.as_deref(),
        Some("$24 per month"),
        "the text inside the frame"
    );

    let before = std::fs::read(&file).unwrap();
    let told = server
        .canvas_progress(Parameters(progress(&project, &work, Some("done"))))
        .await;
    let told = answer(&told);
    assert_eq!(
        (told["shown"].clone(), told["status"].clone()),
        (json!(false), json!("done"))
    );
    assert_eq!(told["note"], ops::PROGRESS_UNSHOWN);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "progress wrote nothing"
    );
    let unknown = server
        .canvas_progress(Parameters(progress(&project, "w_00000000", None)))
        .await;
    assert_eq!(unknown.is_error, Some(true));
    assert!(
        said(&unknown).contains("isn't on the canvas"),
        "{}",
        said(&unknown)
    );
}

/// An agent may not move the user's view or page: it is refused, with the app closed too, and
/// nothing is written.
#[tokio::test]
async fn view_commands_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 1);
    let before = std::fs::read(&file).unwrap();
    for (command, params) in [
        ("page.select", json!({"index": 0})),
        ("view.zoomFit", json!({})),
        (
            "command.batch",
            json!({"commands": [{"command": "view.zoomIn"}]}),
        ),
    ] {
        let ran = server
            .canvas_command_tool(CanvasCommandArgs {
                command: command.into(),
                params: Some(params),
                file: None,
                project: project_arg(&project),
                work: None,
            })
            .await;
        assert_eq!(ran.is_error, Some(true), "{command}");
        assert!(said(&ran).contains("moves the view"), "{}", said(&ran));
    }
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

/// With Sawit running, canvas_begin and canvas_progress go to the app as ops, and so does each
/// tool's `work`; a view command never reaches it.
#[tokio::test]
async fn with_sawit_running_work_ops_go_to_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    let reply = json!({"ok": true, "work": "w_1", "frame": "5", "page": "1"});
    let asked = fake_app(&server.paths, vec![reply.clone()]);
    let frame = json!({"near": "hero", "side": "below"});
    let begun = server
        .canvas_begin(Parameters(begin(&project, "Card", frame.clone())))
        .await;
    assert_eq!(answer(&begun), reply);
    let op = next(&asked);
    assert_eq!(
        (op["op"].clone(), op["name"].clone()),
        (json!("begin"), json!("Card"))
    );
    assert_eq!(op["frame"], frame);
    assert_eq!(op["steps"], json!(["Laying out", "Writing copy"]));
    let mut failed = progress(&project, "w_1", Some("failed"));
    failed.message = Some("Couldn't load the logo".into());
    server.canvas_progress(Parameters(failed)).await;
    let op = next(&asked);
    assert_eq!(op["op"], "progress");
    assert_eq!(
        (op["status"].clone(), op["message"].clone()),
        (json!("failed"), json!("Couldn't load the logo"))
    );
    server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![json!({"type": "text", "text": "Hi"})],
            file: None,
            project: project_arg(&project),
            work: Some("w_1".into()),
        }))
        .await;
    assert_eq!(next(&asked)["work"], "w_1");
    let zoom = server
        .canvas_command_tool(CanvasCommandArgs {
            command: "view.zoomIn".into(),
            params: None,
            file: None,
            project: project_arg(&project),
            work: None,
        })
        .await;
    assert_eq!(zoom.is_error, Some(true));
    assert!(
        asked.recv_timeout(Duration::from_millis(200)).is_err(),
        "it reached the app"
    );
}
