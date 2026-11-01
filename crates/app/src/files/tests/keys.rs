//! Tests of the tree's keys: what they act on, ⌫ and the Trash, and keys while a name is typed.

use std::path::{Path, PathBuf};

use gpui_kit::{Action, AppContext as _, TestAppContext};

use super::{Changes, ROOT, ask, file_actions, focus, open_panel, select, show_tree, typed};
use crate::files::{EditKind, Op, Trash};

/// Keys act only on an entry whose row is shown (FILES-6). The selection used to outlive its
/// row: after a Trash, ⌫ again tried the same entry ("couldn't be moved to the Trash");
/// after Collapse All, ⌫ trashed a file in a folder no longer open, unseen; with the
/// project folder unreadable, its entries could still be renamed, F2's name box drawn
/// nowhere, with the keyboard.
#[gpui_kit::test]
fn keys_act_only_on_an_entry_whose_row_is_shown(cx: &mut TestAppContext) {
    let (window, panel) = open_panel(cx, &[]);
    let changes = Changes::install(&panel, cx);
    let root = Path::new(ROOT);
    let (src, x) = (root.join("src"), root.join("src/lib/x.rs"));
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            show_tree(p, root);
            cx.notify();
        })
    });
    cx.run_until_parked();

    // A Trash of the selected file lands: nothing is selected now.
    select(cx, window, &panel, &x);
    ask(cx, window, &panel, Op::Trash(x.clone()));
    let back = Op::Restore {
        from: PathBuf::from("/Trash/x.rs"),
        to: x.clone(),
    };
    changes.land(0, Ok(back), cx);
    cx.update(|cx| assert_eq!(panel.read(cx).selected, None));

    // Collapse All, with the file selected: the folder it is in, still shown, is selected.
    select(cx, window, &panel, &x);
    cx.update(|cx| panel.update(cx, |p, cx| p.collapse_all(cx)));
    cx.update(|cx| assert_eq!(panel.read(cx).selected.as_ref(), Some(&src)));

    // An entry selected whose row isn't shown: F2 and ⌘D do nothing.
    select(cx, window, &panel, &x);
    cx.simulate_keystrokes(window, "f2 cmd-d");
    cx.update(|cx| {
        let p = panel.read(cx);
        assert!(p.edit.is_none(), "a name box nowhere to be seen");
        assert_eq!((p.selection(), p.trash_target()), (None, None));
    });
    assert_eq!(changes.started().len(), 1, "a duplicate ran");

    // The project folder can't be read: none of its entries is shown.
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.selected = Some(root.join("notes.txt"));
            assert!(p.selection().is_some());
            let unreadable = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
            p.listed(p.folder, root.to_path_buf(), Err(unreadable.into()), cx);
            assert_eq!(p.selection(), None);
        })
    });
}

/// ⌫ while a name is typed in the tree edits the name and runs none of the panel's commands.
/// The name box passes on ⌫ with nothing before the cursor, which a new name has at once and
/// a rename once the start of the name is reached, to the panel's bindings, where Trash would
/// move the selected entry to the Trash without asking.
#[gpui_kit::test]
fn backspace_while_naming_trashes_nothing(cx: &mut TestAppContext) {
    let (window, panel) = open_panel(cx, &["notes.txt"]);
    let file = Path::new(ROOT).join("notes.txt");
    cx.update(|cx| panel.update(cx, |p, _| p.selected = Some(file.clone())));
    let (ran, _observer) = file_actions(cx);
    let tree = cx.update(|cx| panel.read(cx).focus.clone());
    let name = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let p = panel.read(cx);
            p.edit
                .as_ref()
                .map(|e| e.input.read(cx).value().to_string())
        })
    };

    // New File: the name box starts empty.
    focus(cx, window, &tree);
    cx.simulate_keystrokes(window, "cmd-n");
    assert_eq!(name(cx).as_deref(), Some(""));
    cx.simulate_keystrokes(window, "backspace");
    assert_eq!(*ran.borrow(), ["files::NewFile"], "⌫ in a new name");

    // Rename: the name without its extension is selected; the first ⌫ deletes it, and the
    // second is at the start of the name.
    focus(cx, window, &tree);
    cx.simulate_keystrokes(window, "f2");
    assert_eq!(name(cx).as_deref(), Some("notes.txt"));
    cx.simulate_keystrokes(window, "backspace backspace");
    assert_eq!(name(cx).as_deref(), Some(".txt"));
    assert_eq!(
        *ran.borrow(),
        ["files::NewFile", "files::Rename"],
        "⌫ at the start of a name"
    );

    // From the tree, ⌫ is still Trash, which acts on the selected file, and the right-click
    // menu still finds its key. The Trash itself doesn't run: with nothing selected it acts
    // on nothing (the next test), so this ⌫ starts no file change.
    focus(cx, window, &tree);
    assert_eq!(name(cx), None, "leaving the name box ends the edit");
    cx.update(|cx| assert_eq!(panel.read(cx).trash_target(), Some(file.clone())));
    cx.update(|cx| panel.update(cx, |p, _| p.selected = None));
    cx.simulate_keystrokes(window, "backspace");
    assert_eq!(
        *ran.borrow(),
        ["files::NewFile", "files::Rename", "files::Trash"]
    );
    let key = cx
        .update_window(window, |_, window, _| {
            window
                .highest_precedence_binding_for_action_in(&Trash as &dyn Action, &tree)
                .map(|b| b.keystrokes()[0].key().to_string())
        })
        .unwrap();
    assert_eq!(key.as_deref(), Some("backspace"));
}

