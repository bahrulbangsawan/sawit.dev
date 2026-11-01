//! Calls the MCP tools directly (no stdio) against a temp Sawit home.

use std::path::{Path, PathBuf};

use rmcp::model::CallToolResult;
use sawit_board::{BoardDoc, ops, store};
use sawit_core::binaries::{SearchRoots, discover};
use sawit_core::control::{self, Preflight};
use sawit_core::instances::{AppPaths, Instance, ListenScope, Registry};
use sawit_mcp::Sawit;
use sawit_mcp::app::{CanvasCommandArgs, CanvasTargetArgs};
use sawit_mcp::db::{CreateDbArgs, DropDbArgs, ExecuteArgs, TableArgs};
use sawit_mcp::diagrams::{SchemaDiagramArgs, SchemaFormat, Theme};
use serde_json::{Value, json};

fn text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn list_instances_and_write_env() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: home.path().join("pd"),
    };
    let mut reg = Registry::default();
    reg.add(Instance {
        name: "shop".into(),
        bin_dir: "/nonexistent/bin".into(),
        data_dir: paths.cluster_dir("shop"),
        port: 5491,
        listen: ListenScope::Localhost,
        managed: true,
    })
    .unwrap();
    reg.save(&paths.instances_file()).unwrap();
    let server = Sawit::new(paths);

    let out = text(&server.list_instances_tool().await);
    assert!(out.contains("shop"), "{out}");
    assert!(
        out.contains("postgres://postgres@localhost:5491/postgres"),
        "{out}"
    );

    let project = home.path().join("tanstack-app");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".env"),
        "VITE_X=1\nDATABASE_URL=postgres://old\n",
    )
    .unwrap();
    let res = server
        .write_env_tool("shop", project.to_str().unwrap())
        .await;
    assert_ne!(res.is_error, Some(true), "{}", text(&res));
    assert_eq!(
        std::fs::read_to_string(project.join(".env")).unwrap(),
        "VITE_X=1\nDATABASE_URL=postgres://postgres@localhost:5491/postgres\n"
    );

    let missing = server
        .write_env_tool("nope", project.to_str().unwrap())
        .await;
    assert_eq!(missing.is_error, Some(true));
}

