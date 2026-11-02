//! The components agents build: begun, built into, progressed, done, failed, stalled and
//! dismissed; what the canvas shows of them, what it says, and how they undo.

use std::time::Duration;

use figcraft_doc::NodeId;
use gpui_kit::{Entity, TestAppContext};
use sawit_core::agents::work::{CHECK_FADE, NAME_AFTER, RowLook, STALL_AFTER};
use sawit_core::agents::{AgentIdentity, FADE_AFTER};
use serde_json::{Value, json};

use super::super::announce::HOLD;
use super::super::marks::Mark;
use super::{codex, wait};
use crate::board::BoardView;
use crate::board::tests::{Scratch, next_frame, open_in_window, run_op};
use crate::workspace::Answer;

fn gemini() -> AgentIdentity {
    AgentIdentity {
        id: "mcp-9-9".into(),
        name: "Gemini CLI".into(),
        label: None,
        version: None,
    }
}

/// Runs `look`'s op on the canvas, which has to answer at once.
fn agent(
    view: &Entity<BoardView>,
    who: &AgentIdentity,
    op: Value,
    cx: &mut TestAppContext,
) -> Value {
    match view.update(cx, |view, cx| view.agent_op(op, Some(who), cx)) {
        Answer::Now(reply) => reply,
        Answer::Later(_) => panic!("a quick op answers at once"),
    }
}

/// A canvas in a window with work "Pricing card" begun by Codex: the canvas and the work's id.
fn begun(test: &str, cx: &mut TestAppContext) -> (Scratch, Entity<BoardView>, String) {
    let tmp = Scratch::new(test);
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let frame = json!({"x": 40, "y": 60, "width": 240, "height": 180});
    let steps = json!(["Laying out", "Writing copy", "Styling"]);
    let op = json!({"op": "begin", "name": "Pricing card", "frame": frame, "steps": steps});
    let reply = agent(&view, &codex(), op, cx);
    assert_eq!(reply["ok"], true, "{reply}");
    (tmp, view, reply["work"].as_str().unwrap().to_string())
}

/// What is painted for the agents on the page on show now.
fn marks(view: &Entity<BoardView>, cx: &mut TestAppContext) -> Vec<Mark> {
    view.read_with(cx, |view, cx| {
        let ds = view.board().unwrap().session.ds().unwrap();
        let now = cx.background_executor().now();
        view.agents.marks(&ds.doc, &ds.view, ds.page, now, false)
    })
}

fn row(view: &Entity<BoardView>, cx: &mut TestAppContext) -> RowLook {
    view.read_with(cx, |view, cx| {
        view.agents.works[0].row(cx.background_executor().now())
    })
}

fn said(view: &Entity<BoardView>, cx: &mut TestAppContext) -> String {
    view.read_with(cx, |view, _| view.agents.announcer.current.to_string())
}

/// A begun work shows at once: its frame's skeleton, its first step, the agent's pointer at the
/// frame's top-right corner with its name on the row instead of a pill, and a polite sentence.
#[gpui_kit::test]
fn a_begun_work_shows_its_skeleton_row_and_pointer(cx: &mut TestAppContext) {
    let (_tmp, view, id) = begun("work-begun", cx);
    view.read_with(cx, |view, _| {
        let work = &view.agents.works[0];
        assert_eq!(
            (work.id.as_str(), work.name.as_ref()),
            (id.as_str(), "Pricing card")
        );
        assert_eq!(work.step.as_deref(), Some("Laying out"));
        let agent = &view.agents.agents[0];
        assert_eq!(
            agent.at.map(|at| at.point),
            Some((280.0, 60.0)),
            "the frame's top-right"
        );
        assert_eq!(agent.work.as_deref(), Some(id.as_str()));
    });
    assert_eq!(said(&view, cx), "Codex started Pricing card");
    let painted = marks(&view, cx);
    assert!(
        painted
            .iter()
            .filter(|m| matches!(m, Mark::Bar { .. }))
            .count()
            >= 3
    );
    assert!(painted.iter().any(|m| matches!(m, Mark::Cursor { .. })));
    assert!(
        !painted.iter().any(|m| matches!(m, Mark::Pill { .. })),
        "the name is on the row"
    );
    assert_eq!(row(&view, cx), RowLook::Working { stalled: false });
    // Content landing where a bar is takes that bar away.
    let title = json!({"type": "text", "x": 14, "y": 14, "width": 160, "text": "Pro"});
    agent(
        &view,
        &codex(),
        json!({"op": "add", "work": id, "elements": [title]}),
        cx,
    );
    let bars = marks(&view, cx)
        .iter()
        .filter(|m| matches!(m, Mark::Bar { .. }))
        .count();
    assert!(
        bars >= 2
            && bars
                < painted
                    .iter()
                    .filter(|m| matches!(m, Mark::Bar { .. }))
                    .count()
    );
}

