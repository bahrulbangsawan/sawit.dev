//! `canvas_export`: where it writes, in which format, never over a canvas file, with text drawn.

use rmcp::handler::server::wrapper::Parameters;

use super::*;
use crate::app::canvas::{CanvasCommandArgs, CanvasExportArgs, CanvasTargetArgs};
use crate::app::tests::{answer, native, said};

#[tokio::test]
async fn an_export_never_overwrites_the_canvas_file() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    let file = board_file(&project);
    write_board(&file, 1);
    let was = std::fs::read(&file).unwrap();

    // The canvas_command path: the guard is the op's own, so it holds with no app running.
    let refused = server
        .canvas_command_tool(CanvasCommandArgs {
            command: "file.exportSvg".into(),
            params: Some(json!({"path": file})),
            file: None,
            project: project_arg(&project),
        })
        .await;
    assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
    assert_eq!(
        said(&refused),
        "file.exportSvg would overwrite a canvas file"
    );
    assert_eq!(std::fs::read(&file).unwrap(), was);

    // The canvas_export path, for every canvas name and both formats.
    for name in [
        "canvas.sawit",
        "canvas.excalidraw",
        "canvas.sawit.bak",
        "canvas.sawit.del.bak",
    ] {
        for format in ["svg", "png"] {
            let target = file.with_file_name(name);
            let refused = server
                .canvas_export(Parameters(CanvasExportArgs {
                    format: Some(format.into()),
                    output_path: Some(target.display().to_string()),
                    file: None,
                    project: project_arg(&project),
                }))
                .await;
            assert_eq!(refused.is_error, Some(true), "{name}: {}", said(&refused));
            assert!(said(&refused).contains("would overwrite a canvas file"));
        }
    }
    assert_eq!(std::fs::read(&file).unwrap(), was);
    assert!(!file.with_file_name("canvas.excalidraw").exists());

    // The canvas is still there, with its sticky.
    let got = server
        .canvas_get_tool(CanvasTargetArgs {
            file: None,
            project: project_arg(&project),
        })
        .await;
    assert_eq!(answer(&got)["elements"].as_array().unwrap().len(), 1);

    // An export to an image path still writes.
    for (format, name) in [("svg", "page.svg"), ("png", "page.png")] {
        let out = dir.path().join(name);
        let done = server
            .canvas_export(Parameters(CanvasExportArgs {
                format: Some(format.into()),
                output_path: Some(out.display().to_string()),
                file: None,
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(done.is_error, Some(true), "{}", said(&done));
        assert!(!std::fs::read(&out).unwrap().is_empty());
    }
}

/// MCP-11: canvas_export writes where every tool that writes a file writes: an absolute path
/// (or `~/…`), or with none (or a blank one) the project's exports folder. A relative path was
/// written in sawit-mcp's folder and reported as given, and "PNG" was an unknown format.
#[tokio::test]
async fn canvas_export_reads_its_path_and_format_as_every_tool_does() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    write_board(&board_file(&project), 1);
    let export = |format: &str, output_path: &str| CanvasExportArgs {
        format: Some(format.into()),
        output_path: Some(output_path.into()),
        file: None,
        project: project_arg(&project),
    };

    // Through a file, so that nothing can be written under the crate whatever happens.
    let refused = server
        .canvas_export(Parameters(export("svg", "Cargo.toml/out.svg")))
        .await;
    assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
    assert!(
        said(&refused).contains("must be absolute"),
        "{}",
        said(&refused)
    );

    for (format, name) in [("PNG", "page.png"), (" Svg ", "page.svg")] {
        let out = dir.path().join(name);
        let done = server
            .canvas_export(Parameters(export(format, &out.display().to_string())))
            .await;
        assert_ne!(done.is_error, Some(true), "{format}: {}", said(&done));
        assert!(!std::fs::read(&out).unwrap().is_empty());
    }

    let blank = server.canvas_export(Parameters(export("svg", "  "))).await;
    assert_ne!(blank.is_error, Some(true), "{}", said(&blank));
    let exports = project.join(".nyawit").join("exports");
    assert!(
        said(&blank).contains(&exports.display().to_string()),
        "{}",
        said(&blank)
    );
}

/// The SVG `canvas_export` writes, with no app running, for a canvas of one sticky that says
/// `text`.
async fn exported_sticky(text: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    let file = board_file(&project);
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let sticky = json!({"x": 0, "y": 0, "text": text});
    board.execute("board.sticky", &sticky).unwrap();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, board.to_bytes().unwrap()).unwrap();

    let out = dir.path().join("page.svg");
    let done = server
        .canvas_export(Parameters(CanvasExportArgs {
            format: Some("svg".into()),
            output_path: Some(out.display().to_string()),
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(done.is_error, Some(true), "{}", said(&done));
    std::fs::read_to_string(out).unwrap()
}

#[tokio::test]
async fn a_headless_export_of_text_has_glyphs() {
    let with_text = exported_sticky("note 0").await;
    let without_text = exported_sticky("").await;

    // Text laid out with no font to draw it in comes out as a path with no outline (`d=""`),
    // and a sticky that says something then exports as one that says nothing. The sticky's own
    // rectangle and the text's path are two paths either way, so counting them proves nothing.
    assert!(!with_text.contains(r#"d="""#), "{with_text}");
    let (with, without) = (with_text.len(), without_text.len());
    assert!(
        with > without,
        "{with} bytes, no more than the {without} of a sticky with no text: {with_text}"
    );
}

/// canvas_export reads `output_path` as every other tool that writes a file does: trimmed,
/// with `~` for the home folder. It used to write the path as given, so `~/Desktop/board.svg`
/// made a folder named `~` in the folder the agent runs in, and a stray space at the end
/// became part of the file's name.
#[tokio::test]
async fn an_export_path_is_read_as_every_tool_reads_one() {
    let named = |path: &str| crate::output_path("canvas_export", Some(path)).unwrap();
    // `~`, without writing to the home folder: the path that is written is this one.
    if let Some(home) = std::env::var_os("HOME") {
        let home = Path::new(&home);
        let desktop = home.join("Desktop").join("board.svg");
        assert_eq!(named("~/Desktop/board.svg"), Some(desktop));
        assert_eq!(named(" ~/board.png \n"), Some(home.join("board.png")));
    }
    assert_eq!(named(" /a/board.svg "), Some(PathBuf::from("/a/board.svg")));

    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    write_board(&board_file(&project), 1);
    let out = dir.path().join("page.svg");
    let done = server
        .canvas_export(Parameters(CanvasExportArgs {
            format: Some("svg".into()),
            output_path: Some(format!("{}  ", out.display())),
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_ne!(done.is_error, Some(true), "{}", said(&done));
    let written = std::fs::read(&out).unwrap();
    assert_eq!(
        said(&done),
        format!(
            "Canvas exported to {} ({} bytes)",
            out.display(),
            written.len()
        )
    );
}