#[tokio::test]
async fn query_is_read_only() {
    let roots = SearchRoots::detect("/nonexistent".into()).await;
    let Some(bins) = discover(&roots).await.into_iter().next() else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: home.path().join("pd"),
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let inst = Instance {
        name: "mcp".into(),
        bin_dir: bins.bin_dir,
        data_dir: paths.cluster_dir("mcp"),
        port,
        listen: ListenScope::Localhost,
        managed: true,
    };
    let mut reg = Registry::default();
    reg.add(inst.clone()).unwrap();
    reg.save(&paths.instances_file()).unwrap();
    control::init_cluster(&inst).await.unwrap();
    control::start(&inst, &paths, vec![], Preflight::Force)
        .await
        .unwrap();

    let server = Sawit::new(paths);
    let ok = server
        .query_tool("mcp", "select 41 + 1 as answer", None, None)
        .await;
    assert!(text(&ok).contains("42"), "{}", text(&ok));
    let denied = server
        .query_tool("mcp", "create table t(x int)", None, None)
        .await;
    assert_eq!(denied.is_error, Some(true), "{}", text(&denied));
    // Even single statements a superuser could run must not reach the shell.
    let marker = home.path().join("pwned");
    let sql = format!(
        "DO $$ BEGIN EXECUTE 'COPY (SELECT 1) TO PROGRAM ''touch {}'''; END $$",
        marker.display()
    );
    let attack = server.query_tool("mcp", &sql, None, None).await;
    assert_eq!(attack.is_error, Some(true), "{}", text(&attack));
    assert!(!marker.exists(), "shell command ran via MCP");
    let schema = server.describe_schema_tool("mcp", None, None).await;
    assert!(text(&schema).contains("public"), "{}", text(&schema));

    // execute_sql writes; query then sees the rows.
    let wrote = server
        .execute_sql_tool(ExecuteArgs {
            target: "mcp".into(),
            sql: "create table t(id int primary key, x text not null default 'a'); insert into t(id) values (1), (2)".into(),
            database: None,
            max_rows: None,
        })
        .await;
    assert_ne!(wrote.is_error, Some(true), "{}", text(&wrote));
    assert!(text(&wrote).contains("2 rows affected"), "{}", text(&wrote));
    let count = server
        .query_tool("mcp", "select count(*) as n from t", None, None)
        .await;
    assert!(text(&count).contains("2"), "{}", text(&count));
    let table = server
        .describe_table_tool(TableArgs {
            target: "mcp".into(),
            schema: None,
            table: "t".into(),
            database: None,
        })
        .await;
    let table = text(&table);
    assert!(table.contains("id integer NOT NULL"), "{table}");
    assert!(table.contains("PRIMARY KEY"), "{table}");
    let diagram = server
        .schema_diagram_tool(SchemaDiagramArgs {
            target: "mcp".into(),
            schema: None,
            database: None,
            format: SchemaFormat::Mermaid,
            output_path: None,
            theme: Theme::Light,
        })
        .await;
    assert!(text(&diagram).contains("erDiagram"), "{}", text(&diagram));
    let svg_path = home.path().join("schema.svg");
    let svg = server
        .schema_diagram_tool(SchemaDiagramArgs {
            target: "mcp".into(),
            schema: Some("public".into()),
            database: None,
            format: SchemaFormat::Svg,
            output_path: Some(svg_path.display().to_string()),
            theme: Theme::Dark,
        })
        .await;
    assert_ne!(svg.is_error, Some(true), "{}", text(&svg));
    assert!(std::fs::read_to_string(&svg_path).unwrap().contains("<svg"));

    // Databases: create, address by `database`, drop only when confirmed.
    let created = server
        .create_database_tool(CreateDbArgs {
            target: "mcp".into(),
            name: "other db".into(),
            owner: None,
            template: None,
        })
        .await;
    assert_ne!(created.is_error, Some(true), "{}", text(&created));
    let listed = text(&server.list_databases_tool("mcp").await);
    assert!(listed.contains("- other db"), "{listed}");
    let current = server
        .query_tool("mcp", "select current_database()", Some("other db"), None)
        .await;
    assert!(text(&current).contains("other db"), "{}", text(&current));
    let refused = server
        .drop_database_tool(DropDbArgs {
            target: "mcp".into(),
            name: "other db".into(),
            confirm: "other".into(),
        })
        .await;
    assert_eq!(refused.is_error, Some(true));
    let dropped = server
        .drop_database_tool(DropDbArgs {
            target: "mcp".into(),
            name: "other db".into(),
            confirm: "other db".into(),
        })
        .await;
    assert_ne!(dropped.is_error, Some(true), "{}", text(&dropped));
    assert!(!text(&server.list_databases_tool("mcp").await).contains("other db"));

    control::stop(&inst).await.unwrap();
}

// The canvas tools while Sawit is closed. Each test gives the server a Sawit home of its own, so
// that no app answers on its socket, and names its project folder, so that nothing is written
// under the repository.

/// The Excalidraw board of `sawit-board`'s importer tests: 12 live elements, which import as 9
/// layers (a label folds into its rectangle, a frame holds its child).
const OLD_BOARD: &str = include_str!("../../board/tests/fixtures/excalidraw/basic.excalidraw");

/// A server whose home is in `dir`, where no app is running.
fn native(dir: &Path) -> Sawit {
    Sawit::new(AppPaths {
        root: dir.join("home"),
    })
}

/// A project folder in `dir`, with nothing in it yet: a project the agent works in for the first
/// time.
fn project_in(dir: &Path) -> PathBuf {
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    project
}

fn project_arg(project: &Path) -> Option<String> {
    Some(project.display().to_string())
}

/// `<project>/.nyawit/canvas.figcraft`.
fn board_file(project: &Path) -> PathBuf {
    project.join(".nyawit").join("canvas.figcraft")
}

/// `<project>/.nyawit/canvas.excalidraw`, the canvas as it was before the native one.
fn old_canvas(project: &Path) -> PathBuf {
    project.join(".nyawit").join("canvas.excalidraw")
}

