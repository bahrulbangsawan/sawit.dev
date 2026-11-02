//! Saving the stored canvas: calls take turns across processes, and a save that would drop
//! another's edit, or can't be made, is an error that changes nothing.

use std::os::unix::fs::PermissionsExt;

use sawit_board::store::Stamp;

use super::*;
use crate::app::canvas::CanvasCommandArgs;
use crate::app::canvas::stored::{
    CANVAS_ATTEMPTS, CANVAS_LOCK, edit_stored, headless_turn, lock_stored_canvas, run_and_save,
};
use crate::app::tests::{native, said};

#[tokio::test]
async fn a_canvas_that_can_not_be_saved_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    let file = board_file(&project);
    write_board(&file, 1);
    let before = std::fs::read(&file).unwrap();
    let folder = file.parent().unwrap();
    let writable = |mode| std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode));
    writable(0o555).unwrap();
    if std::fs::write(folder.join("probe"), b"x").is_ok() {
        // Whoever may write to a folder that is read-only (root) can't make a save fail this way.
        writable(0o755).unwrap();
        return;
    }

    let result = server
        .canvas_command_tool(CanvasCommandArgs {
            command: "board.sticky".into(),
            params: Some(json!({"x": 0, "y": 0})),
            file: None,
            project: project_arg(&project),
            work: None,
        })
        .await;
    writable(0o755).unwrap();

    // Not a success with the sticky in the reply: it is not in the file.
    assert_eq!(result.is_error, Some(true), "{}", said(&result));
    assert!(
        said(&result).contains("couldn't be saved"),
        "{}",
        said(&result)
    );
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

