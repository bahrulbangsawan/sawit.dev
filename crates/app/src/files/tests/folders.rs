//! Tests of the folder the panel shows: changing it, reading it, and what a change leaves behind.

use std::path::{Path, PathBuf};

use gpui_kit::{Point, TestAppContext, point, px};
use sawit_core::files::Entry;

use super::{
    Changes, OTHER_ROOT, ROOT, ask, events, file_actions, focus, open_panel, show, show_tree,
};
use crate::files::tree::read_tree;
use crate::files::{Clip, Op, Scratch};

/// The folder picked with the tab's Open Project Folder button goes to the workspace, which
/// shows it where it is open already, or puts it in this project's place (FILES-9). The tab
/// used to take it itself, past the workspace's checks: a folder another project of the
/// window had became a second project of it, and the old project's tabs kept its group.
#[gpui_kit::test]
fn open_project_folder_asks_the_workspace(cx: &mut TestAppContext) {
    let (_window, panel) = open_panel(cx, &["notes.txt"]);
    Changes::install(&panel, cx);
    let (seen, _events) = events(&panel, cx);
    cx.update(|cx| panel.update(cx, |p, cx| p.prompt_open(cx)));
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from(OTHER_ROOT)]));
    cx.run_until_parked();
    let other = Path::new(OTHER_ROOT).file_name().unwrap().to_string_lossy();
    assert_eq!(*seen.borrow(), [format!("open folder {other}")]);
    cx.update(|cx| assert_eq!(panel.read(cx).root.as_deref(), Some(Path::new(ROOT))));
}

/// The tree's Git items follow the project's repository as the Git store has it (FILES-15):
/// one made with Initialize Repository, or `git init` in the terminal once the Git store has
/// noticed. The tree used to look the repository up once per folder, so Add to .gitignore,
/// Add to .git/info/exclude and View History stayed off until the folder was opened again,
/// and on once `.git` was deleted.
#[gpui_kit::test]
fn the_git_items_follow_the_projects_repository(cx: &mut TestAppContext) {
    let (_window, panel) = open_panel(cx, &["notes.txt"]);
    let git = |cx: &mut TestAppContext| cx.update(|cx| panel.read(cx).menu_state().git);
    assert!(!git(cx));
    cx.update(|cx| panel.update(cx, |p, cx| p.set_repo(Some(PathBuf::from(ROOT)), cx)));
    assert!(git(cx), "a repository was made");
    cx.update(|cx| panel.update(cx, |p, cx| p.set_repo(None, cx)));
    assert!(!git(cx), "the repository was deleted");
}

/// Work started in one folder that lands once the panel shows another leaves that one alone
/// (FILES-3). A change's undo and selection, an error, Expand All's folders and a folder's
/// listing of the old folder all went to the new one (its repository too, which now comes
/// from the project's Git store): ⌫ and ⌘Z then trashed the old folder's copy, unseen. A
/// move that landed is still told, for the tabs of what moved to follow it. A change asked
/// for in the old folder doesn't run.
#[gpui_kit::test]
fn work_that_lands_after_a_folder_change_leaves_the_new_folder_alone(cx: &mut TestAppContext) {
    let (window, panel) = open_panel(cx, &["notes.txt", "src"]);
    let changes = Changes::install(&panel, cx);
    let (seen, _events) = events(&panel, cx);
    let root = Path::new(ROOT);
    let (notes, kept) = (root.join("notes.txt"), root.join("kept.txt"));
    let rename = Op::Move {
        from: notes.clone(),
        to: kept.clone(),
    };
    ask(cx, window, &panel, rename);
    let duplicate = Op::Copy {
        from: notes.clone(),
        to: root.join("notes copy.txt"),
    };
    ask(cx, window, &panel, duplicate);
    let old = cx.update(|cx| {
        panel.update(cx, |p, _| {
            let old = p.folder;
            // What set_root does, but for reading the new folder.
            p.forget_folder();
            show(p, Path::new(OTHER_ROOT), &["readme.md"]);
            old
        })
    });
    let undo = Op::Move {
        from: kept.clone(),
        to: notes.clone(),
    };
    changes.land(0, Ok(undo), cx);
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            let entry = Entry {
                name: "lib.rs".into(),
                path: root.join("src/lib.rs"),
                is_dir: false,
            };
            p.listed(old, root.join("src"), Ok(vec![entry]), cx);
            p.expanded_all(old, vec![(root.join("src"), Vec::new())], cx);
            p.failed(old, "“notes.txt” couldn't be renamed.".into(), cx);
        })
    });

    assert_eq!(changes.started().len(), 1, "the duplicate ran");
    assert_eq!(*seen.borrow(), ["moved notes.txt kept.txt"]);
    // Should the old folder's entry be selected all the same, nothing acts on it.
    cx.update(|cx| {
        panel.update(cx, |p, _| {
            p.selected = Some(kept.clone());
            assert_eq!((p.trash_target(), p.selection()), (None, None));
            assert_eq!(p.target().as_deref(), Some(Path::new(OTHER_ROOT)));
            p.selected = None;
        })
    });
    cx.update(|cx| {
        let p = panel.read(cx);
        assert!(p.undo.is_empty() && p.redo.is_empty() && !p.busy);
        assert_eq!((&p.selected, &p.error, &p.repo), (&None, &None, &None));
        let other = Path::new(OTHER_ROOT).to_path_buf();
        assert_eq!(p.expanded.iter().collect::<Vec<_>>(), [&other]);
        assert_eq!(p.children.keys().collect::<Vec<_>>(), [&other]);
    });
}