fn write_file(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A canvas file at `path` with `stickies` stickies, written as the app writes one.
fn write_board(path: &Path, stickies: usize) {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    for n in 0..stickies {
        let sticky = json!({"x": n as f64 * 300.0, "y": 0, "text": format!("note {n}")});
        board.execute("board.sticky", &sticky).unwrap();
    }
    write_file(path, board.to_bytes().unwrap());
}

/// A canvas file at `path` with one sticky, "kept", and one image more than FigCraft reads from a
/// file (20 000): that image stays in the file, unread, so a save over the file would delete it.
/// Building its 20 001 archive members takes most of a second.
fn write_board_with_an_image_past_the_limit(path: &Path) {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let sticky = json!({"x": 0, "y": 0, "text": "kept"});
    board.execute("board.sticky", &sticky).unwrap();
    let doc = board.document_itself().unwrap();
    let document = serde_json::to_vec(&figcraft_format::to_json(&doc).unwrap()).unwrap();
    let names: Vec<String> = (0..=20_000).map(|n| format!("images/i{n}")).collect();
    let members = std::iter::once(("document.json", document.as_slice()))
        .chain(names.iter().map(|name| (name.as_str(), &b"x"[..])));
    write_file(path, figcraft_format::zip_files(members).unwrap());
}

/// The file at `path` as the disk has it: a file that is written again, even with the same bytes,
/// is a new one (saves are renames).
fn on_disk(path: &Path) -> (u64, i64, i64) {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).unwrap();
    (meta.ino(), meta.mtime(), meta.mtime_nsec())
}

/// What `get` lists on the canvas file at `path`, opened as the app opens it.
fn layers_in(path: &Path) -> Vec<Value> {
    let mut board = store::open(path).unwrap();
    let reply = ops::run(&mut board, &json!({"op": "get"}));
    reply["elements"].as_array().unwrap().clone()
}

/// The `elements` of a successful `canvas_get`.
fn elements(result: &CallToolResult) -> Vec<Value> {
    assert_ne!(result.is_error, Some(true), "{}", text(result));
    let reply: Value = serde_json::from_str(&text(result)).unwrap();
    reply["elements"].as_array().unwrap().clone()
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

async fn get(server: &Sawit, project: &Path) -> CallToolResult {
    let project = project_arg(project);
    server
        .canvas_get_tool(CanvasTargetArgs {
            file: None,
            project,
        })
        .await
}

async fn command(server: &Sawit, project: &Path, id: &str, params: Value) -> CallToolResult {
    let args = CanvasCommandArgs {
        command: id.into(),
        params: Some(params),
        file: None,
        project: project_arg(project),
    };
    server.canvas_command_tool(args).await
}

#[tokio::test]
async fn offline_get_reads_figcraft_then_excalidraw() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());

    // A project whose canvas is a .figcraft file: its two stickies.
    let figcraft = dir.path().join("figcraft-project");
    write_board(&board_file(&figcraft), 2);
    let listed = elements(&get(&server, &figcraft).await);
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert!(listed.iter().all(|e| e["type"] == json!("sticky")));
    assert!(listed.iter().any(|e| e["text"] == json!("note 1")));

    // A project with only the old canvas: what the importer makes of it, from nothing but a read.
    let old = dir.path().join("old-project");
    write_file(&old_canvas(&old), OLD_BOARD);
    let listed = elements(&get(&server, &old).await);
    assert_eq!(listed.len(), 9, "{listed:?}");
    assert!(listed.iter().any(|e| e["text"] == json!("Service")));
    // Native ids: the layers' own, not the Excalidraw ids ("box").
    assert!(listed.iter().all(|e| e["id"] != json!("box")), "{listed:?}");
    assert_eq!(names(&old.join(".nyawit")), ["canvas.excalidraw"]);
    assert_eq!(
        std::fs::read_to_string(old_canvas(&old)).unwrap(),
        OLD_BOARD
    );

    // With both files, the .figcraft is the canvas: the old one is imported once.
    write_board(&board_file(&old), 1);
    assert_eq!(elements(&get(&server, &old).await).len(), 1);

    // With neither, the canvas is empty and nothing is made.
    let empty = dir.path().join("empty-project");
    std::fs::create_dir(&empty).unwrap();
    assert!(elements(&get(&server, &empty).await).is_empty());
    assert!(names(&empty).is_empty());
}

