//! Comment threads as rows: no marker, never a note lost with the tab, removed and put back as
//! thread changes for mesurer, kept by an agent's clear while resolved.

use gpui_kit::VisualTestContext;
use sawit_core::annotations::{Source, Thread, ThreadMessage, ThreadStatus};

use super::{at, panel};
use crate::browser::annotate::{AnnotatePanel, ThreadChange};

fn thread(id: &str, status: ThreadStatus) -> Thread {
    Thread {
        id: id.into(),
        messages: vec![ThreadMessage {
            text: format!("About {id}"),
            created_at: 1.,
        }],
        status,
        origin: "http_a".into(),
        page: "/1".into(),
        preview: None,
        frames: Vec::new(),
        target: format!("#{id}"),
    }
}

fn add(panel: &gpui_kit::Entity<AnnotatePanel>, n: u32, t: Thread, cx: &mut VisualTestContext) {
    let mut a = at("http://a/1", n);
    a.comment = t.first().to_string();
    a.source = Source::Comment(t);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.add_comment(a, cx)));
}

#[gpui_kit::test]
fn comment_rows_are_mesurers_threads(cx: &mut gpui_kit::TestAppContext) {
    let (panel, cx) = panel(cx);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.add(at("http://a/1", 1), false, window, cx)));
    add(&panel, 2, thread("c1", ThreadStatus::Open), cx);
    add(&panel, 3, thread("c2", ThreadStatus::Resolved), cx);
    let read = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let p = panel.read(cx);
            (
                p.annotations(cx).iter().map(|a| a.n).collect::<Vec<_>>(),
                p.markers_for("http://a/1", cx)
                    .iter()
                    .map(|m| m.n)
                    .collect::<Vec<_>>(),
                p.thread_ids("http_a", "/1"),
            )
        })
    };
    let (rows, markers, ids) = read(cx);
    assert_eq!(rows, [1, 2, 3]);
    assert_eq!(markers, [1], "mesurer's pins mark threads");
    assert_eq!(ids, ["c1", "c2"]);
    assert_eq!(
        cx.update(|_, cx| panel.read(cx).unsent(cx)),
        0,
        "an empty note, and threads"
    );
    cx.update(|window, cx| panel.update(cx, |p, cx| p.type_note(0, "Bigger", window, cx)));
    assert_eq!(
        cx.update(|_, cx| panel.read(cx).unsent(cx)),
        1,
        "the note only"
    );

    // A reply reaches the row; a thread mesurer deleted drops it, with no change to carry back.
    let mut replied = thread("c1", ThreadStatus::Open);
    replied.messages.push(ThreadMessage {
        text: "Also red".into(),
        created_at: 2.,
    });
    let updated = cx.update(|_, cx| panel.update(cx, |p, cx| p.update_thread(replied, cx)));
    assert!(updated);
    let texts = cx.update(|_, cx| {
        panel.read(cx).annotations(cx)[1]
            .thread()
            .unwrap()
            .messages
            .len()
    });
    assert_eq!(texts, 2);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.drop_thread("http_a", "c1", cx)));
    assert_eq!(read(cx).0, [1, 3]);
    let changes = cx.update(|_, cx| panel.update(cx, |p, _| p.take_thread_changes()));
    assert!(changes.is_empty());
}

#[gpui_kit::test]
fn removing_a_comment_row_takes_its_thread_and_undo_puts_it_back(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (panel, cx) = panel(cx);
    add(&panel, 1, thread("c1", ThreadStatus::Open), cx);
    add(&panel, 2, thread("c2", ThreadStatus::Resolved), cx);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.remove(0, cx)));
    let changes = cx.update(|_, cx| panel.update(cx, |p, _| p.take_thread_changes()));
    assert_eq!(
        changes,
        [ThreadChange::Removed(thread("c1", ThreadStatus::Open))]
    );
    let note = cx.update(|_, cx| panel.read(cx).note());
    assert_eq!(note.as_deref(), Some("Removed comment 1"));
    cx.update(|_, cx| panel.update(cx, |p, cx| p.undo(cx)));
    let changes = cx.update(|_, cx| panel.update(cx, |p, _| p.take_thread_changes()));
    assert_eq!(
        changes,
        [ThreadChange::Restored(thread("c1", ThreadStatus::Open))]
    );
    assert_eq!(
        cx.update(|_, cx| panel.read(cx).thread_ids("http_a", "/1")),
        ["c1", "c2"]
    );

    // An agent's clear: the resolved thread, which it didn't read, keeps its row.
    cx.update(|_, cx| panel.update(cx, |p, cx| p.clear(cx)));
    assert_eq!(
        cx.update(|_, cx| panel.read(cx).thread_ids("http_a", "/1")),
        ["c2"]
    );
    let changes = cx.update(|_, cx| panel.update(cx, |p, _| p.take_thread_changes()));
    assert!(changes.is_empty(), "the browser deletes what an agent read");

    // "Copy for agent": the open threads count as copied.
    add(&panel, 3, thread("c3", ThreadStatus::Open), cx);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.comments_copied("http_a", "/1", cx)));
    let copied = cx.update(|_, cx| {
        let p = panel.read(cx);
        (p.thread_copied("c3"), p.thread_copied("c2"), p.note())
    });
    assert_eq!(
        copied,
        (true, false, Some("Copied 1 comment for an agent".into()))
    );
}
