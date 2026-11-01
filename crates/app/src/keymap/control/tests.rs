//! The agent's keymap ops against a store of their own: listing, binding, unbinding and
//! restoring, with the editor's checks and its care for the file.

use gpui_kit::{App, Entity, KeyBinding, TestAppContext};
use sawit_core::keymap::{BindingRef, KeySource, KeymapList, KeymapOp};
use serde_json::{Value, json};

use super::super::tests::{Scratch, runs};
use super::super::{KeymapStore, init_at, store};
use super::run;

gpui_kit::actions!(keymap_control_test, [Close, CloseAll, Other]);

/// The app's defaults for these tests: ⌘W closes and ⌘K W closes all, in `Workspace`.
fn start(cx: &mut TestAppContext, scratch: &Scratch) -> Entity<KeymapStore> {
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("cmd-w", Close, Some("Workspace")),
            KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
        ]);
        init_at(scratch.file(), cx);
        store(cx)
    })
}

fn list(cx: &mut App, op: Value) -> KeymapList {
    let op: KeymapOp = serde_json::from_value(op).unwrap();
    serde_json::from_value(run(op, cx).unwrap()).unwrap()
}

fn bind(cx: &mut App, keys: &str, action: &str) -> Result<Value, String> {
    run(
        KeymapOp::Bind {
            keys: keys.into(),
            action: action.into(),
            args: None,
            context: Some("Workspace".into()),
            replace: None,
            confirm: false,
        },
        cx,
    )
}

fn unbind(cx: &mut App, keys: &str, restore: bool) -> Result<Value, String> {
    run(
        KeymapOp::Unbind {
            keys: keys.into(),
            context: None,
            action: None,
            args: None,
            restore,
            confirm: false,
        },
        cx,
    )
}

/// The rows on `keys`, by action and where each comes from.
fn rows_on(cx: &mut App, keys: &str) -> Vec<(String, KeySource)> {
    list(cx, json!({"op": "list", "keys": keys}))
        .rows
        .into_iter()
        .filter(|row| row.keys == keys)
        .map(|row| (row.action, row.source))
        .collect()
}

/// A binding an agent makes is the user's, in keymap.json, and runs; the list shows it so,
/// with the file it is in.
#[gpui_kit::test]
fn a_binding_an_agent_makes_lists_as_the_users(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-bind");
    start(cx, &scratch);
    cx.update(|cx| {
        let answer = bind(cx, "shift-cmd-e", "keymap_control_test::Other").unwrap();
        assert_eq!(answer["bound"]["source"], "user", "{answer}");
        assert_eq!(answer["bound"]["keys"], "cmd-shift-e", "{answer}");
        assert_eq!(answer["bound"]["display"], "⇧⌘E", "{answer}");
        assert_eq!(runs(cx, "cmd-shift-e"), ["keymap_control_test::Other"]);
        assert_eq!(
            rows_on(cx, "cmd-shift-e"),
            [("keymap_control_test::Other".to_string(), KeySource::User)]
        );
        let listed = list(cx, json!({"op": "list"}));
        assert_eq!(listed.path, scratch.file());
        assert!(listed.writable && listed.errors.is_empty());
    });
    let text = std::fs::read_to_string(scratch.file()).unwrap();
    assert!(text.contains("keymap_control_test::Other"), "{text}");
}

/// A default an agent unbinds is turned off in keymap.json and lists as removed; unbinding
/// it with restore brings it back, as the editor's remove and restore buttons do.
#[gpui_kit::test]
fn an_unbound_default_lists_as_removed_until_restored(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-unbind");
    start(cx, &scratch);
    cx.update(|cx| {
        let answer = unbind(cx, "cmd-k w", false).unwrap();
        assert_eq!(
            answer["turned_off"]["action"],
            "keymap_control_test::CloseAll"
        );
        assert!(runs(cx, "cmd-k w").is_empty());
        assert_eq!(
            rows_on(cx, "cmd-k w"),
            [(
                "keymap_control_test::CloseAll".to_string(),
                KeySource::Removed
            )]
        );
        // Turned off already: there is nothing that runs there to unbind.
        let again = unbind(cx, "cmd-k w", false).unwrap_err();
        assert!(again.contains("removed"), "{again}");

        let answer = unbind(cx, "cmd-k w", true).unwrap();
        assert_eq!(
            answer["restored"]["action"],
            "keymap_control_test::CloseAll"
        );
        assert_eq!(runs(cx, "cmd-k w"), ["keymap_control_test::CloseAll"]);
        assert_eq!(
            rows_on(cx, "cmd-k w"),
            [(
                "keymap_control_test::CloseAll".to_string(),
                KeySource::Default
            )]
        );
    });
}

