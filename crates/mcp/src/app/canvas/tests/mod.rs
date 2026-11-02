//! The canvas tools' tests, with Sawit running (a stand-in answers on its socket) and closed, and
//! the helpers they share.

mod backups;
mod export;
mod files;
mod load;
mod running;
mod saves;
mod stored;

use std::path::{Path, PathBuf};
use std::time::Duration;

use sawit_board::{BoardDoc, ops, store};
use serde_json::{Value, json};

/// The Excalidraw board of `sawit-board`'s importer tests: 12 live elements, which import as
/// 9 layers.
const OLD_BOARD: &str =
    include_str!("../../../../../board/tests/fixtures/excalidraw/basic.excalidraw");

fn project_arg(project: &Path) -> Option<String> {
    Some(project.display().to_string())
}

/// A project folder in `dir`, with nothing in it yet: a project the agent works in for the
/// first time.
fn project_in(dir: &Path) -> PathBuf {
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    project
}

/// `<project>/.nyawit/canvas.sawit`.
fn board_file(project: &Path) -> PathBuf {
    project.join(".nyawit").join("canvas.sawit")
}

/// A canvas file at `path` with `stickies` stickies, written as the app writes one.
fn write_board(path: &Path, stickies: usize) {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    for n in 0..stickies {
        let sticky = json!({"x": n as f64 * 300.0, "y": 0, "text": format!("note {n}")});
        board.execute("board.sticky", &sticky).unwrap();
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, board.to_bytes().unwrap()).unwrap();
}

/// What `get` lists on the canvas file at `path`, opened as the app opens it.
fn layers_in(path: &Path) -> Vec<Value> {
    let mut board = store::open(path).unwrap();
    let reply = ops::run(&mut board, &json!({"op": "get"}));
    reply["elements"].as_array().unwrap().clone()
}

/// Runs `call` on a runtime of its own, on a thread of its own, and gives what it answered:
/// `None` when it hadn't answered within 5 s (the thread is left as it is). For a call whose
/// read could block for ever, so that a test of it fails rather than hangs.
fn within_seconds<F>(call: F) -> Option<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = tx.send(runtime.block_on(call));
    });
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

/// The texts of the stickies and other text layers on the canvas stored in `file`.
fn texts_in(file: &Path) -> std::collections::HashSet<String> {
    let layers = layers_in(file);
    let texts = layers.iter().filter_map(|layer| layer["text"].as_str());
    texts.map(str::to_string).collect()
}

/// The bytes of the backups beside the canvas file `file` that clears and loads keep:
/// `<file>.bak` and the older ones.
fn backups_of(file: &Path) -> Vec<Vec<u8>> {
    kept_as(file, ".bak")
}

/// The bytes of the backups beside the canvas file `file` that deletions keep:
/// `<file>.del.bak` and the older ones.
fn deletion_backups_of(file: &Path) -> Vec<Vec<u8>> {
    kept_as(file, ".del.bak")
}

/// The bytes of the files beside `file` named `<file><suffix>`, then `<file><suffix>.<n>`.
fn kept_as(file: &Path, suffix: &str) -> Vec<Vec<u8>> {
    let dir = file.parent().unwrap();
    let name = format!("{}{suffix}", file.file_name().unwrap().to_string_lossy());
    let mut kept: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&name)
        })
        .collect();
    kept.sort();
    kept.iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect()
}

/// The texts on every page of the canvas in `bytes`, a backup's say.
fn texts_of(bytes: &[u8]) -> std::collections::HashSet<String> {
    let doc = BoardDoc::from_bytes(bytes).unwrap().doc().unwrap();
    let texts = doc.nodes().filter_map(|node| node.kind.text());
    texts.map(|text| text.characters.clone()).collect()
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