/// Plan 7 final review I-1 and T5-5: a canvas file written by another since this call read it
/// (a second agent's `sawit-mcp`, whose turns this process doesn't take) is never saved over,
/// which would drop that edit though its call answered ok. The save is refused in FigCraft's
/// sentence, and the file keeps the other's bytes; read again, the call goes through.
#[test]
fn a_canvas_saved_by_another_since_it_was_read_is_not_saved_over() {
    let dir = tempfile::tempdir().unwrap();
    let file = board_file(&project_in(dir.path()));
    write_board(&file, 1);
    // Read as `headless` reads it: stamped, then opened.
    let read = Stamp::of(&file);
    let mut board = store::open(&file).unwrap();
    // The other process saves its edit in between.
    write_board(&file, 2);
    let theirs = std::fs::read(&file).unwrap();

    let sticky = json!({"op": "command", "command": "board.sticky", "params": {"x": 900, "y": 0}});
    let refused = run_and_save(&mut board, &file, read, &sticky).unwrap_err();
    assert!(
        refused.contains("the file changed on disk since it was opened or saved here"),
        "{refused}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), theirs);

    let read = Stamp::of(&file);
    let mut board = store::open(&file).unwrap();
    run_and_save(&mut board, &file, read, &sticky).unwrap();
    assert_eq!(layers_in(&file).len(), 3);
}

/// MCP-1: two agents' `sawit-mcp` processes edit one canvas while Sawit is closed. Each call's
/// answer must say what happened to its edit: ok when it is on the canvas, an error when it
/// isn't. Their saves raced: a call answered ok while the other's save went over its edit,
/// and an error while its edit was the one on disk. Threads stand in for the processes here,
/// each taking only the turns that hold across processes ([`headless_turn`]).
#[test]
fn agents_in_two_processes_get_true_answers_and_lose_no_edit() {
    let dir = tempfile::tempdir().unwrap();
    let file = board_file(&project_in(dir.path()));
    let lock = dir.path().join("home").join(CANVAS_LOCK);
    let (writers, calls) = (4, 12);
    let answers: Vec<(String, Result<Value, String>)> = std::thread::scope(|scope| {
        let running: Vec<_> = (0..writers)
                .map(|writer| {
                    let (file, lock) = (&file, &lock);
                    scope.spawn(move || {
                        (0..calls)
                            .map(|call| {
                                let text = format!("w{writer}-n{call}");
                                let sticky = json!({"type": "sticky", "x": call * 250, "y": writer * 250, "text": text});
                                let op = json!({"op": "add", "elements": [sticky]});
                                (text, headless_turn(file, lock, &op))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
        running
            .into_iter()
            .flat_map(|writer| writer.join().unwrap())
            .collect()
    });

    let on_canvas = texts_in(&file);
    let mut lies = Vec::new();
    for (text, answer) in &answers {
        match answer {
            Ok(_) if !on_canvas.contains(text) => lies.push(format!("{text}: ok, not saved")),
            Err(e) if on_canvas.contains(text) => lies.push(format!("{text}: saved, yet {e}")),
            _ => {}
        }
    }
    assert!(lies.is_empty(), "{} wrong answers: {lies:#?}", lies.len());
    // Taking turns, no call has anything to refuse.
    let failed: Vec<_> = answers.iter().filter(|(_, a)| a.is_err()).collect();
    assert!(
        failed.is_empty(),
        "{} calls failed: {failed:#?}",
        failed.len()
    );
    assert_eq!(on_canvas.len(), writers * calls);
}

/// MCP-1: something that takes no turns (Sawit as it starts, another program) saves the
/// stored canvas while a call runs. The call's save would drop that edit, so it is refused,
/// and the call runs again on what is on disk now: both edits land, and the answer is ok.
/// When the file changes every time, nothing of the call's is saved, and the error tells the
/// agent what to do, not to close a tab it doesn't have.
#[test]
fn a_call_runs_again_on_a_canvas_saved_under_it_and_says_when_it_gave_up() {
    let dir = tempfile::tempdir().unwrap();
    let file = board_file(&project_in(dir.path()));
    write_board(&file, 1);
    let sticky = |text: &str| json!({"op": "add", "elements": [{"type": "sticky", "x": 600, "y": 0, "text": text}]});
    // Another program's save: the board with one more sticky.
    let save_theirs = |text: &str| {
        let mut board = store::open(&file).unwrap();
        let reply = ops::run(&mut board, &sticky(text));
        assert_eq!(reply["ok"], true, "{reply}");
        std::fs::write(&file, board.to_bytes().unwrap()).unwrap();
    };

    let mut reads = 0;
    let once = edit_stored(&file, &sticky("ours"), || {
        reads += 1;
        if reads == 1 {
            save_theirs("theirs");
        }
    });
    assert!(once.is_ok(), "{once:?}");
    assert_eq!(reads, 2);
    let texts = texts_in(&file);
    assert!(
        texts.contains("ours") && texts.contains("theirs"),
        "{texts:?}"
    );

    let mut reads = 0;
    let every_time = edit_stored(&file, &sticky("never"), || {
        reads += 1;
        save_theirs(&format!("theirs {reads}"));
    });
    let error = every_time.unwrap_err();
    assert_eq!(reads, CANVAS_ATTEMPTS);
    assert!(error.starts_with("Nothing was saved"), "{error}");
    assert!(error.contains("Call it again"), "{error}");
    assert!(!error.contains("tab"), "{error}");
    let texts = texts_in(&file);
    assert!(!texts.contains("never"), "{texts:?}");
    assert!(
        texts.contains(&format!("theirs {CANVAS_ATTEMPTS}")),
        "{texts:?}"
    );
}

/// The canvas lock is let go when a call ends, and a link at its name is never followed.
#[test]
fn the_canvas_lock_is_let_go_and_never_taken_through_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("home").join(CANVAS_LOCK);
    let held = lock_stored_canvas(&lock).unwrap();
    // Held, another open file of it can't have it: as another process couldn't.
    let other = std::fs::File::open(&lock).unwrap();
    assert!(matches!(
        other.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(held);
    other.try_lock().unwrap();
    drop(other);

    std::fs::remove_file(&lock).unwrap();
    let victim = dir.path().join("victim");
    std::fs::write(&victim, "the user's").unwrap();
    std::os::unix::fs::symlink(&victim, &lock).unwrap();
    let refused = lock_stored_canvas(&lock).unwrap_err();
    assert!(refused.contains("it is a link"), "{refused}");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "the user's");
}