/// The form's checks stand for an agent too: an action that isn't one, a context that can't
/// be read or keys that aren't keystrokes are refused, and nothing is written.
#[gpui_kit::test]
fn a_binding_that_cant_be_made_writes_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-refused");
    start(cx, &scratch);
    cx.update(|cx| {
        let unknown = bind(cx, "cmd-e", "no_such::Action").unwrap_err();
        assert!(unknown.contains("Nothing was changed"), "{unknown}");
        let context = run(
            KeymapOp::Bind {
                keys: "cmd-e".into(),
                action: "keymap_control_test::Other".into(),
                args: None,
                context: Some("Workspace &&".into()),
                replace: None,
                confirm: false,
            },
            cx,
        )
        .unwrap_err();
        assert!(context.contains("can't be read"), "{context}");
        let keys = bind(cx, "cmd-nope-e", "keymap_control_test::Other").unwrap_err();
        assert!(keys.contains("aren't keystrokes"), "{keys}");
        let off = bind(cx, "cmd-e", "zed::NoAction").unwrap_err();
        assert!(off.contains("keymap_unbind"), "{off}");
        assert!(runs(cx, "cmd-e").is_empty());
    });
    assert!(!scratch.file().exists(), "keymap.json was written");
}

/// keymap.json's comments go when it is written again, so a change to a file that has some
/// is refused until the agent confirms it, and the file stays as it was. The lines a new file
/// starts with are Sawit's, and don't stop it.
#[gpui_kit::test]
fn a_keymap_with_comments_is_written_only_when_confirmed(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-comments");
    let store = start(cx, &scratch);
    std::fs::create_dir_all(&scratch.0).unwrap();
    let mine = "// My bindings.\n[{\"context\": \"Workspace\", \"bindings\": {\"ctrl-alt-h\": \"keymap_control_test::Other\"}}]\n";
    std::fs::write(scratch.file(), mine).unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| store.reload(cx));
        let refused = bind(cx, "cmd-e", "keymap_control_test::Other").unwrap_err();
        assert!(refused.contains("comments"), "{refused}");
        assert!(refused.contains("confirm: true"), "{refused}");
        let refused = unbind(cx, "cmd-w", false).unwrap_err();
        assert!(refused.contains("comments"), "{refused}");
    });
    assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), mine);
    cx.update(|cx| {
        run(
            KeymapOp::Bind {
                keys: "cmd-e".into(),
                action: "keymap_control_test::Other".into(),
                args: None,
                context: Some("Workspace".into()),
                replace: None,
                confirm: true,
            },
            cx,
        )
        .unwrap();
        assert_eq!(runs(cx, "cmd-e"), ["keymap_control_test::Other"]);
        assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_control_test::Other"]);
    });
    let text = std::fs::read_to_string(scratch.file()).unwrap();
    assert!(!text.contains("My bindings"), "{text}");
}

/// The lines a new keymap.json starts with, on how to write it, are Sawit's: a change goes
/// through without asking, as the editor's does.
#[gpui_kit::test]
fn a_new_keymap_files_own_lines_dont_stop_a_change(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-new-file");
    let store = start(cx, &scratch);
    cx.update(|cx| {
        store.read(cx).ensure_file().unwrap();
        store.update(cx, |store, cx| store.reload(cx));
        bind(cx, "cmd-e", "keymap_control_test::Other").unwrap();
        assert_eq!(runs(cx, "cmd-e"), ["keymap_control_test::Other"]);
    });
}

/// The keystroke search finds the chords that start with the keys, and the filter finds a
/// binding by the words of its name, as the editor's do.
#[gpui_kit::test]
fn the_list_finds_bindings_by_keys_and_by_name(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-list");
    start(cx, &scratch);
    cx.update(|cx| {
        let chords = list(cx, json!({"op": "list", "keys": "cmd-k"}));
        let keys: Vec<&str> = chords.rows.iter().map(|row| row.keys.as_str()).collect();
        assert_eq!(keys, ["cmd-k w"]);
        let named = list(cx, json!({"op": "list", "filter": "Close ALL"}));
        let actions: Vec<&str> = named.rows.iter().map(|row| row.action.as_str()).collect();
        assert_eq!(actions, ["keymap_control_test::CloseAll"]);
        assert_eq!(named.rows[0].context.as_deref(), Some("Workspace"));
        // Actions no key runs show only when asked.
        let unbound = list(
            cx,
            json!({"op": "list", "filter": "keymap_control_test other", "unbound": true}),
        );
        assert_eq!(unbound.rows.len(), 1, "{:?}", unbound.rows);
        assert_eq!(unbound.rows[0].source, KeySource::Unbound);
        let bad = run(
            serde_json::from_value(json!({"op": "list", "keys": "cmd-nope-k"})).unwrap(),
            cx,
        )
        .unwrap_err();
        assert!(bad.contains("aren't keystrokes"), "{bad}");
    });
}