/// Steps show as they are said; done is said, the name goes after 2 s and the check after it;
/// a failure keeps its reason until the agent retries.
#[gpui_kit::test]
fn progress_shows_steps_done_and_failure(cx: &mut TestAppContext) {
    let (_tmp, view, id) = begun("work-progress", cx);
    let step = json!({"op": "progress", "work": id, "step": "Writing copy"});
    let reply = agent(&view, &codex(), step, cx);
    assert_eq!(
        (reply["ok"].clone(), reply["shown"].clone()),
        (json!(true), json!(true))
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.agents.works[0].step.as_deref(), Some("Writing copy"))
    });
    let failed = json!({"op": "progress", "work": id, "status": "failed", "message": "Couldn't load the logo"});
    agent(&view, &codex(), failed, cx);
    assert_eq!(row(&view, cx), RowLook::Failed);
    assert!(
        marks(&view, cx)
            .iter()
            .any(|m| matches!(m, Mark::Failed { .. }))
    );
    wait(cx, HOLD);
    assert_eq!(
        said(&view, cx),
        "Codex couldn't finish Pricing card: couldn't load the logo"
    );
    wait(cx, Duration::from_secs(90));
    assert_eq!(row(&view, cx), RowLook::Failed, "it keeps its reason");
    // Any op of the work is a retry.
    let retry = json!({"op": "add", "work": id, "elements": [{"type": "text", "text": "Logo"}]});
    agent(&view, &codex(), retry, cx);
    assert_eq!(row(&view, cx), RowLook::Working { stalled: false });
    agent(
        &view,
        &codex(),
        json!({"op": "progress", "work": id, "status": "done"}),
        cx,
    );
    assert!(matches!(row(&view, cx), RowLook::Done { name: true, .. }));
    // Said at once: the sentence before it has long been heard.
    assert_eq!(said(&view, cx), "Codex finished Pricing card");
    wait(cx, NAME_AFTER);
    assert!(matches!(row(&view, cx), RowLook::Done { name: false, .. }));
    wait(cx, CHECK_FADE);
    assert_eq!(row(&view, cx), RowLook::Gone);
    let unknown = agent(
        &view,
        &codex(),
        json!({"op": "progress", "work": "w_0"}),
        cx,
    );
    assert!(
        unknown["error"]
            .as_str()
            .unwrap()
            .contains("isn't on the canvas"),
        "{unknown}"
    );
}

/// A work quiet for a minute shows as stalled, which is said once; the user can dismiss it, and
/// its frame stays.
#[gpui_kit::test]
fn a_quiet_work_stalls_and_can_be_dismissed(cx: &mut TestAppContext) {
    let (_tmp, view, id) = begun("work-stall", cx);
    wait(cx, STALL_AFTER);
    assert_eq!(row(&view, cx), RowLook::Working { stalled: true });
    wait(cx, HOLD);
    assert_eq!(said(&view, cx), "Codex stalled on Pricing card");
    let frame = view.read_with(cx, |view, _| view.agents.works[0].frame);
    view.update(cx, |view, cx| view.dismiss_work(&id, cx));
    assert_eq!(row(&view, cx), RowLook::Gone, "its row goes at once");
    // The canvas, with only works on it, lets go of them at its next quiet tick.
    wait(cx, Duration::from_millis(1100));
    view.read_with(cx, |view, _| {
        assert!(view.agents.works.is_empty());
        assert!(
            view.board().unwrap().doc().unwrap().node(frame).is_some(),
            "the frame stays"
        );
    });
}

