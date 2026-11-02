//! The screen tools: canvas_begin {kind: "screen"}, canvas_screen_write, canvas_screen_get and
//! canvas_screen_export, with Sawit closed (on the saved file and the screen's folder) and
//! running (a stand-in answers on its socket).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ContentBlock;

use super::running::{fake_app, next};
use super::work::begin;
use super::*;
use crate::app::canvas::screens::UNRENDERED;
use crate::app::canvas::{CanvasScreenExportArgs, CanvasScreenGetArgs, CanvasScreenWriteArgs};
use crate::app::tests::{answer, native, said};

fn write(project: &Path, screen: &str) -> CanvasScreenWriteArgs {
    CanvasScreenWriteArgs {
        work: None,
        screen: Some(screen.into()),
        html: None,
        edits: None,
        files: None,
        file: None,
        project: project_arg(project),
    }
}

fn get(project: &Path, screen: Option<&str>) -> CanvasScreenGetArgs {
    CanvasScreenGetArgs {
        work: None,
        screen: screen.map(Into::into),
        file: None,
        project: project_arg(project),
    }
}

const PAGE: &str = "<!doctype html><html><head><link rel=\"stylesheet\" href=\"css/site.css\">\
                    </head><body><h1 class=\"title\">Sign in</h1></body></html>";

/// With Sawit closed, a screen's frame is saved and its folder made at once, its files are
/// written and read back, and each answer says nothing is rendered.
#[tokio::test]
async fn with_sawit_closed_screens_are_files_and_frames() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board(&file, 1);
    let mut args = begin(
        &project,
        "Sign in",
        json!({"x": 0, "y": 0, "width": 390, "height": 844}),
    );
    args.kind = Some("screen".into());
    let begun = answer(&server.canvas_begin(Parameters(args)).await);
    assert_eq!(begun["screen"], "sign-in", "{begun}");
    assert_eq!(begun["note"], UNRENDERED);
    let folder = project.join(".nyawit/screens/sign-in");
    assert!(folder.is_dir(), "the folder is made");
    assert!(
        layers_in(&file).iter().any(|l| l["id"] == begun["frame"]),
        "the frame is saved"
    );

    let mut first = write(&project, "sign-in");
    first.work = begun["work"].as_str().map(Into::into);
    first.screen = None;
    first.html = Some(PAGE.into());
    first.files = Some(vec![
        json!({"path": "css/site.css", "content": ".title{color:#123}"}),
    ]);
    let wrote = answer(&server.canvas_screen_write(Parameters(first)).await);
    assert_eq!(wrote["written"], json!(["index.html", "css/site.css"]));
    assert_eq!(wrote["rendered"], false);
    assert_eq!(
        std::fs::read_to_string(folder.join("index.html")).unwrap(),
        PAGE
    );

    let mut edit = write(&project, "Sign in");
    edit.edits = Some(vec![
        json!({"find": ">Sign in<", "replace": ">Welcome back<"}),
    ]);
    answer(&server.canvas_screen_write(Parameters(edit)).await);
    let mut missing = write(&project, "sign-in");
    missing.edits = Some(vec![json!({"find": "nowhere", "replace": "x"})]);
    let refused = server.canvas_screen_write(Parameters(missing)).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(said(&refused).contains("isn't in it"), "{}", said(&refused));

    let got = server
        .canvas_screen_get(Parameters(get(&project, Some("sign-in"))))
        .await;
    assert!(
        !got.content
            .iter()
            .any(|c| matches!(c, ContentBlock::Image(_))),
        "no picture"
    );
    let got = answer(&got);
    assert!(got["html"].as_str().unwrap().contains("Welcome back"));
    assert_eq!(got["files"], json!(["css/site.css", "index.html"]));
    assert_eq!(got["viewport"], json!({"w": 390.0, "h": 844.0}));
    assert_eq!(got["note"], UNRENDERED);
    let listed = answer(
        &server
            .canvas_screen_get(Parameters(get(&project, None)))
            .await,
    );
    assert_eq!(listed["screens"][0]["screen"], "sign-in");

    let unknown = server.canvas_screen_write(Parameters(CanvasScreenWriteArgs {
        html: Some("<p>x</p>".into()),
        ..write(&project, "login")
    }));
    let unknown = unknown.await;
    assert!(
        said(&unknown).contains("no screen is called “login”"),
        "{}",
        said(&unknown)
    );
}

