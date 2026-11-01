//! Tests of file changes: what one that fails keeps, and undo and redo while changes run.

use std::path::PathBuf;

use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::files::Entry;

use super::{Changes, ask, history, name_it, open_panel_at, select, typed};
use crate::files::{Op, Scratch};

/// Small slips (FILES-16). A Paste that fails ("… already exists here") keeps the Cut, to
/// paste elsewhere; a rename or a new name that fails brings its name box back with the
/// name typed, to fix; and a folder opened again is read again, as it may have changed
/// while it was closed. The Cut was cleared before its move ran, the name box closed with
/// the name in it, and a folder opened again showed what it held when it was closed.
#[gpui_kit::test]
fn a_change_that_fails_keeps_what_was_cut_or_typed(cx: &mut TestAppContext) {
    let dir = Scratch::new("a-change-that-fails");
    let root = dir.0.as_path();
    std::fs::write(root.join("a.txt"), "").unwrap();
    std::fs::create_dir(root.join("docs")).unwrap();
    let (window, panel) = open_panel_at(cx, root, &["a.txt", "docs"]);
    let changes = Changes::install(&panel, cx);
    let (a, docs) = (root.join("a.txt"), root.join("docs"));
    let cut = |cx: &mut TestAppContext| {
        cx.update(|cx| (panel.read(cx).clip.as_ref()).map(|c| (c.path.clone(), c.cut)))
    };

    // ⌘X, then ⌘V in a folder that has the name: the Cut is still there to paste.
    select(cx, window, &panel, &a);
    cx.simulate_keystrokes(window, "cmd-x");
    select(cx, window, &panel, &docs);
    cx.simulate_keystrokes(window, "cmd-v");
    let error = "“a.txt” already exists here.";
    changes.land(0, Err(error.into()), cx);
    assert_eq!(cut(cx), Some((a.clone(), true)));
    cx.simulate_keystrokes(window, "cmd-v");
    let back = Op::Move {
        from: docs.join("a.txt"),
        to: a.clone(),
    };
    changes.land(1, Ok(back), cx);
    assert_eq!(cut(cx), None, "pasted");

    // A rename to a name that is taken: the name box is back, with the name typed.
    select(cx, window, &panel, &a);
    cx.simulate_keystrokes(window, "f2");
    name_it(cx, window, &panel, "docs");
    let taken = "“docs” already exists here.";
    changes.land(2, Err(taken.into()), cx);
    assert_eq!(typed(&panel, cx).as_deref(), Some("docs"));
    cx.update(|cx| assert_eq!(panel.read(cx).error.as_deref(), Some(taken)));
    cx.simulate_keystrokes(window, "escape");

    // The same for a new file.
    select(cx, window, &panel, &a);
    cx.simulate_keystrokes(window, "cmd-n");
    name_it(cx, window, &panel, "a.txt");
    assert_eq!(
        changes.started().last(),
        Some(&Op::Create {
            path: a.clone(),
            dir: false
        })
    );
    changes.land(3, Err(error.into()), cx);
    assert_eq!(typed(&panel, cx).as_deref(), Some("a.txt"));
    cx.simulate_keystrokes(window, "escape");

    // A folder opened, closed and opened again is read each time.
    let folder = Entry {
        name: "docs".into(),
        path: docs.clone(),
        is_dir: true,
    };
    let before = changes.read().len();
    for _ in 0..2 {
        cx.update_window(window, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.toggle(&folder, window, cx);
                p.children.insert(docs.clone(), Vec::new());
                p.toggle(&folder, window, cx);
            })
        })
        .unwrap();
    }
    assert_eq!(changes.read()[before..], [docs.clone(), docs]);
}

