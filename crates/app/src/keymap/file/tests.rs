//! The keymap file read in Zed's format (comments, trailing commas, unbinds), written back, and
//! changed binding by binding.

use super::*;

#[test]
fn reads_zeds_format() {
    let keymap = UserKeymap::parse(
        r#"
            // Mine.
            [
              { "bindings": { "cmd-q": null } },
              {
                "context": "Workspace", /* the window */
                "bindings": {
                  "cmd-shift-w": "workspace::CloseAllTabs",
                  "cmd-k e": ["zed::Unbind", "workspace::CloseTabsLeft"],
                  "ctrl-1": ["board::Command", { "id": "tool.move" }],
                  "ctrl-2": ["workspace::ShowFiles"],
                },
              },
            ]
            "#,
    )
    .unwrap();
    assert_eq!(keymap.sections.len(), 2);
    assert_eq!(keymap.sections[0].context, None);
    assert_eq!(keymap.sections[0].bindings[0].entry, Entry::Disabled);
    let workspace = &keymap.sections[1];
    assert_eq!(workspace.context.as_deref(), Some("Workspace"));
    let entries: Vec<_> = workspace.bindings.iter().map(|b| &b.entry).collect();
    assert_eq!(
        entries,
        [
            &Entry::action("workspace::CloseAllTabs", None),
            &Entry::unbind("workspace::CloseTabsLeft"),
            &Entry::action("board::Command", Some(json!({"id": "tool.move"}))),
            &Entry::action("workspace::ShowFiles", None),
        ]
    );
    assert_eq!(
        workspace.bindings[1].entry.unbinds(),
        Some("workspace::CloseTabsLeft")
    );
}

