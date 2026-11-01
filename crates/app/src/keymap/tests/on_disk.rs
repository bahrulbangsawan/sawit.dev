//! The keymap file on disk: written by hand or by another program, read as it is, and written
//! back without losing what is there, its permissions, or a link.

use gpui_kit::{Entity, TestAppContext};

use super::{Scratch, default_row, runs, start};
use crate::keymap::{KeymapStore, Source};

#[gpui_kit::test]
fn the_file_written_by_hand_binds_over_the_defaults(cx: &mut TestAppContext) {
    let scratch = Scratch::new("by-hand");
    let store = start(cx, &scratch);
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(
            scratch.file(),
            r#"// Mine.
            [{"context": "Workspace", "bindings": {"cmd-w": "keymap_test::Other", "cmd-k w": null}}]"#,
        )
        .unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| store.reload(cx));
        // The user's binding runs first; the default still runs if it doesn't take the key.
        assert_eq!(
            runs(cx, "cmd-w"),
            ["keymap_test::Other", "keymap_test::Close"]
        );
        assert!(runs(cx, "cmd-k w").is_empty());
        assert_eq!(store.read(cx).errors().count(), 0);
    });
    // A file that can't be read keeps the bindings it had, and says why; the editor won't
    // write over it.
    std::fs::write(scratch.file(), "[{").unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| store.reload(cx));
        assert_eq!(
            runs(cx, "cmd-w"),
            ["keymap_test::Other", "keymap_test::Close"]
        );
        assert_eq!(store.read(cx).errors().count(), 1);
        assert!(!store.read(cx).can_write());
    });
}

/// A default the file turns off shows as removed however the file does it: with `null` (the
/// new file's own example), or with an unbind whose context covers the default's (none at
/// all, or the same written another way), as GPUI reads them.
#[gpui_kit::test]
fn a_default_turned_off_by_hand_shows_as_removed(cx: &mut TestAppContext) {
    let scratch = Scratch::new("off-by-hand");
    let store = start(cx, &scratch);
    std::fs::create_dir_all(&scratch.0).unwrap();
    for file in [
        r#"[{"context": "Workspace", "bindings": {"cmd-k w": null}}]"#,
        r#"[{"bindings": {"cmd-k w": null}}]"#,
        r#"[{"bindings": {"cmd-k w": ["zed::Unbind", "keymap_test::CloseAll"]}}]"#,
        r#"[{"context": "Workspace || Editor", "bindings": {"cmd-k w": ["zed::Unbind", "keymap_test::CloseAll"]}}]"#,
    ] {
        std::fs::write(scratch.file(), file).unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| store.reload(cx));
            assert!(runs(cx, "cmd-k w").is_empty(), "{file}");
            let source = default_row(&store, cx, "keymap_test::CloseAll");
            assert!(
                matches!(source, Source::Removed { .. }),
                "{file}: {source:?}"
            );
            // Restore takes the line out.
            store.update(cx, |s, cx| s.remove(source, cx)).unwrap();
            assert_eq!(runs(cx, "cmd-k w"), ["keymap_test::CloseAll"], "{file}");
        });
    }
    // A null in another context leaves the default as it is.
    std::fs::write(
        scratch.file(),
        r#"[{"context": "Editor", "bindings": {"cmd-k w": null}}]"#,
    )
    .unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| store.reload(cx));
        let source = default_row(&store, cx, "keymap_test::CloseAll");
        assert!(matches!(source, Source::Default(_)), "{source:?}");
    });
}

/// Writes `text` to the keymap file as another program would, with a modification time the
/// store can't have seen.
fn edit_by_hand(scratch: &Scratch, text: &str) {
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(scratch.file(), text).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(scratch.file())
        .unwrap()
        .set_modified(later)
        .unwrap();
}

/// A change made in the editor before its once-a-second look at the file never writes over
/// what another program saved there meanwhile: the store reads the file again and asks for
/// the change again, with the file as it is now.
#[gpui_kit::test]
fn an_edit_on_disk_is_never_written_over(cx: &mut TestAppContext) {
    let scratch = Scratch::new("edited-on-disk");
    let store = start(cx, &scratch);
    cx.update(|cx| {
        store
            .update(cx, |s, cx| {
                s.add(
                    Some("Workspace"),
                    "ctrl-alt-a",
                    "keymap_test::Other",
                    None,
                    cx,
                )
            })
            .unwrap();
    });
    let by_hand = r#"[{"context": "Workspace", "bindings": {"ctrl-alt-h": "keymap_test::Other"}}]"#;
    edit_by_hand(&scratch, by_hand);
    cx.update(|cx| {
        let result = store.update(cx, |s, cx| {
            s.add(
                Some("Workspace"),
                "ctrl-alt-b",
                "keymap_test::CloseAll",
                None,
                cx,
            )
        });
        assert!(result.is_err(), "{result:?}");
        // The store has read the hand edit, and binds it.
        assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_test::Other"]);
        assert!(runs(cx, "ctrl-alt-a").is_empty());
    });
    assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), by_hand);
    // Asked again, the change goes in beside the hand edit.
    cx.update(|cx| {
        store
            .update(cx, |s, cx| {
                s.add(
                    Some("Workspace"),
                    "ctrl-alt-b",
                    "keymap_test::CloseAll",
                    None,
                    cx,
                )
            })
            .unwrap();
        assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_test::Other"]);
        assert_eq!(runs(cx, "ctrl-alt-b"), ["keymap_test::CloseAll"]);
    });
}