/// Everything one work did undoes as one step while nothing else happens between its ops; another
/// agent's op in between starts a new step of the work.
#[gpui_kit::test]
fn a_works_ops_undo_as_one_step(cx: &mut TestAppContext) {
    let (_tmp, view, id) = begun("work-undo", cx);
    let depth = |cx: &mut TestAppContext| {
        view.read_with(cx, |view, _| {
            view.board()
                .unwrap()
                .session
                .ds()
                .unwrap()
                .history
                .undo
                .len()
        })
    };
    let one = depth(cx);
    let text =
        |t: &str| json!({"op": "add", "work": id, "elements": [{"type": "text", "text": t}]});
    agent(&view, &codex(), text("Pro"), cx);
    agent(&view, &codex(), text("$24"), cx);
    assert_eq!(depth(cx), one, "joined to the begin's step");
    let label = view.read_with(cx, |view, _| {
        view.board()
            .unwrap()
            .session
            .ds()
            .unwrap()
            .history
            .undo
            .last()
            .unwrap()
            .label
            .clone()
    });
    assert_eq!(label, "Pricing card");
    run_op(&view, json!({"op": "command", "command": "edit.undo"}), cx);
    let frame = view.read_with(cx, |view, _| view.agents.works[0].frame);
    view.read_with(cx, |view, _| {
        assert!(
            view.board().unwrap().doc().unwrap().node(frame).is_none(),
            "the whole component"
        );
    });
    run_op(&view, json!({"op": "command", "command": "edit.redo"}), cx);
    // Gemini's sticky between two of Codex's ops: Codex's next op is a step of its own.
    let sticky = json!({"op": "add", "elements": [{"type": "sticky", "x": 900, "y": 0}]});
    agent(&view, &gemini(), sticky, cx);
    agent(&view, &codex(), text("Seats"), cx);
    assert_eq!(depth(cx), one + 2);
}

/// Agents may not move the user's view or page: the canvas refuses it, and the view stays.
#[gpui_kit::test]
fn an_agent_may_not_move_the_view(cx: &mut TestAppContext) {
    let (_tmp, view, _) = begun("work-view", cx);
    let zoom = view.read_with(cx, |view, _| {
        view.board().unwrap().session.ds().unwrap().view.zoom
    });
    let reply = agent(
        &view,
        &codex(),
        json!({"op": "command", "command": "view.zoomIn"}),
        cx,
    );
    assert_eq!(reply["ok"], false, "{reply}");
    let after = view.read_with(cx, |view, _| {
        view.board().unwrap().session.ds().unwrap().view.zoom
    });
    assert_eq!(zoom, after);
}

