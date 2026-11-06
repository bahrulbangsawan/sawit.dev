//! The browser's tabs: opened, shown, moved and closed, each its own page; the tab on show alone
//! shown; kept between runs.

use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use crate::browser::BrowserTabs;

fn tabs_in<'a>(
    project: &std::path::Path,
    cx: &'a mut TestAppContext,
) -> (Entity<BrowserTabs>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::browser::init(cx);
    });
    let cx = cx.add_empty_window();
    let project = project.to_path_buf();
    let tabs = cx.update(|window, cx| {
        cx.new(|cx| {
            BrowserTabs::new(
                Some(project),
                Some("http://localhost:5173/".into()),
                window,
                cx,
            )
        })
    });
    (tabs, cx)
}

fn urls(tabs: &Entity<BrowserTabs>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        tabs.read(cx)
            .describe(cx)
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["url"].as_str().unwrap().to_string())
            .collect()
    })
}

#[gpui_kit::test]
fn tabs_open_show_move_and_close(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (tabs, cx) = tabs_in(dir.path(), cx);
    cx.update(|window, cx| {
        tabs.update(cx, |t, cx| {
            t.set_on_show(true, cx);
            t.open_tab(Some("http://localhost:3000/".into()), false, window, cx);
            t.open_tab(Some("http://localhost:8080/".into()), true, window, cx);
        })
    });
    assert_eq!(
        urls(&tabs, cx),
        [
            "http://localhost:5173/",
            "http://localhost:3000/",
            "http://localhost:8080/"
        ]
    );
    let shown = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let t = tabs.read(cx);
            (
                t.counts().1,
                t.views()
                    .iter()
                    .map(|v| v.read(cx).on_show())
                    .collect::<Vec<_>>(),
            )
        })
    };
    // A tab opened behind stays behind; one shown is the one on show, alone.
    assert_eq!(shown(cx), (2, vec![false, false, true]));
    cx.update(|_, cx| tabs.update(cx, |t, cx| t.activate(0, cx)));
    assert_eq!(shown(cx), (0, vec![true, false, false]));
    // Found by id, title or address.
    let found = cx.update(|_, cx| tabs.read(cx).find("3000", cx).map(|(ix, _)| ix));
    assert_eq!(found, Some(1));
    // Moved: the tab on show stays on show.
    cx.update(|_, cx| tabs.update(cx, |t, cx| t.move_tab(0, 2, cx)));
    assert_eq!(urls(&tabs, cx)[2], "http://localhost:5173/");
    assert_eq!(shown(cx), (2, vec![false, false, true]));
    // Closed: the one before it shows.
    cx.update(|window, cx| tabs.update(cx, |t, cx| t.close_tab(2, window, cx)));
    assert_eq!(shown(cx), (1, vec![false, true]));
    // The browser off screen shows none.
    cx.update(|_, cx| tabs.update(cx, |t, cx| t.set_on_show(false, cx)));
    assert_eq!(shown(cx).1, vec![false, false]);
}

#[gpui_kit::test]
fn tabs_are_kept_for_next_time(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".nyawit").join("browser-tabs.json");
    {
        let (tabs, cx) = tabs_in(dir.path(), cx);
        cx.update(|window, cx| {
            tabs.update(cx, |t, cx| {
                t.open_tab(Some("http://localhost:3000/".into()), true, window, cx);
            })
        });
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
    }
    let saved = sawit_core::browser::load_tabs(&file);
    assert_eq!(saved.active, 1);
    let urls: Vec<&str> = saved.tabs.iter().map(|t| t.url.as_str()).collect();
    assert_eq!(urls, ["http://localhost:5173/", "http://localhost:3000/"]);
}