/// ⌘Z pressed while a change runs undoes that change, once it has landed (FILES-5). Changes
/// used to run side by side: ⌘Z right after ⌘D on a big folder undid the change before the
/// copy, a rename here, and the copy, landing later, then wiped the redo of that rename.
#[gpui_kit::test]
fn undo_while_a_change_runs_undoes_that_change(cx: &mut TestAppContext) {
    let dir = Scratch::new("undo-while-a-change-runs");
    let root = dir.0.as_path();
    std::fs::create_dir(root.join("big")).unwrap();
    std::fs::write(root.join("config.yml"), "").unwrap();
    let (window, panel) = open_panel_at(cx, root, &["big", "config.yml"]);
    let changes = Changes::install(&panel, cx);
    let (yml, old) = (root.join("config.yml"), root.join("config.old.yml"));
    let (big, copy) = (root.join("big"), root.join("big copy"));
    let renamed = Op::Move {
        from: old.clone(),
        to: yml.clone(),
    };

    // A rename, landed: its undo is kept.
    let rename = Op::Move {
        from: yml.clone(),
        to: old.clone(),
    };
    ask(cx, window, &panel, rename);
    changes.land(0, Ok(renamed.clone()), cx);

    // ⌘D on the folder, then ⌘Z at once: the ⌘Z waits for the copy.
    select(cx, window, &panel, &big);
    cx.simulate_keystrokes(window, "cmd-d cmd-z");
    let copying = Op::Copy {
        from: big.clone(),
        to: copy.clone(),
    };
    assert_eq!(changes.started()[1..], [copying], "⌘Z waits for the copy");
    assert!(history(&panel, cx).2, "the panel shows a change runs");

    // The copy lands, and the ⌘Z undoes it.
    changes.land(1, Ok(Op::Trash(copy.clone())), cx);
    assert_eq!(changes.started()[2..], [Op::Trash(copy.clone())]);
    let restore = Op::Restore {
        from: PathBuf::from("/Trash/big copy"),
        to: copy.clone(),
    };
    changes.land(2, Ok(restore.clone()), cx);
    assert_eq!(history(&panel, cx), (vec![renamed], vec![restore], false));
}

/// ⌘Z pressed while a change runs is that change's: when the change fails, the ⌘Z is
/// skipped, and the panel says so (review-files M2). It ran, and undid the change before
/// it, a rename here, without a word; a ⇧⌘Z waiting behind a failed change redid an older
/// one. A ⌘Z pressed after the next change is that one's, and still runs.
#[gpui_kit::test]
fn an_undo_behind_a_change_that_fails_is_skipped(cx: &mut TestAppContext) {
    let dir = Scratch::new("an-undo-behind-a-failed-change");
    let root = dir.0.as_path();
    std::fs::create_dir(root.join("big")).unwrap();
    std::fs::write(root.join("config.yml"), "").unwrap();
    let (window, panel) = open_panel_at(cx, root, &["big", "config.yml"]);
    let changes = Changes::install(&panel, cx);
    let (yml, old) = (root.join("config.yml"), root.join("config.old.yml"));
    let (big, copy) = (root.join("big"), root.join("big copy"));
    let renamed = || Op::Move {
        from: old.clone(),
        to: yml.clone(),
    };
    let unrenamed = Op::Move {
        from: yml.clone(),
        to: old.clone(),
    };
    let copying = || Op::Copy {
        from: big.clone(),
        to: copy.clone(),
    };
    let denied = "“big” couldn't be copied: Permission denied (os error 13)";
    let said = |cx: &mut TestAppContext| {
        cx.update(|cx| panel.read(cx).error.clone())
            .unwrap_or_default()
    };

    // A rename, landed: its undo is kept.
    ask(cx, window, &panel, unrenamed.clone());
    changes.land(0, Ok(renamed()), cx);

    // ⌘D on the folder, then ⌘Z at once; the copy fails. The rename stays done.
    select(cx, window, &panel, &big);
    cx.simulate_keystrokes(window, "cmd-d cmd-z");
    assert_eq!(changes.started()[1..], [copying()]);
    changes.land(1, Err(denied.into()), cx);
    assert_eq!(
        changes.started()[2..],
        [],
        "the ⌘Z meant for the copy undid the rename"
    );
    assert_eq!(history(&panel, cx), (vec![renamed()], vec![], false));
    let error = said(cx);
    assert!(error.starts_with(denied), "{error}");
    assert!(
        error.contains("Undo") && error.contains("skipped"),
        "{error}"
    );

    // The same for ⇧⌘Z: with the rename undone, it would redo the rename.
    cx.simulate_keystrokes(window, "cmd-z");
    assert_eq!(changes.started()[2..], [renamed()]);
    changes.land(2, Ok(unrenamed.clone()), cx);
    select(cx, window, &panel, &big);
    cx.simulate_keystrokes(window, "cmd-d shift-cmd-z");
    assert_eq!(changes.started()[3..], [copying()]);
    changes.land(3, Err(denied.into()), cx);
    assert_eq!(changes.started()[4..], [], "the ⇧⌘Z redid the rename");
    assert_eq!(history(&panel, cx), (vec![], vec![unrenamed], false));
    let error = said(cx);
    assert!(
        error.contains("Redo") && error.contains("skipped"),
        "{error}"
    );

    // ⌘D ⌘Z ⌘D ⌘Z, and the first copy fails: its ⌘Z is skipped, and the second copy runs,
    // then its own ⌘Z.
    cx.simulate_keystrokes(window, "cmd-d cmd-z cmd-d cmd-z");
    assert_eq!(changes.started()[4..], [copying()]);
    changes.land(4, Err(denied.into()), cx);
    assert_eq!(changes.started()[5..], [copying()]);
    changes.land(5, Ok(Op::Trash(copy.clone())), cx);
    assert_eq!(changes.started()[6..], [Op::Trash(copy)]);
    assert_eq!(said(cx), "", "a change that lands clears the error");
}

