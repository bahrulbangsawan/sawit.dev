//! The rows added for the GitHub CLI, MinIO, Herdr, HyperFrames and voxl: voxl's long build
//! goes to a terminal, Install Missing leaves out what needs a terminal or another Mac, and a
//! Homebrew copy upgrades with its formula.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::dev_tools::find;

use crate::tools_page::{Job, State, ToolsEvent, ToolsPage};

/// voxl builds from source for minutes: with Homebrew and Rust there, its install goes to a
/// terminal, where the build's progress shows, rather than the page's queue.
#[gpui_kit::test]
fn voxls_build_runs_in_a_terminal(cx: &mut TestAppContext) {
    let page = cx.new(|cx| {
        let mut page = ToolsPage::blank(cx);
        page.loading = false;
        page.running = Some(("deno", Job::Install));
        page.states.insert("homebrew", State::Done);
        page.states.insert("rust", State::Done);
        page
    });
    let terminal = Rc::new(RefCell::new(Vec::new()));
    let seen = terminal.clone();
    cx.update(|cx| {
        cx.subscribe(&page, move |_, event: &ToolsEvent, _| {
            if let ToolsEvent::RunInTerminal(command) = event {
                seen.borrow_mut().push(command.clone());
            }
        })
        .detach()
    });
    page.update(cx, |page, cx| page.enqueue(&["voxl"], cx));
    cx.run_until_parked();
    assert_eq!(
        *terminal.borrow(),
        ["brew install x264 opus pkgconf esbuild && cargo install voxl-server"]
    );
    page.read_with(cx, |page, _| {
        assert_eq!(page.states.get("voxl"), Some(&State::InTerminal));
        assert!(page.queue.is_empty());
    });
}

/// Install Missing covers what installs on its own: not Homebrew (a password), voxl (a long
/// build), nor the Herdr app on an Intel Mac.
#[gpui_kit::test]
fn install_missing_leaves_out_terminal_and_unsupported_tools(cx: &mut TestAppContext) {
    let page = cx.new(|cx| {
        let mut page = ToolsPage::blank(cx);
        page.loading = false;
        page
    });
    page.read_with(cx, |page, _| {
        let missing = page.missing();
        assert!(!missing.contains(&"homebrew") && !missing.contains(&"voxl"));
        assert!(missing.contains(&"gh") && missing.contains(&"herdr"));
        let herdr_app = find("herdr-app").unwrap();
        assert_eq!(
            missing.contains(&"herdr-app"),
            herdr_app.unsupported().is_none()
        );
    });
}
