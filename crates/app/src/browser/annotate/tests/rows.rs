//! The rows: Clear and Undo, the page's popover in step with the panel, and cards that fold.

use sawit_core::annotations::Intent;

use super::{at, panel};
use crate::browser::annotate::AnnotatePanel;

/// Clear asks before it drops notes that weren't copied, sent or read by an agent, and
/// before several; a lone note already sent goes at once, with Undo.
#[gpui_kit::test]
fn clear_asks_before_dropping_notes_not_sent(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::VisualTestContext;
    let (panel, cx) = panel(cx);
    let rows = |cx: &mut VisualTestContext| cx.update(|_, cx| panel.read(cx).annotations(cx).len());
    let add = |note: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            panel.update(cx, |p, cx| p.add(at("http://a/1", 1), false, window, cx));
            let comment = panel.read(cx).rows.last().unwrap().comment.clone();
            comment.update(cx, |c, cx| c.set_value(note.to_string(), window, cx));
        });
    };
    let clear = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| panel.update(cx, |p, cx| p.request_clear(window, cx)));
        cx.run_until_parked();
    };

    add("Make it blue", cx);
    assert_eq!(cx.update(|_, cx| panel.read(cx).unsent(cx)), 1);
    clear(cx);
    assert!(cx.has_pending_prompt(), "a note would be lost");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(rows(cx), 1);
    clear(cx);
    cx.simulate_prompt_answer("Clear annotation");
    cx.run_until_parked();
    assert_eq!(rows(cx), 0);

    add("Make it blue", cx);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.mark_sent(cx)));
    assert_eq!(cx.update(|_, cx| panel.read(cx).unsent(cx)), 0);
    // A note edited after it was sent is unsent again.
    cx.update(|window, cx| {
        let comment = panel.read(cx).rows[0].comment.clone();
        comment.update(cx, |c, cx| {
            c.set_value("Make it red".to_string(), window, cx)
        });
    });
    assert_eq!(cx.update(|_, cx| panel.read(cx).unsent(cx)), 1);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.mark_sent(cx)));
    clear(cx);
    assert!(!cx.has_pending_prompt(), "one note, and it was sent");
    assert_eq!(rows(cx), 0);
    // Undo brings it back.
    cx.update(|_, cx| panel.update(cx, |p, cx| p.undo(cx)));
    assert_eq!(rows(cx), 1);

    // Several ask first, sent or not.
    add("Make it green", cx);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.mark_sent(cx)));
    clear(cx);
    assert!(cx.has_pending_prompt(), "two notes go at once");
    cx.simulate_prompt_answer("Clear 2 annotations");
    cx.run_until_parked();
    assert_eq!(rows(cx), 0);
}

/// What the popover saves and removes on the page reaches the row with its number, on its
/// page only; a delete can be undone into the same place.
#[gpui_kit::test]
fn the_page_and_the_panel_keep_in_step(cx: &mut gpui_kit::TestAppContext) {
    let (panel, cx) = panel(cx);
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.add(at("http://a/1", 1), false, window, cx);
            p.add(at("http://a/1", 2), false, window, cx);
            p.add(at("http://a/2", 1), false, window, cx);
        })
    });
    let updated = cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.update_from_page("http://a/1#x", 2, "Bigger", Intent::Change, window, cx)
        })
    });
    assert!(updated);
    let markers = cx.update(|_, cx| panel.read(cx).markers_for("http://a/1", cx));
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[1].comment, "Bigger");
    assert_eq!(markers[1].intent, Intent::Change);
    // No note 3 on that page.
    let missing = cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.update_from_page("http://a/1", 3, "x", Intent::Fix, window, cx)
        })
    });
    assert!(!missing);

    let removed =
        cx.update(|_, cx| panel.update(cx, |p, cx| p.remove_from_page("http://a/1", 1, cx)));
    assert!(removed);
    let left: Vec<(String, u32)> = cx.update(|_, cx| {
        panel
            .read(cx)
            .annotations(cx)
            .into_iter()
            .map(|a| (a.url, a.n))
            .collect()
    });
    assert_eq!(left, [("http://a/1".into(), 2), ("http://a/2".into(), 1)]);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.undo(cx)));
    let back: Vec<u32> =
        cx.update(|_, cx| panel.read(cx).annotations(cx).iter().map(|a| a.n).collect());
    assert_eq!(back, [1, 2, 1]);
}

/// A card folds and opens from its head, by click or key; Collapse all folds the page's
/// cards and Expand all opens them. A new note opens; past three, the others fold. Each
/// card keeps its state through a delete and its undo.
#[gpui_kit::test]
fn cards_fold_and_open(cx: &mut gpui_kit::TestAppContext) {
    use crate::browser::panel::PANEL_WIDTH;
    use gpui_kit::{VisualTestContext, px, size};
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(PANEL_WIDTH), px(900.)), |window, cx| {
        let mut panel = AnnotatePanel::new(window, cx);
        panel.add(at("http://a/1", 1), false, window, cx);
        panel.add(at("http://a/1", 2), false, window, cx);
        panel
    });
    let panel = window.root(cx).unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    let folded = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.collapsed());
    assert_eq!(folded(&mut cx), [false, false], "new notes open");
    assert!(cx.debug_bounds("annotate-fold").is_some(), "two notes");

    // A click on a card's head folds it, and opens it again.
    let head = cx.debug_bounds("annotate-card-0").unwrap();
    cx.simulate_click(head.center(), gpui_kit::Modifiers::none());
    assert_eq!(folded(&mut cx), [true, false]);
    let folded_head = cx.debug_bounds("annotate-card-0").unwrap();
    assert!(
        folded_head.size.height < px(40.),
        "folded to a line: {folded_head:?}"
    );
    cx.simulate_click(folded_head.center(), gpui_kit::Modifiers::none());
    assert_eq!(folded(&mut cx), [false, false]);

    // With the keyboard on the head, Return and Space fold and open it.
    cx.update(|window, cx| {
        let head = panel.read(cx).rows[0].head.clone();
        window.focus(&head, cx);
    });
    cx.run_until_parked();
    let press = |key: &str, cx: &mut VisualTestContext| {
        use gpui_kit::{KeyDownEvent, KeyUpEvent, Keystroke};
        let keystroke =
            cx.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
        cx.run_until_parked();
    };
    press("enter", &mut cx);
    assert_eq!(folded(&mut cx), [true, false], "Return");
    press("space", &mut cx);
    assert_eq!(folded(&mut cx), [false, false], "Space");

    // Collapse all, then Expand all.
    let fold = cx.debug_bounds("annotate-fold").unwrap();
    cx.simulate_click(fold.center(), gpui_kit::Modifiers::none());
    assert_eq!(folded(&mut cx), [true, true]);
    cx.simulate_click(fold.center(), gpui_kit::Modifiers::none());
    assert_eq!(folded(&mut cx), [false, false]);

    // A third note opens with the others; a fourth folds the three before it.
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.toggle(1, cx);
            p.add(at("http://a/1", 3), false, window, cx);
        })
    });
    assert_eq!(
        folded(&mut cx),
        [false, true, false],
        "kept as the user left them"
    );
    cx.update(|window, cx| panel.update(cx, |p, cx| p.add(at("http://a/1", 4), false, window, cx)));
    assert_eq!(folded(&mut cx), [true, true, true, false]);

    // A deleted card comes back as it was.
    cx.update(|_, cx| {
        panel.update(cx, |p, cx| {
            p.toggle(0, cx);
            p.remove(0, cx);
            p.undo(cx);
        })
    });
    assert_eq!(folded(&mut cx), [false, true, true, false]);
}
