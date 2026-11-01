//! The keys the chrome's tooltips, menus and sheet show, as the keymap binds them now.

use super::*;

/// The Maximize button's tooltip says the workspace's key for it as the keymap is now: ⇧⌘↩ by
/// default, the key the user gives it in the keymap editor instead, and none once they take
/// that one out too.
#[gpui_kit::test]
fn the_maximize_tooltip_follows_the_keymap(cx: &mut TestAppContext) {
    let tmp = Scratch::new("maximize-tooltip");
    cx.update(bind_app_keys);
    cx.update(|cx| crate::keymap::init_at(tmp.0.join("keymap.json"), cx));
    let cx = cx.add_empty_window();
    let tooltip =
        |cx: &mut VisualTestContext| cx.update(|window, _| top_bar::maximize_tooltip(window));
    // The row of the keymap that binds Maximize now.
    let bound = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let rows = crate::keymap::store(cx).read(cx).rows(false, cx);
            let row = rows.into_iter().find(|row| {
                row.action.as_ref() == "workspace::ToggleMaximizePane"
                    && !matches!(row.source, crate::keymap::Source::Removed { .. })
            });
            row.unwrap().source
        })
    };
    assert_eq!(tooltip(cx), "Maximize  ⇧⌘↩");

    let default = bound(cx);
    let maximize = "workspace::ToggleMaximizePane";
    let rebound = cx.update(|_, cx| {
        crate::keymap::store(cx).update(cx, |store, cx| {
            store.change(default, Some("Workspace"), "ctrl-alt-m", maximize, None, cx)
        })
    });
    assert_eq!(rebound, Ok(()));
    assert_eq!(tooltip(cx), "Maximize  ⌃⌥M");

    let users = bound(cx);
    let removed =
        cx.update(|_, cx| crate::keymap::store(cx).update(cx, |store, cx| store.remove(users, cx)));
    assert_eq!(removed, Ok(()));
    assert_eq!(tooltip(cx), "Maximize");
}

/// The canvas's hints read the keymap: a command's keys as FigCraft writes them while they
/// still run it, the key the user gives it instead, and none once that is taken out too.
#[gpui_kit::test]
fn the_canvas_hints_follow_the_keymap(cx: &mut TestAppContext) {
    let tmp = Scratch::new("canvas-hints");
    cx.update(bind_app_keys);
    cx.update(|cx| crate::keymap::init_at(tmp.0.join("keymap.json"), cx));
    let default_of = |id: &str, keys: &str, cx: &App| {
        let rows = crate::keymap::store(cx).read(cx).rows(false, cx);
        rows.into_iter()
            .find(|row| {
                row.args
                    .as_deref()
                    .is_some_and(|a| a.contains(&format!("\"{id}\"")))
                    && row.keystrokes.as_ref() == keys
                    && matches!(row.source, crate::keymap::Source::Default(_))
            })
            .unwrap_or_else(|| panic!("the default {keys} of {id}"))
            .source
    };
    cx.update(|cx| {
        // The defaults read as FigCraft writes them, as they did before any rebinding.
        for id in [
            "tool.rectangle",
            "app.actions",
            "view.zoomIn",
            "view.zoomOut",
            "view.zoomFit",
            "help.shortcuts",
            "view.toggleUi",
            "view.layers",
            "view.design",
            "align.left",
            "text.bold",
        ] {
            assert_eq!(
                super::super::command_keys(id, cx),
                sawit_board::menu::shortcut_label(id),
                "{id}"
            );
        }
        let store = crate::keymap::store(cx);
        // R moves to ⌃⌥Y (a key no default uses).
        let rectangle = default_of("tool.rectangle", "r", cx);
        store
            .update(cx, |store, cx| {
                store.change(
                    rectangle,
                    Some(shortcuts::CANVAS_CONTEXT),
                    "ctrl-alt-y",
                    "board::Command",
                    Some(json!({ "id": "tool.rectangle" })),
                    cx,
                )
            })
            .unwrap();
        assert_eq!(
            super::super::command_keys("tool.rectangle", cx).as_deref(),
            Some("⌃⌥Y")
        );
        // ⌘K is taken out of the palette's keys: its other key, ⌘/, still opens it.
        let palette = default_of("app.actions", "cmd-k", cx);
        store
            .update(cx, |store, cx| store.remove(palette, cx))
            .unwrap();
        assert_eq!(
            super::super::command_keys("app.actions", cx).as_deref(),
            Some("⌘/")
        );
        // The sheet and the palette say the same.
        let sheet = menus::live_sheet(shortcuts::sheet(), cx);
        let sheet_keys = |id: &str| {
            sheet
                .iter()
                .flat_map(|group| &group.rows)
                .find(|row| row.id == id)
                .map(|row| row.keys.clone())
        };
        assert_eq!(sheet_keys("tool.rectangle").as_deref(), Some("⌃⌥Y"));
        assert_eq!(sheet_keys("app.actions").as_deref(), Some("⌘/"));
        let mut items = vec![sawit_board::actions::ActionItem {
            id: "tool.rectangle",
            label: "Rectangle".into(),
            path: "Rectangle".into(),
            shortcut: Some("R".into()),
        }];
        menus::live_actions(&mut items, cx);
        assert_eq!(items[0].shortcut.as_deref(), Some("⌃⌥Y"));
        // Taken out too, the rectangle has no key, and leaves the sheet.
        let rows = store.read(cx).rows(false, cx);
        let users = rows
            .iter()
            .find(|row| row.keystrokes.as_ref() == "ctrl-alt-y")
            .unwrap()
            .source;
        store
            .update(cx, |store, cx| store.remove(users, cx))
            .unwrap();
        assert_eq!(super::super::command_keys("tool.rectangle", cx), None);
        let sheet = menus::live_sheet(shortcuts::sheet(), cx);
        assert!(
            !sheet
                .iter()
                .flat_map(|group| &group.rows)
                .any(|row| row.id == "tool.rectangle")
        );
    });
}