#[tokio::test]
async fn offline_command_edits_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());

    let sticky = json!({"x": 0, "y": 0});
    let made = command(&server, &project, "board.sticky", sticky.clone()).await;
    assert_ne!(made.is_error, Some(true), "{}", text(&made));
    let reply: Value = serde_json::from_str(&text(&made)).unwrap();
    assert_eq!(reply["ok"], json!(true), "{reply}");

    let layers = layers_in(&board_file(&project));
    assert_eq!(layers.len(), 1, "{layers:?}");
    assert_eq!(layers[0]["type"], json!("sticky"));
    // The new .nyawit/ keeps its own files out of git, and nothing else is left in it.
    assert_eq!(
        names(&project.join(".nyawit")),
        [".gitignore", "canvas.figcraft"]
    );

    // The next call opens the file as it is now, so the canvas grows.
    let more = command(&server, &project, "board.sticky", sticky).await;
    assert_ne!(more.is_error, Some(true), "{}", text(&more));
    assert_eq!(layers_in(&board_file(&project)).len(), 2);
}

#[tokio::test]
async fn an_offline_edit_of_an_old_canvas_saves_the_import_and_keeps_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_file(&old_canvas(&project), OLD_BOARD);

    let made = command(
        &server,
        &project,
        "board.sticky",
        json!({"x": 2000, "y": 0}),
    )
    .await;
    assert_ne!(made.is_error, Some(true), "{}", text(&made));

    // The canvas file holds what the old canvas imports to, and the new sticky. The old canvas
    // is as it was.
    assert_eq!(layers_in(&board_file(&project)).len(), 10);
    assert_eq!(
        names(&project.join(".nyawit")),
        ["canvas.excalidraw", "canvas.figcraft"]
    );
    assert_eq!(
        std::fs::read_to_string(old_canvas(&project)).unwrap(),
        OLD_BOARD
    );

    // From then on the canvas file is the canvas: the old one is not imported again.
    write_file(&old_canvas(&project), r#"{"type": "excalidraw"}"#);
    assert_eq!(elements(&get(&server, &project).await).len(), 10);
}

#[tokio::test]
async fn offline_get_and_other_reads_write_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_board(&board_file(&project), 2);
    let before = on_disk(&board_file(&project));

    elements(&get(&server, &project).await);
    // A view command changes what is on show and not the document.
    let zoom = command(&server, &project, "view.zoomIn", json!({})).await;
    assert_ne!(zoom.is_error, Some(true), "{}", text(&zoom));

    assert_eq!(on_disk(&board_file(&project)), before);
    assert_eq!(names(&project.join(".nyawit")), ["canvas.figcraft"]);
}

/// Plan 7 final review I-1 (ruling P7-C1, offline): a version of the canvas shown in place of the
/// document is no edit. Nothing is saved, so the file keeps the document, and no backup of a
/// canvas being emptied is made either, though the version on show is blank.
#[tokio::test]
async fn an_offline_version_preview_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    // A canvas with a sticky, and a version from when it was blank.
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    board
        .execute("file.saveVersion", &json!({"name": "Blank"}))
        .unwrap();
    let sticky = json!({"x": 0, "y": 0, "text": "now"});
    board.execute("board.sticky", &sticky).unwrap();
    write_file(&file, board.to_bytes().unwrap());
    let before = on_disk(&file);

    let preview = command(&server, &project, "version.preview", json!({"index": 0})).await;
    assert_ne!(preview.is_error, Some(true), "{}", text(&preview));

    assert_eq!(on_disk(&file), before);
    assert_eq!(names(&project.join(".nyawit")), ["canvas.figcraft"]);
    assert_eq!(layers_in(&file).len(), 1);
}

/// Plan 7 final review I-1 (ruling P7-C1, offline): a canvas file that holds images FigCraft's
/// loader left out, past what it reads from one file, is never saved over, which would delete
/// them. The edit is refused in FigCraft's words, the file stays as it is, and the canvas still
/// reads.
#[tokio::test]
async fn an_offline_edit_never_saves_over_images_left_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let file = board_file(&project);
    write_board_with_an_image_past_the_limit(&file);
    let before = std::fs::read(&file).unwrap();

    let sticky = command(&server, &project, "board.sticky", json!({"x": 300, "y": 0})).await;
    assert_eq!(sticky.is_error, Some(true), "{}", text(&sticky));
    assert!(
        text(&sticky).starts_with("canvas wasn't saved: 1 of its images weren't loaded"),
        "{}",
        text(&sticky)
    );
    assert!(
        std::fs::read(&file).unwrap() == before,
        "the file was saved"
    );
    // Beside it is only the copy the store keeps of a file it opens with notes.
    assert_eq!(
        names(&project.join(".nyawit")),
        ["canvas.figcraft", "canvas.figcraft.lossy.bak"]
    );

    let listed = elements(&get(&server, &project).await);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["text"], json!("kept"));
}