/// ⌘D pressed twice on a folder that takes a while to copy makes two copies, as in Finder:
/// the second waits for the first, then takes the next free name. Both used to start at
/// once with the same name, and the second failed with "already exists".
#[gpui_kit::test]
fn a_second_duplicate_takes_the_next_free_name(cx: &mut TestAppContext) {
    let dir = Scratch::new("a-second-duplicate");
    let root = dir.0.as_path();
    std::fs::create_dir(root.join("big")).unwrap();
    let (window, panel) = open_panel_at(cx, root, &["big"]);
    let changes = Changes::install(&panel, cx);
    select(cx, window, &panel, &root.join("big"));
    cx.simulate_keystrokes(window, "cmd-d cmd-d");
    let copy = |to: &str| Op::Copy {
        from: root.join("big"),
        to: root.join(to),
    };
    assert_eq!(changes.started(), [copy("big copy")]);
    // The first copy made its folder.
    std::fs::create_dir(root.join("big copy")).unwrap();
    changes.land(0, Ok(Op::Trash(root.join("big copy"))), cx);
    assert_eq!(changes.started()[1..], [copy("big copy 2")]);
}

/// An undo that fails stays to try again once what was in its way is dealt with (FILES-5):
/// it used to leave the history. One whose file is gone, which can never be undone, goes.
#[gpui_kit::test]
fn an_undo_that_fails_stays_to_try_again(cx: &mut TestAppContext) {
    let dir = Scratch::new("an-undo-that-fails");
    let root = dir.0.as_path();
    std::fs::write(root.join("b.txt"), "").unwrap();
    let (window, panel) = open_panel_at(cx, root, &["b.txt"]);
    let changes = Changes::install(&panel, cx);
    let gone = Op::Move {
        from: root.join("gone.txt"),
        to: root.join("c.txt"),
    };
    let blocked = Op::Move {
        from: root.join("b.txt"),
        to: root.join("a.txt"),
    };
    cx.update(|cx| panel.update(cx, |p, _| p.undo = vec![gone.clone(), blocked.clone()]));
    select(cx, window, &panel, &root.join("b.txt"));

    cx.simulate_keystrokes(window, "cmd-z");
    changes.land(0, Err("“a.txt” already exists here.".into()), cx);
    let undo = vec![gone.clone(), blocked.clone()];
    assert_eq!(history(&panel, cx), (undo, vec![], false));
    cx.update(|cx| {
        assert_eq!(
            panel.read(cx).error.as_deref(),
            Some("“a.txt” already exists here.")
        )
    });

    // Tried again, and then the one whose file is gone.
    cx.simulate_keystrokes(window, "cmd-z");
    assert_eq!(changes.started().last(), Some(&blocked));
    changes.land(1, Ok(blocked.clone()), cx);
    cx.simulate_keystrokes(window, "cmd-z");
    assert_eq!(changes.started()[2..], [gone]);
    changes.land(2, Err("No such file or directory (os error 2)".into()), cx);
    assert_eq!(history(&panel, cx), (vec![], vec![blocked], false));
}