/// ⌫ asks before it moves the selected file or folder to the Trash; ⌘⌫ doesn't (FILES-1).
/// The tree takes the keyboard on a click, which nothing shows, and reopening a file whose
/// tab is open used to leave it there: ⌫ meant for the editor then moved the file, or a
/// whole folder, to the Trash without a word. Zed's project panel, whose keys these are,
/// asks too; ⌘⌫ is Finder's Move to Trash.
#[gpui_kit::test]
fn backspace_asks_before_moving_anything_to_the_trash(cx: &mut TestAppContext) {
    let (window, panel) = open_panel(cx, &["notes.txt"]);
    let changes = Changes::install(&panel, cx);
    let notes = Path::new(ROOT).join("notes.txt");
    select(cx, window, &panel, &notes);

    cx.simulate_keystrokes(window, "backspace");
    assert_eq!(changes.started(), [], "moved to the Trash without asking");
    let (question, _) = cx.pending_prompt().expect("⌫ asks");
    assert_eq!(question, "Move “notes.txt” to the Trash?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(changes.started(), []);

    cx.simulate_keystrokes(window, "backspace");
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert_eq!(changes.started(), [Op::Trash(notes.clone())]);
    let back = Op::Restore {
        from: PathBuf::from("/Trash/notes.txt"),
        to: notes.clone(),
    };
    changes.land(0, Ok(back), cx);

    select(cx, window, &panel, &notes);
    cx.simulate_keystrokes(window, "cmd-backspace");
    assert!(!cx.has_pending_prompt(), "⌘⌫ asked");
    assert_eq!(changes.started()[1..], [Op::Trash(notes)]);
}

/// While a name is typed, the tree's keys leave it alone (FILES-13). F2 started a rename
/// over it, which dropped the name typed; ⌘N and ⌥⌘N started the name again; ⌘D duplicated
/// the selected entry and ⌃⇧↩ opened it, the name box still open.
#[gpui_kit::test]
fn keys_while_a_name_is_typed_run_no_tree_command(cx: &mut TestAppContext) {
    let (window, panel) = open_panel(cx, &["notes.txt"]);
    let changes = Changes::install(&panel, cx);
    select(cx, window, &panel, &Path::new(ROOT).join("notes.txt"));
    let (ran, _observer) = file_actions(cx);
    cx.simulate_keystrokes(window, "cmd-n");
    cx.update_window(window, |_, window, cx| {
        let input = panel.read(cx).edit.as_ref().unwrap().input.clone();
        input.update(cx, |s, cx| s.set_value("draft", window, cx));
    })
    .unwrap();
    cx.simulate_keystrokes(window, "f2 cmd-n alt-cmd-n cmd-d");
    assert_eq!(*ran.borrow(), ["files::NewFile"]);
    // ⌃⇧↩ would open the file in another app, which a test can't.
    cx.simulate_keystrokes(
        window,
        "ctrl-shift-enter alt-cmd-r alt-cmd-c alt-cmd-shift-f",
    );
    assert_eq!(*ran.borrow(), ["files::NewFile"]);
    assert_eq!(changes.started(), []);
    assert_eq!(typed(&panel, cx).as_deref(), Some("draft"));
    cx.update(|cx| {
        let edit = panel.read(cx).edit.as_ref().map(|e| e.kind.clone());
        assert!(matches!(edit, Some(EditKind::New { dir: false })));
    });
}

/// ⌫ in the tree with nothing selected moves nothing to the Trash. Trash used to fall back
/// to the project folder, as the commands that show, open or copy a path do, and moved the
/// whole project to the Trash without asking. Delete, the other command that removes, has
/// always needed a selection.
#[gpui_kit::test]
fn backspace_with_nothing_selected_leaves_the_project_folder(cx: &mut TestAppContext) {
    let (window, panel) = open_panel(cx, &["notes.txt"]);
    let (ran, _observer) = file_actions(cx);
    let tree = cx.update(|cx| panel.read(cx).focus.clone());
    focus(cx, window, &tree);
    cx.update(|cx| assert_eq!(panel.read(cx).trash_target(), None));
    // A Trash this starts fails the test at once: open_panel installs no runtime.
    cx.simulate_keystrokes(window, "backspace");
    assert_eq!(*ran.borrow(), ["files::Trash"]);
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.root.as_deref(), Some(Path::new(ROOT)));
        assert!(p.error.is_none() && p.undo.is_empty());
    });
}
