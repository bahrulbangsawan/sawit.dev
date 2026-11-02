//! Panels, picking and Sawit's own keys.

use serde_json::json;

use super::test_browser;
use crate::browser;
use crate::browser::design::Segment;
use crate::browser::picking::{pick_mode, rearm_ops};
use crate::browser::{Panel, PickMode};

#[test]
fn ops_after_load_rearms_pick() {
    assert_eq!(rearm_ops(PickMode::Off), Vec::<serde_json::Value>::new());
    assert_eq!(
        rearm_ops(PickMode::Annotate),
        vec![json!({"op": "pick", "mode": "annotate"})]
    );
    assert_eq!(
        rearm_ops(PickMode::Inspect),
        vec![json!({"op": "pick", "mode": "inspect"})]
    );
}

/// A page script can arm the picker and click for itself (`__sawitAgent` is the page's to
/// call): a pick adds an annotation only while the user has Annotate open and picking.
#[gpui_kit::test]
fn picks_count_only_while_annotating(cx: &mut gpui_kit::TestAppContext) {
    use sawit_core::browser::PageEvent;
    let (view, cx) = test_browser(cx);
    let pick = serde_json::json!({"mode": "annotate", "element": {
            "selector": "#buy", "label": "button#buy", "tag": "button",
            "rect": {"x": 1, "y": 1, "width": 40, "height": 20}}});
    let picked = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            view.update(cx, |b, cx| {
                b.receive(
                    browser::FromPage::Event(PageEvent::Picked(pick.clone())),
                    window,
                    cx,
                )
            })
        });
        cx.run_until_parked();
    };
    let rows = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| view.read(cx).annotate.read(cx).annotations(cx).len())
    };
    picked(cx);
    assert_eq!(rows(cx), 0, "Annotate is closed");

    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(browser::Panel::Annotate, cx)));
    picked(cx);
    assert_eq!(rows(cx), 1, "the user's pick");

    // Esc stopped picking; the panel is still open.
    cx.update(|_, cx| view.update(cx, |b, cx| b.set_pick(browser::PickMode::Off, cx)));
    picked(cx);
    assert_eq!(rows(cx), 1, "picking is off");

    // Design's Inspect picks are no annotations either.
    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(browser::Panel::Design, cx)));
    picked(cx);
    assert_eq!(rows(cx), 1, "Design is open");
}

/// The Panel op opens Annotate or Design on a tab, closes the panel, and names what it
/// doesn't know.
#[gpui_kit::test]
fn the_panel_op_opens_panels_and_tabs(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let op = |panel: Option<&str>, tab: Option<&str>, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| view.update(cx, |b, cx| b.panel_op(panel, tab, cx)))
    };
    assert_eq!(
        op(Some("design"), Some("fonts"), cx).unwrap(),
        json!({"panel": "design", "tab": "fonts"})
    );
    assert_eq!(
        op(Some("design"), Some("inspect"), cx).unwrap(),
        json!({"panel": "design", "tab": "inspect"})
    );
    assert_eq!(
        cx.update(|_, cx| view.read(cx).pick),
        browser::PickMode::Inspect
    );
    assert_eq!(
        op(Some("annotate"), None, cx).unwrap(),
        json!({"panel": "annotate", "tab": null})
    );
    assert_eq!(
        op(None, None, cx).unwrap(),
        json!({"panel": null, "tab": null})
    );
    assert!(
        op(Some("layers"), None, cx)
            .unwrap_err()
            .contains("Unknown panel")
    );
    assert!(
        op(Some("design"), Some("x"), cx)
            .unwrap_err()
            .contains("Unknown tab")
    );
}

