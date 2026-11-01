//! What the panel's actions do to the file, run as a click on a row runs them: selecting, renaming,
//! the eye and padlock, the pages, a drop, and collapsing the rows.

use std::collections::HashSet;

use figcraft_doc::NodeId;
use gpui_kit::TestAppContext;
use sawit_board::layers::DropPos;
use serde_json::{Value, json};

use super::super::drag::drop_pos;
use super::super::{LayerDrag, PageAction};
use super::{
    children, command, design_file, name_of, open, pages, selection, shown_and_locked, toast,
    undo_steps,
};
use crate::board::tests::{Scratch, run_op};

#[gpui_kit::test]
fn click_selects_and_shift_selects_range(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-range");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let view = open(cx, &path);
    // F open: the rows are F, B, A and G.
    view.update(cx, |view, cx| {
        view.expanded.insert(t.f);
        view.select_rows(0, false, false, cx);
    });
    assert_eq!(selection(&view, cx), [t.f]);
    // ⇧: the rows from the one clicked last to this one.
    view.update(cx, |view, cx| view.select_rows(2, true, false, cx));
    assert_eq!(selection(&view, cx), [t.f, t.b, t.a]);
    // Up the list as well, from a row clicked alone.
    view.update(cx, |view, cx| {
        view.select_rows(3, false, false, cx);
        view.select_rows(1, true, false, cx);
    });
    let picked: HashSet<NodeId> = selection(&view, cx).into_iter().collect();
    assert_eq!(picked, HashSet::from([t.b, t.a, t.g]));
    assert_eq!(toast(&view, cx), None);
}

#[gpui_kit::test]
fn cmd_click_toggles(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-toggle");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let view = open(cx, &path);
    view.update(cx, |view, cx| {
        view.expanded.insert(t.f);
        view.select_rows(1, false, false, cx);
        view.select_rows(3, false, true, cx);
    });
    assert_eq!(selection(&view, cx), [t.b, t.g]);
    view.update(cx, |view, cx| view.select_rows(1, false, true, cx));
    assert_eq!(selection(&view, cx), [t.g]);
    // ⇧ with no row clicked before in the selection toggles too.
    view.update(cx, |view, cx| {
        view.layer_anchor = None;
        view.select_rows(2, true, false, cx);
    });
    assert_eq!(selection(&view, cx), [t.g, t.a]);
}

/// Review Focus 2, and the rename ruling of P4.T1's review: a blank name, or the name the
/// layer has, changes nothing and makes no undo step.
#[gpui_kit::test]
fn blank_rename_keeps_name(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-blank-rename");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let view = open(cx, &path);
    let steps = undo_steps(&view, cx);
    for blank in ["  ", "", "\t"] {
        view.update(cx, |view, cx| view.rename_layer(t.a, blank, cx));
        assert_eq!(name_of(&view, t.a, cx), "A");
    }
    view.update(cx, |view, cx| view.rename_layer(t.a, "A", cx));
    assert_eq!(undo_steps(&view, cx), steps);
    // A name typed is kept, as one undo step.
    view.update(cx, |view, cx| view.rename_layer(t.a, "Avatar", cx));
    assert_eq!(name_of(&view, t.a, cx), "Avatar");
    assert_eq!(undo_steps(&view, cx), steps + 1);
    assert_eq!(toast(&view, cx), None);
}

#[gpui_kit::test]
fn eye_and_lock_toggle(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-eye-lock");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let view = open(cx, &path);
    let steps = undo_steps(&view, cx);
    view.update(cx, |view, cx| view.toggle_visible(t.a, cx));
    assert_eq!(shown_and_locked(&view, t.a, cx), (false, false));
    view.update(cx, |view, cx| view.toggle_visible(t.a, cx));
    assert_eq!(shown_and_locked(&view, t.a, cx), (true, false));
    view.update(cx, |view, cx| view.toggle_locked(t.b, cx));
    assert_eq!(shown_and_locked(&view, t.b, cx), (true, true));
    view.update(cx, |view, cx| view.toggle_locked(t.b, cx));
    assert_eq!(shown_and_locked(&view, t.b, cx), (true, false));
    // Each one undo step, on the row's layer alone.
    assert_eq!(undo_steps(&view, cx), steps + 4);
    assert_eq!(shown_and_locked(&view, t.f, cx), (true, false));
}

/// Review Focus 4: deleting the open page opens another; the only page can't be deleted, and
/// FigCraft's refusal is a toast.
#[gpui_kit::test]
fn deleting_current_page_selects_another(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-delete-page");
    let path = tmp.0.join("design.sawit");
    design_file(&path);
    let view = open(cx, &path);
    let (_, first) = pages(&view, cx);
    view.update(cx, |view, cx| view.page_action(PageAction::Add, cx));
    let (all, _) = pages(&view, cx);
    let second = all[1];
    view.update(cx, |view, cx| {
        view.page_action(PageAction::Select(first), cx);
        view.page_action(PageAction::Select(second), cx);
    });
    assert_eq!(pages(&view, cx), (vec![first, second], second));
    view.update(cx, |view, cx| {
        view.page_action(PageAction::Delete(second), cx)
    });
    assert_eq!(pages(&view, cx), (vec![first], first));
    assert_eq!(toast(&view, cx), None);
    // The only page stays, open, and the toast says why.
    view.update(cx, |view, cx| {
        view.page_action(PageAction::Delete(first), cx)
    });
    assert_eq!(pages(&view, cx), (vec![first], first));
    assert!(toast(&view, cx).is_some());
}