/// A binding of keys another binding has in the same context stays beside it, runs first,
/// and the answer says so, as the form does; the list marks both as conflicting.
#[gpui_kit::test]
fn a_bind_says_what_it_clashes_with(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-clash");
    start(cx, &scratch);
    cx.update(|cx| {
        let answer = bind(cx, "cmd-w", "keymap_control_test::Other").unwrap();
        let clashes = answer["clashes"].to_string();
        assert!(clashes.contains("also bound to"), "{answer}");
        assert!(clashes.contains("keymap control test: close"), "{answer}");
        assert_eq!(answer["bound"]["conflict"], true, "{answer}");
        assert_eq!(
            runs(cx, "cmd-w"),
            ["keymap_control_test::Other", "keymap_control_test::Close"]
        );
        let chord = bind(cx, "cmd-k", "keymap_control_test::Other").unwrap();
        assert!(
            chord["clashes"].to_string().contains("also starts"),
            "{chord}"
        );
    });
}

/// `replace` changes the binding it names, as the form's Edit does: a default moves to the
/// new keys (turned off where it was), and the answer names both.
#[gpui_kit::test]
fn replace_moves_a_binding_to_new_keys(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-replace");
    start(cx, &scratch);
    cx.update(|cx| {
        let answer = run(
            KeymapOp::Bind {
                keys: "cmd-e".into(),
                action: "keymap_control_test::Close".into(),
                args: None,
                context: Some("Workspace".into()),
                replace: Some(Box::new(BindingRef {
                    keys: "cmd-w".into(),
                    context: Some("Workspace".into()),
                    action: "keymap_control_test::Close".into(),
                    args: None,
                })),
                confirm: false,
            },
            cx,
        )
        .unwrap();
        assert_eq!(answer["replaced"]["keys"], "cmd-w", "{answer}");
        assert_eq!(answer["bound"]["keys"], "cmd-e", "{answer}");
        assert_eq!(runs(cx, "cmd-e"), ["keymap_control_test::Close"]);
        assert!(runs(cx, "cmd-w").is_empty());
        assert_eq!(
            rows_on(cx, "cmd-w"),
            [("keymap_control_test::Close".to_string(), KeySource::Removed)]
        );
    });
}

/// Keys several bindings have are named with their context or action: unnamed, the unbind
/// is refused and lists them; named, it takes the one meant.
#[gpui_kit::test]
fn an_unbind_of_keys_bound_twice_needs_the_one_meant(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-twice");
    start(cx, &scratch);
    cx.update(|cx| {
        run(
            KeymapOp::Bind {
                keys: "cmd-w".into(),
                action: "keymap_control_test::Other".into(),
                args: None,
                context: Some("Editor".into()),
                replace: None,
                confirm: false,
            },
            cx,
        )
        .unwrap();
        let vague = unbind(cx, "cmd-w", false).unwrap_err();
        assert!(vague.contains("Several bindings"), "{vague}");
        assert!(
            vague.contains("Workspace") && vague.contains("Editor"),
            "{vague}"
        );
        let answer = run(
            KeymapOp::Unbind {
                keys: "cmd-w".into(),
                context: Some("Editor".into()),
                action: None,
                args: None,
                restore: false,
                confirm: false,
            },
            cx,
        )
        .unwrap();
        assert_eq!(answer["removed"]["source"], "user", "{answer}");
        assert_eq!(runs(cx, "cmd-w"), ["keymap_control_test::Close"]);
        let nothing = unbind(cx, "ctrl-alt-q", false).unwrap_err();
        assert!(nothing.contains("Nothing is bound"), "{nothing}");
    });
}

/// While keymap.json can't be read, a change would write over what the user is in the middle
/// of writing: it is refused, and the file stays as it is. A file another program changed
/// since the keymap last read it is read again, and the change must be asked for again.
#[gpui_kit::test]
fn a_change_never_writes_over_what_it_hasnt_read(cx: &mut TestAppContext) {
    let scratch = Scratch::new("agent-unreadable");
    let store = start(cx, &scratch);
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(scratch.file(), "[{").unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| store.reload(cx));
        let refused = bind(cx, "cmd-e", "keymap_control_test::Other").unwrap_err();
        assert!(refused.contains("can't be read"), "{refused}");
        let listed = list(cx, json!({"op": "list"}));
        assert!(!listed.writable && listed.errors.len() == 1, "{listed:?}");
    });
    assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), "[{");

    std::fs::write(scratch.file(), "[]").unwrap();
    cx.update(|cx| store.update(cx, |store, cx| store.reload(cx)));
    let by_hand =
        r#"[{"context": "Workspace", "bindings": {"ctrl-alt-h": "keymap_control_test::Other"}}]"#;
    std::fs::write(scratch.file(), by_hand).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(scratch.file())
        .unwrap()
        .set_modified(later)
        .unwrap();
    cx.update(|cx| {
        let refused = bind(cx, "cmd-e", "keymap_control_test::Other").unwrap_err();
        assert!(refused.contains("changed outside"), "{refused}");
        assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_control_test::Other"]);
    });
    assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), by_hand);
}