/// A keymap.json that is a link (to a dotfiles folder) stays a link: the editor writes the
/// file it points to.
#[gpui_kit::test]
fn a_linked_keymap_file_stays_a_link(cx: &mut TestAppContext) {
    let scratch = Scratch::new("linked");
    let dotfiles = scratch.0.join("dotfiles");
    std::fs::create_dir_all(&dotfiles).unwrap();
    let target = dotfiles.join("sawit-keymap.json");
    std::fs::write(&target, "[]").unwrap();
    std::os::unix::fs::symlink(&target, scratch.file()).unwrap();
    let store = start(cx, &scratch);
    cx.update(|cx| {
        store
            .update(cx, |s, cx| {
                s.add(
                    Some("Workspace"),
                    "ctrl-alt-l",
                    "keymap_test::Other",
                    None,
                    cx,
                )
            })
            .unwrap();
    });
    let link = std::fs::symlink_metadata(scratch.file()).unwrap();
    assert!(
        link.file_type().is_symlink(),
        "keymap.json is no longer a link"
    );
    assert!(
        std::fs::read_to_string(&target)
            .unwrap()
            .contains("ctrl-alt-l")
    );
}

/// Adds a binding of ⌃⌥`key` to `Other`, as the Keymap Editor does, which writes the file.
fn add_binding(cx: &mut TestAppContext, store: &Entity<KeymapStore>, key: &str) {
    let keys = format!("ctrl-alt-{key}");
    cx.update(|cx| {
        store
            .update(cx, |s, cx| {
                s.add(Some("Workspace"), &keys, "keymap_test::Other", None, cx)
            })
            .unwrap();
    });
}

/// Seams m6: keymap.json is written as every file Sawit saves is (`files::write_atomic`): it
/// keeps its permissions. The keymap's own write made a new file in its place, 0644 whatever
/// the user had set.
#[gpui_kit::test]
fn the_keymap_file_keeps_its_permissions(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = Scratch::new("mode");
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(scratch.file(), "[]").unwrap();
    let owner_only = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(scratch.file(), owner_only).unwrap();
    let store = start(cx, &scratch);
    add_binding(cx, &store, "m");
    let file = std::fs::symlink_metadata(scratch.file()).unwrap();
    assert!(file.is_file(), "keymap.json isn't a file");
    assert_eq!(
        file.permissions().mode() & 0o777,
        0o600,
        "keymap.json's permissions changed"
    );
    assert!(
        std::fs::read_to_string(scratch.file())
            .unwrap()
            .contains("ctrl-alt-m")
    );
}

/// Seams m6: the write goes through a staging file of its own, made new beside keymap.json,
/// never through what is already at a name beside it. The keymap's own write staged at a
/// fixed `keymap.json.tmp` with `fs::write`, which follows a link there: it wrote the keymap
/// into the file the link points to, then made keymap.json that link.
#[gpui_kit::test]
fn the_keymap_file_is_never_written_through_a_link_beside_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new("staged");
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(scratch.file(), "[]").unwrap();
    let elsewhere = scratch.0.join("elsewhere.json");
    std::fs::write(&elsewhere, "not the keymap").unwrap();
    let link = scratch.0.join("keymap.json.tmp");
    std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
    let store = start(cx, &scratch);
    add_binding(cx, &store, "s");
    assert_eq!(
        std::fs::read_to_string(&elsewhere).unwrap(),
        "not the keymap",
        "the keymap was written through the link beside it"
    );
    assert!(
        !std::fs::symlink_metadata(scratch.file())
            .unwrap()
            .file_type()
            .is_symlink(),
        "keymap.json became a link"
    );
    assert!(
        std::fs::read_to_string(scratch.file())
            .unwrap()
            .contains("ctrl-alt-s")
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), elsewhere);
}

#[gpui_kit::test]
fn a_binding_that_cant_load_is_named(cx: &mut TestAppContext) {
    let scratch = Scratch::new("bad-binding");
    let store = start(cx, &scratch);
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(
        scratch.file(),
        r#"[{"bindings": {"cmd-e": "no_such::Action", "cmd-r": "keymap_test::Other"}}]"#,
    )
    .unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| store.reload(cx));
        let errors: Vec<String> = store.read(cx).errors().map(String::from).collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("cmd-e"), "{errors:?}");
        // The others load.
        assert_eq!(runs(cx, "cmd-r"), ["keymap_test::Other"]);
    });
}