/// A screen exports as one HTML file, or as a React component with its CSS, where the agent
/// says, with the app closed too.
#[tokio::test]
async fn screens_export_as_html_and_react() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_board(&board_file(&project), 0);
    let mut args = begin(&project, "Sign in", json!({"x": 0, "y": 0}));
    args.kind = Some("screen".into());
    answer(&server.canvas_begin(Parameters(args)).await);
    let mut page = write(&project, "sign-in");
    page.html = Some(PAGE.into());
    page.files = Some(vec![
        json!({"path": "css/site.css", "content": ".title{color:#123}"}),
    ]);
    answer(&server.canvas_screen_write(Parameters(page)).await);
    let out = dir.path().join("out");
    let export = |format: &str, path: PathBuf| CanvasScreenExportArgs {
        work: None,
        screen: Some("sign-in".into()),
        format: Some(format.into()),
        path: Some(path.display().to_string()),
        file: None,
        project: project_arg(&project),
    };
    let html = out.join("sign-in.html");
    answer(
        &server
            .canvas_screen_export(Parameters(export("html", html.clone())))
            .await,
    );
    let single = std::fs::read_to_string(&html).unwrap();
    assert!(single.contains(".title{color:#123}") && !single.contains("css/site.css"));
    let tsx = out.join("SignIn.tsx");
    let made = answer(
        &server
            .canvas_screen_export(Parameters(export("react", tsx.clone())))
            .await,
    );
    assert_eq!(made["format"], "react");
    let component = std::fs::read_to_string(&tsx).unwrap();
    assert!(component.contains("className=\"title\""), "{component}");
    assert!(component.contains("import \"./SignIn.css\""), "{component}");
    assert!(
        std::fs::read_to_string(out.join("SignIn.css"))
            .unwrap()
            .contains(".title")
    );
    let wrong = server
        .canvas_screen_export(Parameters(export("pdf", out.join("x.pdf"))))
        .await;
    assert!(said(&wrong).contains("use html or react"));
}

/// With Sawit running, the screen tools are the app's ops: it writes, renders and pictures the
/// screen, and its picture comes back as an image. A write that says nothing never reaches it.
#[tokio::test]
async fn with_sawit_running_screen_ops_go_to_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    let wrote =
        json!({"ok": true, "screen": "sign-in", "written": ["index.html"], "rendered": true});
    let got = json!({"ok": true, "screen": "sign-in", "html": PAGE, "png_base64": "iVBORw0KGgo="});
    let asked = fake_app(&server.paths, vec![wrote.clone(), got]);
    let mut page = write(&project, "sign-in");
    page.html = Some(PAGE.into());
    assert_eq!(
        answer(&server.canvas_screen_write(Parameters(page)).await),
        wrote
    );
    let op = next(&asked);
    assert_eq!(
        (op["op"].as_str(), op["html"].as_str()),
        (Some("screen_write"), Some(PAGE))
    );
    let picture = server
        .canvas_screen_get(Parameters(get(&project, Some("sign-in"))))
        .await;
    assert!(
        matches!(&picture.content[0], ContentBlock::Image(image) if image.data == "iVBORw0KGgo=")
    );
    assert_eq!(next(&asked)["op"], "screen_get");
    let silent = server
        .canvas_screen_write(Parameters(write(&project, "sign-in")))
        .await;
    assert_eq!(silent.is_error, Some(true));
    assert!(
        asked.recv_timeout(Duration::from_millis(200)).is_err(),
        "it reached the app"
    );
}