/// Expand All doesn't follow links to folders (FILES-10): a link to a folder above (`up ->
/// ..`) took it out of the project, and one to its own folder (`loop -> .`) opened the same
/// folder inside itself again and again, until 2000 folders were open, each read again
/// every 3 seconds. Linked folders still open by hand.
#[test]
fn expand_all_leaves_linked_folders_closed() {
    let dir = Scratch::new("expand-all-links");
    let root = dir.0.as_path();
    std::fs::create_dir(root.join("a")).unwrap();
    std::os::unix::fs::symlink("..", root.join("a/up")).unwrap();
    std::os::unix::fs::symlink(".", root.join("loop")).unwrap();
    let tree = read_tree(root.to_path_buf());
    let read: Vec<&Path> = tree.iter().map(|(dir, _)| dir.as_path()).collect();
    assert_eq!(read, [root, &root.join("a")]);
    // Both links are still listed, as folders.
    assert!(tree[0].1.iter().any(|e| e.name == "loop" && e.is_dir));
}

/// The 3-second refresh reads every open folder in one go, off the UI thread, and a read
/// that fails but for a while (too many files open at once) leaves what was read before
/// (FILES-10). It used to start one read per folder at once, up to 2000 of them, and took
/// any error for the folder being deleted: the folder closed, and the project folder showed
/// "Unable to read this folder".
#[gpui_kit::test]
fn the_refresh_reads_open_folders_at_once_and_keeps_them_through_errors(cx: &mut TestAppContext) {
    let (_window, panel) = open_panel(cx, &[]);
    let changes = Changes::install(&panel, cx);
    let root = Path::new(ROOT);
    cx.update(|cx| panel.update(cx, |p, _| show_tree(p, root)));
    cx.update(|cx| panel.update(cx, |p, cx| p.refresh(cx)));
    let reads = changes.reads();
    assert_eq!(reads.len(), 1, "one read per folder: {reads:?}");
    let mut read = reads[0].clone();
    read.sort();
    assert_eq!(read, [root, &root.join("src"), &root.join("src/lib")]);

    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            let folder = p.folder;
            // Too many files open: what was read stays.
            for dir in [root.join("src"), root.to_path_buf()] {
                // EMFILE, "Too many open files".
                let busy = std::io::Error::from_raw_os_error(24);
                p.listed(folder, dir.clone(), Err(busy.into()), cx);
                assert!(p.children.contains_key(&dir) && p.expanded.contains(&dir));
            }
            assert_eq!(p.root_error, None);
            // Gone: forgotten.
            let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
            p.listed(folder, root.join("src/lib"), Err(gone.into()), cx);
            assert!(!p.children.contains_key(&root.join("src/lib")));
            assert!(!p.expanded.contains(&root.join("src/lib")));
        })
    });
}

/// ⌫ after the panel changes folder (its Open Project Folder button, or `sawit -r`) moves
/// nothing to the Trash, and ⌘V pastes nothing. No row of the new folder's tree shows the
/// previous folder's selected or cut entry, and Trash and Paste used to act on them all the
/// same. The new tree also starts at its top, with no error from the previous folder.
#[gpui_kit::test]
fn backspace_after_a_folder_change_trashes_nothing(cx: &mut TestAppContext) {
    let (window, panel) = open_panel(cx, &["notes.txt"]);
    let file = Path::new(ROOT).join("notes.txt");
    cx.update(|cx| {
        panel.update(cx, |p, _| {
            p.selected = Some(file.clone());
            p.clip = Some(Clip {
                path: file.clone(),
                cut: true,
            });
            p.error = Some("“notes.txt” couldn't be renamed.".into());
            p.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), px(-66.)));
        })
    });
    // What set_root does, but for reading the new folder, which needs the runtime that
    // open_panel leaves out.
    let offset = cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.forget_folder();
            // Read before a frame is drawn: drawing a tree shorter than the panel also
            // brings it to the top, and a longer one keeps the offset.
            let offset = p.scroll.0.borrow().base_handle.offset();
            show(p, Path::new(OTHER_ROOT), &["readme.md"]);
            cx.notify();
            offset
        })
    });
    let (ran, _observer) = file_actions(cx);
    let tree = cx.update(|cx| panel.read(cx).focus.clone());
    focus(cx, window, &tree);
    // A Trash or a Paste this starts fails the test at once: there is no runtime.
    cx.simulate_keystrokes(window, "backspace cmd-v");
    assert_eq!(*ran.borrow(), ["files::Trash", "files::Paste"]);
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.root.as_deref(), Some(Path::new(OTHER_ROOT)));
        assert_eq!(p.selected, None);
        assert!(p.clip.is_none() && p.undo.is_empty());
        assert_eq!(p.error, None);
    });
    assert_eq!(offset, Point::default(), "the new tree starts at its top");
}