#[test]
fn comments_inside_strings_stay() {
    let keymap =
        UserKeymap::parse(r#"[{"context": "a // b", "bindings": {"/": "x::Y"}}]"#).unwrap();
    assert_eq!(keymap.sections[0].context.as_deref(), Some("a // b"));
    assert_eq!(keymap.sections[0].bindings[0].keystrokes, "/");
}

/// A file with a comment anywhere outside its strings loses it when the keymap is written
/// again, so an agent's change asks first; one whose `//` is only inside a string, or that
/// has trailing commas alone, loses nothing of the user's.
#[test]
fn a_comment_is_found_outside_the_strings() {
    for commented in [
        "// Mine.\n[]",
        "[{\"bindings\": {\"cmd-q\": null}}] // the end",
        "[/* none yet */]",
        "[{\"bindings\": {/**/}}]",
    ] {
        assert!(has_comments(commented), "{commented}");
    }
    for plain in [
        "",
        "[]",
        r#"[{"context": "a // b", "bindings": {"/": "x::Y", "cmd-/": "x::Z"}}]"#,
        "[{\"bindings\": {\"cmd-q\": null,},},]",
        r#"[{"bindings": {"cmd-k": ["x::Y", {"text": "/* not one */"}]}}]"#,
    ] {
        assert!(!has_comments(plain), "{plain}");
    }
}

#[test]
fn empty_file_is_an_empty_keymap() {
    assert_eq!(UserKeymap::parse("").unwrap(), UserKeymap::default());
    assert_eq!(
        UserKeymap::parse("// nothing\n").unwrap(),
        UserKeymap::default()
    );
}

#[test]
fn errors_name_the_binding() {
    let error = UserKeymap::parse(r#"[{"bindings": {"cmd-x": 3}}]"#).unwrap_err();
    assert!(error.contains("\"cmd-x\""), "{error}");
    assert!(UserKeymap::parse("{}").is_err());
}

#[test]
fn round_trips() {
    let mut keymap = UserKeymap::default();
    keymap.add(
        Some("Workspace"),
        "cmd-k e",
        Entry::unbind("workspace::CloseTabsLeft"),
    );
    keymap.add(
        Some("Workspace"),
        "cmd-e",
        Entry::action("workspace::CloseTabsLeft", None),
    );
    keymap.add(None, "cmd-q", Entry::Disabled);
    keymap.add(
        Some("Board"),
        "ctrl-1",
        Entry::action("board::Command", Some(json!({"id": "tool.move"}))),
    );
    let text = keymap.to_json();
    assert_eq!(UserKeymap::parse(&text).unwrap(), keymap);
}

/// A second binding of the same keys in the same context goes into a section of its own
/// after the first, which stays: a JSON object holds each key once.
#[test]
fn add_keeps_the_other_binding_of_the_same_keys() {
    let mut keymap = UserKeymap::default();
    keymap.add(Some("Workspace"), "cmd-e", Entry::action("a::B", None));
    keymap.add(
        Some(" Workspace "),
        "shift-cmd-e",
        Entry::action("a::B", None),
    );
    keymap.add(Some("Workspace"), "cmd-e", Entry::unbind("c::D"));
    keymap.add(Some(""), "cmd-e", Entry::action("e::F", None));
    let found: Vec<_> = keymap
        .bindings()
        .map(|(_, context, b)| (context, b.keystrokes.as_str(), b.entry.clone()))
        .collect();
    assert_eq!(
        found,
        [
            (Some("Workspace"), "cmd-e", Entry::action("a::B", None)),
            (
                Some("Workspace"),
                "shift-cmd-e",
                Entry::action("a::B", None)
            ),
            (Some("Workspace"), "cmd-e", Entry::unbind("c::D")),
            (None, "cmd-e", Entry::action("e::F", None)),
        ]
    );
    assert_eq!(keymap.sections.len(), 3);
    // The file holds every one of them.
    assert_eq!(UserKeymap::parse(&keymap.to_json()).unwrap(), keymap);

    // The keys in another order are the same keys; the same binding again changes nothing.
    let mut keymap = UserKeymap::default();
    keymap.add(Some("Editor"), "shift-cmd-k", Entry::action("a::B", None));
    keymap.add(Some("Editor"), "cmd-shift-k", Entry::action("x::Y", None));
    assert_eq!(keymap.sections.len(), 2);
    let before = keymap.clone();
    keymap.add(Some("Editor"), "cmd-shift-k", Entry::action("x::Y", None));
    assert_eq!(keymap, before);
}

/// A changed binding stays in its place while its keys are its own in its context, and moves
/// after the bindings it would share them with otherwise, which stay.
#[test]
fn replace_changes_only_its_binding() {
    let mut keymap = UserKeymap::default();
    keymap.add(Some("Workspace"), "cmd-a", Entry::action("a::A", None));
    keymap.add(Some("Workspace"), "cmd-b", Entry::action("b::B", None));
    let first = Location {
        section: 0,
        binding: 0,
    };
    keymap.replace(
        first,
        Some("Workspace"),
        "cmd-c",
        Entry::action("a::A", None),
    );
    assert_eq!(keymap.sections.len(), 1);
    assert_eq!(keymap.sections[0].bindings[0].keystrokes, "cmd-c");
    keymap.replace(
        first,
        Some("Workspace"),
        "cmd-b",
        Entry::action("a::A", None),
    );
    let found: Vec<_> = keymap
        .bindings()
        .map(|(_, _, b)| (b.keystrokes.as_str(), b.entry.clone()))
        .collect();
    assert_eq!(
        found,
        [
            ("cmd-b", Entry::action("b::B", None)),
            ("cmd-b", Entry::action("a::A", None)),
        ]
    );
    // Into another context, it moves there.
    keymap.replace(
        Location {
            section: 1,
            binding: 0,
        },
        Some("Editor"),
        "cmd-b",
        Entry::action("a::A", None),
    );
    assert_eq!(keymap.sections.len(), 2);
    assert_eq!(keymap.sections[1].context.as_deref(), Some("Editor"));
}

#[test]
fn remove_drops_an_empty_section() {
    let mut keymap = UserKeymap::default();
    keymap.add(Some("A"), "cmd-e", Entry::action("a::B", None));
    keymap.add(Some("C"), "cmd-e", Entry::action("a::B", None));
    let removed = keymap.remove(Location {
        section: 0,
        binding: 0,
    });
    assert!(removed.is_some());
    assert_eq!(keymap.sections.len(), 1);
    assert_eq!(keymap.sections[0].context.as_deref(), Some("C"));
}

#[test]
fn finds_a_binding_by_its_place() {
    let mut keymap = UserKeymap::default();
    keymap.add(
        Some("Workspace"),
        "cmd-w",
        Entry::unbind("workspace::CloseActiveTab"),
    );
    let at = Location {
        section: 0,
        binding: 0,
    };
    let binding = keymap.binding(at).unwrap();
    assert_eq!(binding.entry.unbinds(), Some("workspace::CloseActiveTab"));
    assert_eq!(keymap.binding(Location { binding: 1, ..at }), None);
    assert_eq!(keymap.binding(Location { section: 1, ..at }), None);
}
