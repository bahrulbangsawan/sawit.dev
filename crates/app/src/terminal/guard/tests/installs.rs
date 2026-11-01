//! Closing a window or quitting while its Dev Tools page installs something.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{AppContext as _, TestAppContext, VisualTestContext};

use super::super::ask_before_closing;
use super::{start_sleep, try_quit};
use crate::terminal::testing::{init, open_panel, ready};
use crate::tools_page::ToolsPage;

/// Final seams m4 (catalog M5): closing a window while its Dev Tools page installs asks
/// first, as closing that tab does: the page goes with the window, and the installer with
/// it. The window closed at once. Another window closes without asking. Quitting asks the
/// same way, in one question with what the terminals run; Cancel keeps everything, and the
/// answer goes on.
#[gpui_kit::test]
fn closing_or_quitting_asks_while_dev_tools_installs(cx: &mut TestAppContext) {
    cx.update(init);
    let (other, other_panel) = open_panel(cx);
    let (w, panel) = open_panel(cx);
    for (window, panel) in [(other, &other_panel), (w, &panel)] {
        cx.update_window(window, |_, window, cx| {
            ask_before_closing(window, panel.downgrade(), cx)
        })
        .expect("the test window is open");
        let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &shell);
    }
    let page = cx
        .update_window(w, |_, window, cx| {
            cx.new(|cx| ToolsPage::installing_in("bun", window, cx))
        })
        .expect("the test window is open");

    let mut vcx = VisualTestContext::from_window(other, cx);
    assert!(
        vcx.simulate_close(),
        "another window's install held this one"
    );
    assert!(!cx.has_pending_prompt());

    let mut vcx = VisualTestContext::from_window(w, cx);
    assert!(!vcx.simulate_close(), "the window closed without asking");
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Close this window?".to_string(),
            "Dev Tools is still running: Installing Bun. Closing the window stops it.".to_string()
        ))
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(cx.windows().contains(&w), "Cancel closed the window");

    start_sleep(cx, &panel);
    let quits = Rc::new(Cell::new(0));
    try_quit(cx, w, &panel, &quits);
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Quit Sawit?".to_string(),
            "A terminal is still running: sleep. Dev Tools is still running: Installing Bun. \
                 Quitting stops them."
                .to_string()
        ))
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(quits.get(), 0, "Cancel quit");
    try_quit(cx, w, &panel, &quits);
    cx.simulate_prompt_answer("Quit");
    cx.run_until_parked();
    assert_eq!(quits.get(), 1);
    drop(page);
}