/// New text shows a word at a time in what is drawn, never in the document; under reduced
/// motion it shows whole.
#[gpui_kit::test]
fn new_text_is_revealed_a_word_at_a_time(cx: &mut TestAppContext) {
    let (_tmp, view, id) = begun("work-reveal", cx);
    let copy = json!({"type": "text", "x": 14, "y": 40, "text": "Unlimited canvases for teams"});
    let reply = agent(
        &view,
        &codex(),
        json!({"op": "add", "work": id, "elements": [copy]}),
        cx,
    );
    let text: NodeId = reply["ids"][0].as_str().unwrap().parse().unwrap();
    let hidden = |cx: &mut TestAppContext| {
        view.update(cx, |view, cx| {
            let now = cx.background_executor().now();
            let doc = view.board().unwrap().session.ds().unwrap().doc.clone();
            let drawn = view.agents.reveals.document(&doc, now)?;
            let runs = drawn.node(text)?.kind.text()?.runs.clone();
            runs.iter()
                .find(|r| r.style.fills == Some(Vec::new()))
                .map(|r| (r.start, r.end))
        })
    };
    assert_eq!(hidden(cx), Some((9, 28)), "the first word shows");
    cx.executor().advance_clock(Duration::from_millis(45));
    assert_eq!(hidden(cx), Some((18, 28)), "the second word");
    cx.executor().advance_clock(Duration::from_millis(800));
    assert_eq!(hidden(cx), None, "whole within 0.8 s");
    cx.update(|cx| cx.set_reduce_motion(true));
    let more = json!({"type": "text", "x": 14, "y": 80, "text": "Agent presence for five"});
    agent(
        &view,
        &codex(),
        json!({"op": "add", "work": id, "elements": [more]}),
        cx,
    );
    assert_eq!(hidden(cx), None, "reduced motion");
}

/// A free pointer's name shows for 2 s after its op, and again while the user's pointer rests on
/// it; the canvas's tab hears when its agents start and stop work.
#[gpui_kit::test]
fn a_free_pointers_name_shows_while_it_works_and_on_hover(cx: &mut TestAppContext) {
    let tmp = Scratch::new("work-free-name");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let heard = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = heard.clone();
    let _listen = cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &crate::board::AgentActivity, _| {
            count.set(count.get() + 1)
        })
    });
    let sticky = json!({"op": "add", "elements": [{"type": "sticky", "x": -100, "y": -60}]});
    agent(&view, &codex(), sticky, cx);
    let pills = |cx: &mut TestAppContext| {
        marks(&view, cx)
            .iter()
            .filter(|m| matches!(m, Mark::Pill { .. }))
            .count()
    };
    assert_eq!(pills(cx), 1);
    assert_eq!(heard.get(), 1, "at work");
    wait(cx, NAME_AFTER);
    assert_eq!(pills(cx), 0);
    let hit = view.read_with(cx, |view, cx| {
        let ds = view.board().unwrap().session.ds().unwrap();
        let at = view.agents.agents[0].at.unwrap().point;
        let (x, y) = ds.view.page_to_screen(at.0, at.1);
        view.agents.pointer_at(
            &ds.view,
            ds.page,
            (x + 6.0, y + 6.0),
            cx.background_executor().now(),
        )
    });
    assert_eq!(hit.as_deref(), Some("mcp-1-2"));
    view.update(cx, |view, _| view.agents.hovered = hit);
    assert_eq!(pills(cx), 1, "on hover");
    wait(cx, FADE_AFTER);
    assert_eq!(heard.get(), 2, "no longer at work");
}

/// The roster's tally: building, done and failed works, and idle agents apart.
#[gpui_kit::test]
fn the_roster_counts_works_by_state(cx: &mut TestAppContext) {
    let (_tmp, view, card) = begun("work-roster", cx);
    let frame = |x: i32| json!({"x": x, "y": 400, "width": 200, "height": 120});
    let nav = agent(
        &view,
        &gemini(),
        json!({"op": "begin", "name": "Nav", "frame": frame(0)}),
        cx,
    );
    let nav = nav["work"].as_str().unwrap().to_string();
    let form = agent(
        &view,
        &gemini(),
        json!({"op": "begin", "name": "Form", "frame": frame(400)}),
        cx,
    );
    let form = form["work"].as_str().unwrap().to_string();
    agent(
        &view,
        &codex(),
        json!({"op": "progress", "work": card, "status": "done"}),
        cx,
    );
    agent(
        &view,
        &gemini(),
        json!({"op": "progress", "work": nav, "status": "failed"}),
        cx,
    );
    let words = view.read_with(cx, |view, cx| {
        view.agents.tally(cx.background_executor().now()).words()
    });
    assert_eq!(words, "1 building · 1 done · 1 failed");
    let _ = form;
}