#[gpui_kit::test]
fn pages_are_renamed_and_duplicated(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-pages");
    let path = tmp.0.join("design.sawit");
    design_file(&path);
    let view = open(cx, &path);
    let (_, first) = pages(&view, cx);
    let steps = undo_steps(&view, cx);
    // A blank name, or the page's own, changes nothing (as for a layer).
    view.update(cx, |view, cx| {
        view.page_action(PageAction::Rename(first, " ".into()), cx);
        view.page_action(PageAction::Rename(first, "Page 1".into()), cx);
    });
    assert_eq!(name_of(&view, first, cx), "Page 1");
    assert_eq!(undo_steps(&view, cx), steps);
    view.update(cx, |view, cx| {
        view.page_action(PageAction::Rename(first, "Cover".into()), cx);
        view.page_action(PageAction::Duplicate(first), cx);
    });
    assert_eq!(name_of(&view, first, cx), "Cover");
    // The copy comes right after the page, and opens.
    let (all, open) = pages(&view, cx);
    assert_eq!(all.len(), 2);
    assert_eq!((all[0], open), (first, all[1]));
    assert_eq!(name_of(&view, all[1], cx), "Copy of Cover");
    assert_eq!(toast(&view, cx), None);
}

#[gpui_kit::test]
fn drop_reorders(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-drop");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let view = open(cx, &path);
    assert_eq!(children(&view, t.f, cx), [t.g, t.a, t.b]);
    let steps = undo_steps(&view, cx);
    // F open: F, B, A and G. A, dragged, lands above B, on top of F's layers.
    view.update(cx, |view, cx| {
        view.expanded.insert(t.f);
        view.drag = Some(LayerDrag {
            ids: vec![t.a],
            over: None,
        });
        view.drop_rows(1, DropPos::Above, cx);
    });
    assert_eq!(children(&view, t.f, cx), [t.g, t.b, t.a]);
    assert!(view.read_with(cx, |view, _| view.drag.is_none()));
    assert_eq!(undo_steps(&view, cx), steps + 1);
    view.update(cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    assert_eq!(children(&view, t.f, cx), [t.g, t.a, t.b]);
    // A drop that would leave the layers where they are makes no undo step.
    view.update(cx, |view, cx| {
        view.drag = Some(LayerDrag {
            ids: vec![t.b],
            over: None,
        });
        view.drop_rows(2, DropPos::Above, cx);
    });
    assert_eq!(children(&view, t.f, cx), [t.g, t.a, t.b]);
    assert_eq!(undo_steps(&view, cx), steps);
}

/// P4.T1's review: a layer inside an instance is the engine's to refuse. Dropped on a free
/// frame, it stays where it was, the toast says why, and no undo step is made.
#[gpui_kit::test]
fn a_layer_dragged_out_of_an_instance_stays_and_the_toast_says_why(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-instance");
    let path = tmp.0.join("design.sawit");
    let mut session = figcraft_engine::Session::new();
    let mut make = |command: &str, params: Value| {
        let made = session.execute(command, &params).unwrap();
        figcraft_engine::params::id_of(&made["id"]).unwrap()
    };
    let card = make(
        "node.create",
        json!({"type": "frame", "name": "Card", "x": 0, "y": 0, "width": 100, "height": 100}),
    );
    make(
        "node.create",
        json!({"type": "rectangle", "name": "Label", "parent": card, "width": 20, "height": 20}),
    );
    let main = make("component.create", json!({"ids": [card]}));
    let instance = make(
        "instance.create",
        json!({"component": main, "x": 200, "y": 0}),
    );
    let free = make(
        "node.create",
        json!({"type": "frame", "name": "Free", "x": 400, "y": 0, "width": 100, "height": 100}),
    );
    session.execute("select.none", &json!({})).unwrap();
    session
        .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
        .unwrap();
    let view = open(cx, &path);
    let inside = children(&view, instance, cx);
    let [label] = inside[..] else {
        panic!("the instance holds {inside:?}")
    };
    let steps = undo_steps(&view, cx);
    view.update(cx, |view, cx| {
        view.expanded.insert(instance);
        let rows = view.layer_rows();
        let over = rows.iter().position(|row| row.id == free).unwrap();
        view.drag = Some(LayerDrag {
            ids: vec![label],
            over: None,
        });
        view.drop_rows(over, DropPos::Into, cx);
    });
    assert_eq!(children(&view, instance, cx), [label]);
    assert!(children(&view, free, cx).is_empty());
    assert!(selection(&view, cx).is_empty());
    assert_eq!(undo_steps(&view, cx), steps);
    assert!(toast(&view, cx).is_some());
}

/// Ruling P4-C9: Collapse layers (⌥L) closes every row but those leading to the selection.
#[gpui_kit::test]
fn collapse_layers_keeps_the_way_to_the_selection(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-collapse");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let view = open(cx, &path);
    let expanded = |cx: &TestAppContext| view.read_with(cx, |view, _| view.expanded.clone());
    view.update(cx, |view, _| {
        view.expanded.extend([t.f, t.g, t.b]);
        view.board_mut().unwrap().session.set_selection(vec![t.c]);
    });
    run_op(&view, command("layers.collapseAll"), cx);
    assert_eq!(expanded(cx), HashSet::from([t.f, t.g]));
    run_op(&view, command("select.none"), cx);
    run_op(&view, command("layers.collapseAll"), cx);
    assert!(expanded(cx).is_empty());
}

#[test]
fn a_drop_goes_into_a_row_in_its_middle_half_when_the_row_takes_layers() {
    assert_eq!(drop_pos(0.1, true), DropPos::Above);
    assert_eq!(drop_pos(0.3, true), DropPos::Into);
    assert_eq!(drop_pos(0.7, true), DropPos::Into);
    assert_eq!(drop_pos(0.8, true), DropPos::Below);
    assert_eq!(drop_pos(0.3, false), DropPos::Above);
    assert_eq!(drop_pos(0.6, false), DropPos::Below);
}
