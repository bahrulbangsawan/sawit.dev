//! Uninstalling: Retry and ↩ never install over the copy the user meant to remove, and the
//! warning before removing what other tools need.

use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::dev_tools::{Detection, Found, find};

use super::{page_with_bun, queued, remove_bun, retry_bun, select_bun_while_another_runs};
use crate::tools_page::{State, ToolsPage};

/// CATALOG M4: after an uninstall fails, the row says Failed, and neither its Retry nor ↩
/// may run the tool's installer, a `curl | sh` over the copy the user tried to remove (both
/// queued an install). Retry asks again, as the trash button does, then runs the uninstall
/// that failed; ↩ leaves the row alone, since one key never removes anything.
#[gpui_kit::test]
fn retrying_a_failed_uninstall_never_runs_the_installer(cx: &mut TestAppContext) {
    let (window, page) = page_with_bun(cx);
    page.update(cx, |page, cx| {
        page.running = Some(("bun", remove_bun()));
        page.finish(
            "bun",
            remove_bun(),
            Some(Err("Permission denied".into())),
            cx,
        );
        select_bun_while_another_runs(page);
        page.confirm(cx);
    });
    assert_eq!(queued(cx, &page), [], "↩ after the uninstall failed");

    retry_bun(cx, window, &page);
    assert_eq!(queued(cx, &page), [], "Retry ran something without asking");
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Uninstall Bun?".to_string(),
            r#"Runs: rm -rf "$HOME/.bun""#.to_string()
        )),
        "Retry didn't ask to uninstall again"
    );
    cx.simulate_prompt_answer("Uninstall");
    cx.run_until_parked();
    assert_eq!(queued(cx, &page), [("bun", remove_bun())]);
}

/// An uninstall that worked, after which detection still finds a copy installed another way
/// (Homebrew's): the row says so, as Failed, and has nothing to run again. Retry and ↩ used
/// to queue the installer over that copy.
#[gpui_kit::test]
fn a_copy_found_after_its_uninstall_is_never_installed_over(cx: &mut TestAppContext) {
    let (window, page) = page_with_bun(cx);
    page.update(cx, |page, cx| {
        page.running = Some(("bun", remove_bun()));
        page.finish("bun", remove_bun(), Some(Ok(())), cx);
        let homebrew = Found {
            path: "/opt/homebrew/bin/bun".into(),
            version: Some("1.2.0".into()),
            outside: None,
        };
        page.detected(
            Detection {
                found: [("bun", homebrew)].into(),
                problem: None,
            },
            cx,
        );
        assert!(
            matches!(page.states.get("bun"), Some(State::Failed(..))),
            "{:?}",
            page.states.get("bun")
        );
        select_bun_while_another_runs(page);
        page.confirm(cx);
    });
    assert_eq!(queued(cx, &page), [], "↩ on the copy still found");
    retry_bun(cx, window, &page);
    assert!(!cx.has_pending_prompt());
    assert_eq!(queued(cx, &page), [], "Retry on the copy still found");
    // Once that copy is gone too, the row is a missing tool's again, with Install.
    page.update(cx, |page, cx| {
        page.detected(
            Detection {
                found: Default::default(),
                problem: None,
            },
            cx,
        )
    });
    assert_eq!(
        page.read_with(cx, |page, _| page.states.get("bun").cloned()),
        None
    );
}

/// Removing Homebrew warns that its cask's app won't get updates through Homebrew any more,
/// not that it stops working; a copy from the disk image isn't named.
#[gpui_kit::test]
fn removing_homebrew_says_puremac_only_stops_getting_updates(cx: &mut TestAppContext) {
    use sawit_core::dev_tools::Outside;
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::catalog::init(cx);
    });
    let at = |path: &str, outside| Found {
        path: path.into(),
        version: None,
        outside,
    };
    let (window, page) = cx
        .update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |_, cx| {
                cx.new(|cx| {
                    let mut page = ToolsPage::blank(cx);
                    page.loading = false;
                    page.found = [
                        ("homebrew", at("/opt/homebrew/bin/brew", None)),
                        ("puremac", at("/Applications/PureMac.app", None)),
                    ]
                    .into();
                    page
                })
            })
        })
        .unwrap();
    let homebrew = find("homebrew").unwrap();
    let ask = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            page.update(cx, |page, cx| {
                page.ask_uninstall(homebrew, "uninstall.sh".into(), window, cx)
            })
        })
        .unwrap();
        cx.run_until_parked();
        let prompt = cx.pending_prompt();
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        prompt
    };
    assert_eq!(
        ask(cx),
        Some((
            "Uninstall Homebrew?".to_string(),
            "PureMac won't get updates through Homebrew any more.\n\nRuns: uninstall.sh"
                .to_string()
        ))
    );
    page.update(cx, |page, _| {
        page.found.get_mut("puremac").unwrap().outside = Some(Outside::Homebrew)
    });
    assert_eq!(
        ask(cx),
        Some((
            "Uninstall Homebrew?".to_string(),
            "Runs: uninstall.sh".to_string()
        ))
    );
    assert!(page.read_with(cx, |page, _| page.queue.is_empty()));
}
