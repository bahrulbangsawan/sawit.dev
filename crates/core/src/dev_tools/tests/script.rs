//! Script runs: stopped when dropped, any login shell, output that isn't UTF-8, one per tool.

use std::time::Duration;

use crate::dev_tools::detect::detect_as;
use crate::dev_tools::script::{run_locked, run_script, run_script_as, strip_ansi};

#[tokio::test]
async fn dropping_a_run_stops_everything_it_started() {
    let marker = format!("sleep 31.{}", std::process::id());
    let script = format!("{marker} & {marker}");
    let alive = || {
        std::process::Command::new("/usr/bin/pgrep")
            .args(["-f", &marker])
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let run = run_script("test", &script, |_| {});
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), run)
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!alive(), "the script's processes outlived the run");
}

/// fish, nu, PowerShell and tcsh can't run the scripts: they run in zsh.
/// tcsh ships with macOS, so it stands in for them here.
#[tokio::test]
async fn scripts_run_whatever_the_login_shell() {
    let lines = std::sync::Mutex::new(Vec::new());
    run_script_as(Some("/bin/tcsh"), "test", "echo \"${NVM_DIR:+nvm}\"", |l| {
        lines.lock().unwrap().push(l)
    })
    .await
    .unwrap();
    assert_eq!(*lines.lock().unwrap(), ["nvm"]);
    let found = detect_as(Some("/bin/tcsh")).await;
    assert_eq!(found.problem, None);
}

/// An installer that prints a line in Latin-1 goes on being read: the
/// rest of its output still shows, and its pipe isn't closed under it.
#[tokio::test]
async fn output_after_a_line_that_isnt_utf8_still_comes_through() {
    let lines = std::sync::Mutex::new(Vec::new());
    run_script("test", r"printf 'caf\351\n'; echo after", |l| {
        lines.lock().unwrap().push(l)
    })
    .await
    .unwrap();
    assert_eq!(*lines.lock().unwrap(), ["caf\u{FFFD}", "after"]);
}

/// Two runs for one tool (the page, an agent, `sawit pasang`) never
/// overlap: the second says the first is running, and names every place
/// a run can come from.
#[tokio::test]
async fn one_run_per_tool_at_a_time() {
    let id = format!("test-{}", std::process::id());
    let first = run_locked(&id, "Test", "sleep 2", |_| {});
    let second = async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        run_locked(&id, "Test", "true", |_| {}).await
    };
    let (first, second) = tokio::join!(first, second);
    first.unwrap();
    let err = second.expect_err("ran alongside the first");
    assert!(err.to_string().contains("already"), "{err}");
    for word in ["sawit pasang", "sawit pupuk", "sawit cabut"] {
        assert!(err.to_string().contains(word), "{err}");
    }
    // Once the first is done, the next runs. A process another test forked while the lock
    // was held has the lock's file too until it execs, and so holds the lock until then;
    // under load that can outlast the first run by a moment. So the next run may say
    // "already" for a few seconds, but no longer.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match run_locked(&id, "Test", "true", |_| {}).await {
            Ok(()) => break,
            Err(e) if e.to_string().contains("already") && std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => panic!("the next run didn't run once the first was done: {e}"),
        }
    }
}

#[test]
fn ansi_is_stripped() {
    assert_eq!(strip_ansi("\u{1b}[1;32mok\u{1b}[0m done"), "ok done");
}
