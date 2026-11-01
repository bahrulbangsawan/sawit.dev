//! The Dev Tools page's tests, and the page they start from: Bun installed, another tool's
//! installer "running" so that nothing they queue starts.

mod install;
mod keyboard;
mod lifecycle;
mod uninstall;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use sawit_core::dev_tools::{Found, find};

use super::{Job, ToolsPage};

/// Bun's own removal, as its row's trash button runs it.
fn remove_bun() -> Job {
    Job::Uninstall(r#"rm -rf "$HOME/.bun""#.into())
}

/// A Dev Tools page in a window of its own, with Bun installed at `~/.bun`. The tests make
/// another tool's installer "run" before anything is queued, so nothing they queue starts and
/// no script runs here.
fn page_with_bun(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ToolsPage>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::catalog::init(cx);
    });
    cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| {
                let mut page = ToolsPage::blank(cx);
                page.loading = false;
                page.found.insert(
                    "bun",
                    Found {
                        path: "/Users/me/.bun/bin/bun".into(),
                        version: Some("1.2.0".into()),
                        outside: None,
                    },
                );
                page
            })
        })
    })
    .unwrap()
}

/// Puts ↩'s selection on Bun's row, with Deno's installer "running".
fn select_bun_while_another_runs(page: &mut ToolsPage) {
    page.running = Some(("deno", Job::Install));
    page.cursor = page.shown("").iter().position(|(_, t)| t.id == "bun");
}

fn queued(cx: &mut TestAppContext, page: &Entity<ToolsPage>) -> Vec<(&'static str, Job)> {
    page.read_with(cx, |page, _| {
        page.queue
            .iter()
            .map(|(tool, job)| (tool.id, job.clone()))
            .collect()
    })
}

/// Retry, as the row's button runs it.
fn retry_bun(cx: &mut TestAppContext, window: AnyWindowHandle, page: &Entity<ToolsPage>) {
    let bun = find("bun").unwrap();
    cx.update_window(window, |_, window, cx| {
        page.update(cx, |page, cx| page.retry(bun, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
}
