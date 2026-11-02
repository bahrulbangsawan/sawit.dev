//! Annotate's notes: from the page's popover to the panel, markers, and notes not yet sent.

use serde_json::json;

use super::test_browser;
use crate::browser;
use crate::browser::annotate::AnnotateEvent;
use crate::browser::notes::markers_op;

/// A note written in the page's popover arrives with its comment and kind; the popover's
/// later saves and its Remove reach the same row, and only the page's own notes.
#[gpui_kit::test]
fn notes_from_the_popover_reach_the_panel(cx: &mut gpui_kit::TestAppContext) {
    use sawit_core::annotations::Intent;
    use sawit_core::browser::PageEvent;
    let (view, cx) = test_browser(cx);
    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(browser::Panel::Annotate, cx)));
    let send = |event: PageEvent, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            view.update(cx, |b, cx| {
                b.receive(browser::FromPage::Event(event), window, cx)
            })
        });
        cx.run_until_parked();
    };
    send(
        PageEvent::Picked(json!({"mode": "annotate", "comment": "Make it blue",
                "intent": "change", "element": {
                "selector": "#buy", "label": "button#buy", "tag": "button",
                "rect": {"x": 1, "y": 1, "width": 40, "height": 20},
                "role": "button", "name": "Buy"}})),
        cx,
    );
    let notes = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| {
            view.read(cx)
                .annotate
                .read(cx)
                .annotations(cx)
                .into_iter()
                .map(|a| (a.n, a.comment, a.intent))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(notes(cx), [(1, "Make it blue".into(), Intent::Change)]);

    send(
        PageEvent::Annotation(
            json!({"action": "update", "n": 1, "comment": "Make it red", "intent": "question"}),
        ),
        cx,
    );
    assert_eq!(notes(cx), [(1, "Make it red".into(), Intent::Question)]);
    // No note 7: nothing changes.
    send(
        PageEvent::Annotation(json!({"action": "remove", "n": 7})),
        cx,
    );
    assert_eq!(notes(cx).len(), 1);
    send(
        PageEvent::Annotation(json!({"action": "remove", "n": 1})),
        cx,
    );
    assert!(notes(cx).is_empty());
}

/// A note typed on an annotation holds the tab open (closing it asks) until it is copied
/// or sent; a note typed after that holds it again.
#[gpui_kit::test]
fn unsent_notes_hold_the_tab(cx: &mut gpui_kit::TestAppContext) {
    use sawit_core::browser::PageEvent;
    let (view, cx) = test_browser(cx);
    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(browser::Panel::Annotate, cx)));
    let pick = serde_json::json!({"mode": "annotate", "element": {
            "selector": "#buy", "label": "button#buy", "tag": "button",
            "rect": {"x": 1, "y": 1, "width": 40, "height": 20}}});
    cx.update(|window, cx| {
        view.update(cx, |b, cx| {
            b.receive(
                browser::FromPage::Event(PageEvent::Picked(pick)),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    let unsent =
        |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| view.read(cx).unsent_notes(cx));
    assert_eq!(unsent(cx), 0, "a row with no note");
    let note = |text: &str, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            let panel = view.read(cx).annotate.clone();
            panel.update(cx, |p, cx| p.type_note(0, text, window, cx));
        })
    };
    note("Make it blue", cx);
    assert_eq!(unsent(cx), 1);
    cx.update(|_, cx| view.update(cx, |b, cx| b.on_annotate(&AnnotateEvent::Copy, cx)));
    assert_eq!(unsent(cx), 0, "copied");
    note("Make it blue, and bigger", cx);
    assert_eq!(unsent(cx), 1, "changed since");

    // Sending asks the workspace, with the notes; they count as sent once they went.
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = asked.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &browser::BrowserEvent, _| {
            if let browser::BrowserEvent::SendToAgent { markdown, agent } = event {
                seen.borrow_mut().push((markdown.clone(), agent.pane));
            }
        })
        .detach()
    });
    let agent = crate::terminal::RunningAgent {
        tab: gpui_kit::WeakEntity::new_invalid(),
        pane: 7,
        kind: sawit_core::terminal::AgentKind::ClaudeCode,
        label: "shop".into(),
        focused: true,
    };
    cx.update(|_, cx| view.update(cx, |b, cx| b.on_annotate(&AnnotateEvent::SendTo(agent), cx)));
    assert_eq!(asked.borrow().len(), 1);
    assert_eq!(asked.borrow()[0].1, 7, "to the agent chosen");
    assert!(asked.borrow()[0].0.contains("Make it blue, and bigger"));
    assert_eq!(unsent(cx), 1, "asked for, not sent yet");
    let gone = "Claude Code is no longer running there, so nothing was sent.";
    cx.update(|_, cx| view.update(cx, |b, cx| b.finish_send(Err(gone.into()), cx)));
    assert_eq!(unsent(cx), 1, "the agent had ended");
    let sent = "Sent to Claude Code (shop)";
    cx.update(|_, cx| view.update(cx, |b, cx| b.finish_send(Ok(sent.into()), cx)));
    assert_eq!(unsent(cx), 0, "sent");
}

#[test]
fn markers_are_numbered_selectors() {
    use browser::annotate::Marker;
    use sawit_core::annotations::Intent;
    let marker = |n, selector: &str, comment: &str| Marker {
        n,
        selector: selector.into(),
        comment: comment.into(),
        intent: Intent::Change,
        label: "a".into(),
    };
    assert_eq!(
        markers_op("tok", &[marker(1, "#a", "Bigger"), marker(3, "b > i", "")]),
        json!({"op": "markers", "token": "tok", "items": [
                {"n": 1, "selector": "#a", "comment": "Bigger", "intent": "change", "label": "a"},
                {"n": 3, "selector": "b > i", "comment": "", "intent": "change", "label": "a"}]})
    );
    assert_eq!(
        markers_op("tok", &[]),
        json!({"op": "markers", "token": "tok", "items": []})
    );
}