/// Plan 7 final review T5-7: a project the agent names must be a folder that is there. The app
/// answers only for a project one of its windows has open; with it closed, a name that is no
/// folder (a project's name rather than its path) got a canvas of its own, in a new folder by
/// that name, which neither the agent nor the user would see.
#[tokio::test]
async fn an_offline_canvas_needs_a_project_folder_that_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let nowhere = dir.path().join("sawit.dev");

    let listed = get(&server, &nowhere).await;
    assert_eq!(listed.is_error, Some(true), "{}", text(&listed));
    assert!(
        text(&listed).contains("is not a folder"),
        "{}",
        text(&listed)
    );
    let sticky = command(&server, &nowhere, "board.sticky", json!({"x": 0, "y": 0})).await;
    assert_eq!(sticky.is_error, Some(true), "{}", text(&sticky));
    assert!(
        text(&sticky).contains("is not a folder"),
        "{}",
        text(&sticky)
    );
    assert!(!nowhere.exists());

    // A file is no project folder either.
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "notes").unwrap();
    let sticky = command(&server, &file, "board.sticky", json!({"x": 0, "y": 0})).await;
    assert_eq!(sticky.is_error, Some(true), "{}", text(&sticky));
    assert!(
        text(&sticky).contains("is not a folder"),
        "{}",
        text(&sticky)
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "notes");
}

#[tokio::test]
async fn offline_command_failures_are_tool_errors_that_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_board(&board_file(&project), 1);
    let before = std::fs::read(board_file(&project)).unwrap();

    // The engine's refusal comes back as the tool's error, not as a success with `ok: false`.
    let unknown = command(&server, &project, "no.such.command", json!({})).await;
    assert_eq!(unknown.is_error, Some(true), "{}", text(&unknown));
    assert!(
        text(&unknown).contains("no.such.command"),
        "{}",
        text(&unknown)
    );
    // So is a command that Sawit does itself.
    let save = command(&server, &project, "file.save", json!({})).await;
    assert_eq!(save.is_error, Some(true), "{}", text(&save));
    assert!(text(&save).contains("handled by Sawit"), "{}", text(&save));

    assert_eq!(std::fs::read(board_file(&project)).unwrap(), before);
}

#[tokio::test]
async fn an_old_canvas_the_importer_rejects_is_summarised_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    // An Excalidraw file with no elements at all: the importer refuses it, and the old reader
    // says what it always did, that there is nothing on it.
    let old = r#"{"type": "excalidraw"}"#;
    write_file(&old_canvas(&project), old);

    let listed = get(&server, &project).await;
    assert!(elements(&listed).is_empty(), "{}", text(&listed));

    // Nothing writes a canvas over it: a blank board saved in its place would hide the old one.
    let sticky = command(&server, &project, "board.sticky", json!({"x": 0, "y": 0})).await;
    assert_eq!(sticky.is_error, Some(true), "{}", text(&sticky));
    assert!(
        text(&sticky).contains("canvas.excalidraw"),
        "{}",
        text(&sticky)
    );
    assert_eq!(names(&project.join(".nyawit")), ["canvas.excalidraw"]);
    assert_eq!(std::fs::read_to_string(old_canvas(&project)).unwrap(), old);
}

#[tokio::test]
async fn a_corrupt_canvas_is_an_error_and_never_the_old_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_file(&old_canvas(&project), OLD_BOARD);
    write_file(&board_file(&project), "this is not a canvas");

    // The canvas is the .figcraft: its error is the answer, not the elements of a board the
    // canvas left months ago.
    let listed = get(&server, &project).await;
    assert_eq!(listed.is_error, Some(true), "{}", text(&listed));
    assert!(
        text(&listed).contains("canvas.figcraft"),
        "{}",
        text(&listed)
    );
    assert!(!text(&listed).contains("Service"), "{}", text(&listed));

    // And no edit goes over it.
    let sticky = command(&server, &project, "board.sticky", json!({"x": 0, "y": 0})).await;
    assert_eq!(sticky.is_error, Some(true), "{}", text(&sticky));
    assert_eq!(
        std::fs::read_to_string(board_file(&project)).unwrap(),
        "this is not a canvas"
    );
    assert_eq!(
        names(&project.join(".nyawit")),
        ["canvas.excalidraw", "canvas.figcraft"]
    );
}