/// The page answers no one's ⇧⌘. (a page can dispatch one, and the user's is the keymap's),
/// so an agent's `browser_press_key "Meta+Shift+."` is Sawit's to answer: it toggles
/// Annotate as the user's key does, before the page has loaded too. Esc while picking stops picking as
/// the user's does; other keys, and Esc while not picking, go to the page.
#[gpui_kit::test]
fn an_agents_shift_cmd_period_is_sawits_to_handle(cx: &mut gpui_kit::TestAppContext) {
    use sawit_core::browser::BrowserOp;
    let (view, cx) = test_browser(cx);
    let press = |key: &str, cx: &mut gpui_kit::VisualTestContext| {
        let mut answer = cx.update(|_, cx| {
            view.update(cx, |b, cx| {
                b.apply(BrowserOp::Press { key: key.into() }, cx)
            })
        });
        cx.run_until_parked();
        answer.try_recv().ok().flatten()
    };
    let state = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| {
            let b = view.read(cx);
            (b.panel(), b.pick)
        })
    };

    let answer = press("Meta+Shift+.", cx);
    assert_eq!(state(cx), (Some(Panel::Annotate), PickMode::Annotate));
    assert_eq!(
        answer,
        Some(Ok(
            json!({"pressed": "Meta+Shift+.", "panel": "annotate", "picking": "annotate"})
        ))
    );
    press("Escape", cx);
    assert_eq!(
        state(cx),
        (Some(Panel::Annotate), PickMode::Off),
        "Esc stops picking"
    );
    press("Cmd+Shift+.", cx);
    assert_eq!(
        state(cx),
        (Some(Panel::Annotate), PickMode::Annotate),
        "picks again"
    );
    press("shift+meta+>", cx);
    assert_eq!(state(cx), (None, PickMode::Off), "and closes");
    // The page's: waits for it (this tab never loads one).
    assert_eq!(press("Escape", cx), None);
    assert_eq!(press("Meta+.", cx), None);
    assert_eq!(state(cx), (None, PickMode::Off));
}

#[test]
fn sawits_own_keys() {
    use browser::picking::{OwnKey, own_key};
    for key in [
        "Meta+Shift+.",
        "Shift+Meta+.",
        "cmd+shift+.",
        "Command+Shift+>",
        "Meta+>",
        " Meta + Shift + . ",
    ] {
        assert_eq!(own_key(key), Some(OwnKey::ToggleAnnotate), "{key}");
    }
    for key in ["Escape", "esc", "Shift+Escape"] {
        assert_eq!(own_key(key), Some(OwnKey::Escape), "{key}");
    }
    for key in [
        ".",
        "Meta+.",
        "Shift+.",
        "Ctrl+Meta+Shift+.",
        "Meta+Alt+Shift+.",
        "Meta+Shift+,",
        "Meta+Shift+a",
        "Enter",
        "",
    ] {
        assert_eq!(own_key(key), None, "{key}");
    }
}

/// The overlay is drawn in Sawit's colours: every one a CSS colour, the system face as CSS
/// names it.
#[gpui_kit::test]
fn the_overlay_takes_the_apps_colours(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let op = cx.update(|cx| browser::picking::theme_op(cx));
    assert_eq!(op["op"], "theme");
    for name in [
        "bg",
        "fg",
        "muted",
        "border",
        "field",
        "accent",
        "primary",
        "primaryFg",
        "danger",
        "ring",
    ] {
        let v = op["colors"][name].as_str().unwrap();
        assert!(
            v.starts_with('#') && (v.len() == 7 || v.len() == 9),
            "{name}: {v}"
        );
    }
    let font = op["colors"]["font"].as_str().unwrap();
    assert!(!font.starts_with('.'), "{font}");
    assert!(font.ends_with("sans-serif"), "{font}");
}

#[test]
fn design_picks_only_on_inspect() {
    assert_eq!(
        pick_mode(Panel::Design, Segment::Inspect),
        PickMode::Inspect
    );
    for s in [
        Segment::Colors,
        Segment::Fonts,
        Segment::Assets,
        Segment::System,
    ] {
        assert_eq!(pick_mode(Panel::Design, s), PickMode::Off, "{s:?}");
        assert_eq!(pick_mode(Panel::Annotate, s), PickMode::Annotate);
    }
    assert_eq!(
        pick_mode(Panel::Annotate, Segment::Inspect),
        PickMode::Annotate
    );
}

#[test]
fn pick_modes_are_named() {
    assert_eq!(PickMode::Off.as_str(), "off");
    assert_eq!(PickMode::Annotate.as_str(), "annotate");
    assert_eq!(PickMode::Inspect.as_str(), "inspect");
}
