//! The list from the keyboard: the search, ↩ there, ↑ and ↓, ↩ on a row, and ⌘F.

use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::dev_tools::find;

use crate::tools_page::{Job, State, ToolsPage};

/// The list works from the keyboard: the search has it when the page opens, ↩ there moves
/// it to the first row, ↑ and ↓ select, ↩ runs the row's main action (here: taking a tool
/// out of the queue, which runs nothing), and ⌘F goes back to the search.
#[gpui_kit::test]
fn the_list_works_from_the_keyboard(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::catalog::init(cx);
    });
    let (window, page) = cx
        .update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |_, cx| {
                cx.new(|cx| {
                    let mut page = ToolsPage::blank(cx);
                    page.loading = false;
                    page.states.insert("nvm", State::Queued { for_: None });
                    page.queue.push_back((find("nvm").unwrap(), Job::Install));
                    page
                })
            })
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        page.update(cx, |page, cx| page.focus_search(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    // nvm, and Node.js, whose description names it.
    cx.simulate_keystrokes(window, "n v m");
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    let state = |cx: &mut TestAppContext| {
        page.read_with(cx, |page, _| (page.cursor, page.states.get("nvm").cloned()))
    };
    let queued = Some(State::Queued { for_: None });
    assert_eq!(state(cx), (Some(0), queued.clone()), "↩ in the search");
    cx.simulate_keystrokes(window, "down");
    assert_eq!(state(cx), (Some(1), queued.clone()));
    cx.simulate_keystrokes(window, "down up");
    assert_eq!(state(cx), (Some(0), queued));
    cx.simulate_keystrokes(window, "enter");
    assert_eq!(
        state(cx),
        (Some(0), None),
        "↩ didn't take it out of the queue"
    );
    cx.simulate_keystrokes(window, "cmd-f");
    cx.run_until_parked();
    let search = page.read_with(cx, |page, cx| {
        page.search.as_ref().unwrap().focus_handle(cx)
    });
    cx.update_window(window, |_, window, _| {
        assert!(search.is_focused(window), "⌘F")
    })
    .unwrap();
}
