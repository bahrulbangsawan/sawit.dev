//! Closing the page and quitting stop the installer it runs, and quitting waits until the
//! installer was told to stop.

use futures::FutureExt as _;
use futures::future::{Aborted, abortable};
use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::dev_tools::find;

use crate::tools_page::{Job, ToolsPage};

/// Closing the Dev Tools tab drops the page: the installer it runs stops with it, instead
/// of running on unseen, its queue gone, while a reopened page offers to start it again.
#[gpui_kit::test]
fn closing_the_page_stops_its_installer(cx: &mut TestAppContext) {
    let (run, stop) = abortable(futures::future::pending::<()>());
    let page = cx.new(|cx| {
        let mut page = ToolsPage::blank(cx);
        page.running = Some(("bun", Job::Install));
        page.queue.push_back((find("deno").unwrap(), Job::Install));
        page.stop = Some(stop);
        page
    });
    assert_eq!(
        page.read_with(cx, |page, _| page.work()).as_deref(),
        Some("Installing Bun, 1 more queued")
    );
    drop(page);
    // The app lets go of a dropped page at the end of its next update.
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(run.now_or_never(), Some(Err(Aborted)), "still running");
}

/// Final seams m4: quitting stops the installer the page runs, as closing it does. Only the
/// page's drop stopped it, and quitting dropped it (with the windows) too late or not at all:
/// the installer, in a session of its own, ran on with nobody to show its end.
#[gpui_kit::test]
fn quitting_stops_the_installer(cx: &mut TestAppContext) {
    let (run, stop) = abortable(futures::future::pending::<()>());
    // Held here, so only quitting can stop it.
    let page = cx.new(|cx| {
        let mut page = ToolsPage::blank(cx);
        page.running = Some(("bun", Job::Install));
        page.stop = Some(stop);
        page
    });
    cx.quit();
    assert_eq!(run.now_or_never(), Some(Err(Aborted)), "still running");
    drop(page);
}

/// The app's quit waits (as long as it waits for quit handlers) until the stopped run is
/// over, its process group told to end, so the installer can't outlive the app.
#[gpui_kit::test]
fn quitting_waits_until_the_installer_was_told_to_stop(cx: &mut TestAppContext) {
    let (ended, ended_rx) = futures::channel::oneshot::channel::<()>();
    let page = cx.new(|cx| {
        let mut page = ToolsPage::blank(cx);
        page.running = Some(("bun", Job::Install));
        page.ended = Some(ended_rx);
        page
    });
    let mut quit = page
        .update(cx, |page, _| page.stop_for_quit())
        .boxed_local();
    assert_eq!((&mut quit).now_or_never(), None, "it didn't wait");
    drop(ended);
    assert_eq!(quit.now_or_never(), Some(()));
}
