//! Restart's tests: it waits for the old run to stop, however long that takes, and a Stop calls it
//! off.

use gpui_kit::{AppContext as _, TestAppContext};

use std::collections::HashSet;

use super::{Duration, RESTART_POLL, once_stopped, restart_once_stopped};

/// A script's terminal, as a restart sees it.
struct Job {
    busy: bool,
    started: u32,
}

/// A row of the Run panel, as a restart and a Stop see it.
struct Row {
    busy: bool,
    started: u32,
    restarting: HashSet<String>,
}

impl Row {
    /// Restart: the row waits to start again once its old run has stopped.
    fn restart(&mut self, cx: &mut gpui_kit::Context<Self>) {
        self.restarting.insert("k".into());
        restart_once_stopped(
            "k".into(),
            |row: &mut Self| &mut row.restarting,
            |row, _| row.busy,
            |row, _| row.started += 1,
            cx,
        )
        .detach();
    }
}

/// Stop while a restart waits for the old run to stop calls the restart off: Stop means
/// stop. It only interrupted the old run again, so once that run stopped, the script started
/// again although the user had pressed Stop. A restart left alone still starts.
#[gpui_kit::test]
fn a_stop_while_a_restart_waits_calls_it_off(cx: &mut TestAppContext) {
    let row = cx.new(|_| Row {
        busy: true,
        started: 0,
        restarting: HashSet::new(),
    });
    row.update(cx, |row, cx| row.restart(cx));
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    // Stop, as the Run panel's Stop does it, while the old run is still stopping.
    row.update(cx, |row, _| row.restarting.remove("k"));
    row.update(cx, |row, _| row.busy = false);
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(
        row.read_with(cx, |row, _| row.started),
        0,
        "it started again after Stop"
    );

    row.update(cx, |row, cx| {
        row.busy = true;
        row.restart(cx);
    });
    row.update(cx, |row, _| row.busy = false);
    cx.executor().advance_clock(RESTART_POLL);
    cx.run_until_parked();
    assert_eq!(row.read_with(cx, |row, _| row.started), 1);
    assert!(row.read_with(cx, |row, _| row.restarting.is_empty()));
}

/// A restart starts the script again once its old run has stopped, however long the
/// shutdown takes (a dev server's hooks, `docker compose` stopping its containers), and
/// never while the old run still runs: the new one would only show the old one's terminal.
#[gpui_kit::test]
fn a_restart_waits_for_the_old_run_however_long_it_takes(cx: &mut TestAppContext) {
    let job = cx.new(|_| Job {
        busy: true,
        started: 0,
    });
    job.update(cx, |_, cx| {
        once_stopped(|j: &Job, _| j.busy, |j, _| j.started += 1, cx).detach()
    });
    for _ in 0..60 {
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }
    assert_eq!(
        job.read_with(cx, |j, _| j.started),
        0,
        "it started again while the old run still ran"
    );
    job.update(cx, |j, _| j.busy = false);
    cx.executor().advance_clock(RESTART_POLL);
    cx.run_until_parked();
    assert_eq!(job.read_with(cx, |j, _| j.started), 1);
}
