//! The tests of the canvas document: making, loading and saving it, and its errors' words.

use super::*;

#[test]
fn new_board_is_a_board_with_an_empty_page() {
    let b = BoardDoc::new_board("Canvas").unwrap();
    assert_eq!(b.kind(), DocKind::Board);
    let doc = b.doc().unwrap();
    let page = doc.first_page().unwrap();
    assert!(doc.node(page).unwrap().children.is_empty());
}

#[test]
fn bytes_round_trip_keeps_a_sticky() {
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    b.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "hi"}))
        .unwrap();
    let before = b.doc().unwrap().nodes().count();
    let back = BoardDoc::from_bytes(&b.to_bytes().unwrap()).unwrap();
    let doc = back.doc().unwrap();
    assert_eq!(doc.nodes().count(), before);
    assert!(
        doc.nodes()
            .any(|n| matches!(n.board, Some(figcraft_doc::BoardItem::Sticky { .. })))
    );
}

#[test]
fn unknown_command_is_an_engine_error() {
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    assert!(matches!(
        b.execute("no.such", &json!({})),
        Err(BoardError::Engine(_))
    ));
}

/// The toast in the user's screenshot: ⌥⌘G with nothing selected. An agent still reads the
/// engine's words, with what to pass; the user reads plain words, and a key reads nothing.
#[test]
fn a_command_that_does_not_apply_speaks_plainly_to_the_user() {
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    let refused = b.execute("object.frameSelection", &json!({})).unwrap_err();
    assert!(refused.is_unavailable(), "{refused:?}");
    assert_eq!(
        refused.to_string(),
        "command `object.frameSelection` is not available right now: nothing is selected \
             (pass `ids`)"
    );
    assert_eq!(refused.for_user(), "Nothing is selected");
    assert_eq!(refused.for_key(), None);
    // No properties copied to paste: the same.
    let paste = b.execute("edit.pasteProperties", &json!({})).unwrap_err();
    assert!(paste.is_unavailable(), "{paste:?}");
    assert_eq!(paste.for_user(), "No copied properties");
    assert_eq!(paste.for_key(), None);
}

#[test]
fn a_boards_tool_on_a_design_file_says_why_even_from_a_key() {
    let mut design = BoardDoc {
        session: Session::with_document(Document::new("Design")),
    };
    let refused = design.execute("tool.sticky", &json!({})).unwrap_err();
    assert!(refused.is_unavailable(), "{refused:?}");
    let said = "This is for boards, and this canvas is a design file";
    assert_eq!(refused.for_user(), said);
    assert_eq!(refused.for_key().as_deref(), Some(said));
}

#[test]
fn plain_words_drop_the_advice_to_agents() {
    for (engine, user) in [
        ("nothing is selected (pass `ids`)", "Nothing is selected"),
        (
            "the main component was deleted (use component.restoreMain)",
            "The main component was deleted",
        ),
        (
            "no export settings (add one with export.add)",
            "No export settings",
        ),
        (
            "the document has unsaved changes (pass force: true to discard)",
            "The document has unsaved changes",
        ),
        (
            "the file has no path yet: pass `path`",
            "The file has no path yet",
        ),
        (
            "the saved path ends in .fig: choose Save as or pass `path`",
            "The saved path ends in .fig",
        ),
        (
            "pages can't be copied; use page.duplicate",
            "Pages can't be copied",
        ),
        // What is no advice stays.
        (
            "Frame 1 isn't on a page (it is a library component)",
            "Frame 1 isn't on a page (it is a library component)",
        ),
        ("no copied properties", "No copied properties"),
    ] {
        let error = BoardError::Unavailable {
            command: "some.command".into(),
            reason: engine.into(),
        };
        assert_eq!(error.for_user(), user, "{engine}");
        assert!(!error.for_user().contains('`'), "{engine}");
    }
    // A command given what it can't use says what it lacks, and nothing from a key.
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    let refused = b.execute("component.goToMain", &json!({})).unwrap_err();
    assert_eq!(refused.for_user(), "Select an instance");
    assert_eq!(refused.for_key(), None);
    let refused = b.execute("text.link", &json!({})).unwrap_err();
    assert!(!refused.for_user().contains('`'), "{refused:?}");
    assert_eq!(refused.for_key(), None);
    // Other errors lose their advice the same way, and say why from a key too.
    // A message of its own keeps its first letter: it may start with a file's name.
    let other = BoardError::Engine("notes.txt: no image in it (pass `data`)".into());
    assert_eq!(other.for_user(), "notes.txt: no image in it");
    assert_eq!(
        other.for_key().as_deref(),
        Some("notes.txt: no image in it")
    );
}

#[test]
fn revision_moves_on_edit() {
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    let r0 = b.revision();
    b.execute("board.sticky", &json!({"x": 0, "y": 0})).unwrap();
    assert!(b.revision() > r0);
}

#[test]
fn corrupt_or_truncated_bytes_are_a_format_error() {
    let is_format_error =
        |bytes: &[u8]| matches!(BoardDoc::from_bytes(bytes), Err(BoardError::Format(_)));
    assert!(is_format_error(b"not a canvas file"));
    assert!(is_format_error(&[]));
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    b.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "hi"}))
        .unwrap();
    let bytes = b.to_bytes().unwrap();
    assert!(is_format_error(&bytes[..bytes.len() / 2]));
}

/// A file the canvas can't read is said to be no canvas file, or one from a newer Sawit: the
/// user and the agent read Sawit's name, not the engine's.
#[test]
fn a_file_that_cant_be_read_is_told_in_sawits_words() {
    let error = BoardDoc::from_bytes(b"{").err().unwrap().to_string();
    assert!(error.starts_with("not a valid canvas file: "), "{error}");
    let newer = br#"{"version":99,"pages":[]}"#;
    let error = BoardDoc::from_bytes(newer).err().unwrap().to_string();
    assert_eq!(
        error,
        "this file was saved by a newer version of Sawit (format version 99)"
    );
    assert!(!error.contains("FigCraft"));
}

#[test]
fn a_damaged_image_loads_with_issues_that_name_what_was_lost() {
    // A 1x1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    let placed = b
        .execute("image.place", &json!({"data": PNG, "x": 0, "y": 0}))
        .unwrap();
    let hash = placed["image"].as_str().unwrap().to_string();
    let png = b.doc().unwrap().images[&hash].bytes.clone();
    let bytes = b.to_bytes().unwrap();

    // The undamaged file loads without a word.
    let (back, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert!(back.doc().unwrap().images.contains_key(&hash));

    // Flip a byte of the stored image: the file still opens, without the image, and says so.
    let mut damaged = bytes.clone();
    let at = damaged
        .windows(png.len())
        .position(|w| w == png.as_slice())
        .unwrap();
    damaged[at + png.len() / 2] ^= 0xFF;
    let (back, issues) = BoardDoc::from_bytes_with_issues(&damaged).unwrap();
    assert!(issues.iter().any(|i| i.contains(&hash)), "{issues:?}");
    assert!(!back.doc().unwrap().images.contains_key(&hash));
    // `from_bytes` opens it too, and only logs the issues.
    assert!(BoardDoc::from_bytes(&damaged).is_ok());
}

/// Sticky notes on the document, whatever it shows.
fn stickies(doc: &Document) -> usize {
    doc.nodes()
        .filter(|n| matches!(n.board, Some(figcraft_doc::BoardItem::Sticky { .. })))
        .count()
}

#[test]
fn a_version_history_is_kept_by_a_save() {
    let board = BoardDoc::new_board("Canvas").unwrap();
    let doc = board.doc().unwrap();
    let history = [figcraft_format::Version::new(
        "v1",
        "First",
        "",
        1,
        doc.clone(),
    )];
    let bytes = figcraft_format::save_bytes_with_versions(&doc, None, &history).unwrap();

    // It is read with the file, so there is nothing to say about it.
    let (mut board, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    // A save writes it back, after an edit too.
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "after"}))
        .unwrap();
    let saved = board.to_bytes().unwrap();
    let loaded = figcraft_format::load_bytes_with_versions(&saved).unwrap();
    let names: Vec<&str> = loaded.versions.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["First"]);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(stickies(&loaded.doc), 1);
}

#[test]
fn branches_are_kept_by_a_save() {
    let board = BoardDoc::new_board("Canvas").unwrap();
    let doc = board.doc().unwrap();
    let branch = figcraft_format::Branch {
        id: "b1".into(),
        name: "Explore".into(),
        time: 1,
        client: 1,
        state: figcraft_format::BranchState::Active,
        base: doc.clone(),
        head: doc.clone(),
        versions: Vec::new(),
    };
    let bytes = figcraft_format::save_bytes_with_history(&doc, None, &[], &[branch]).unwrap();

    let (mut board, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "after"}))
        .unwrap();
    let loaded = figcraft_format::load_bytes_with_versions(&board.to_bytes().unwrap()).unwrap();
    let names: Vec<&str> = loaded.branches.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Explore"]);
    assert_eq!(stickies(&loaded.doc), 1);
}

#[test]
fn a_save_writes_the_document_not_a_version_shown_in_its_place() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let sticky = |board: &mut BoardDoc, text: &str| {
        let at = json!({"x": stickies(&board.doc().unwrap()) * 300, "y": 0, "text": text});
        board.execute("board.sticky", &at).unwrap();
    };
    sticky(&mut board, "then");
    board
        .execute("file.saveVersion", &json!({"name": "Then"}))
        .unwrap();
    sticky(&mut board, "now");
    let now = board.doc().unwrap();

    // The canvas shows the version in place of the document...
    board
        .execute("version.preview", &json!({"index": 0}))
        .unwrap();
    assert_eq!(stickies(&board.doc().unwrap()), 1);
    // ...and the document is still what is saved, with its history.
    assert!(Arc::ptr_eq(&board.document_itself().unwrap(), &now));
    let loaded = figcraft_format::load_bytes_with_versions(&board.to_bytes().unwrap()).unwrap();
    assert_eq!(stickies(&loaded.doc), 2);
    assert_eq!(loaded.versions.len(), 1);
}

#[test]
fn images_past_the_loaders_limit_make_saving_over_their_file_refused() {
    let bytes = file_with_an_image_past_the_limit();
    let path = Path::new("/work/design.sawit");
    let (board, issues) = BoardDoc::from_file_bytes(&bytes, path).unwrap();
    let note = |i: &&String| i.contains("past the limit");
    assert_eq!(issues.iter().filter(note).count(), 1, "{issues:?}");
    assert!(
        issues.iter().any(|i| i.contains("(1 of them)")),
        "{issues:?}"
    );
    // Saved over, the file would lose the image it holds unread. That is refused, in
    // FigCraft's own words, the canvas in its place; saved anywhere else, nothing is lost.
    let refused = board.refuse_dropping_images(path).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "design wasn't saved: 1 of its images weren't loaded (together they pass what \
             the canvas reads from one file), and saving over it would delete them. Save it under \
             another name; this file keeps them."
    );
    assert!(
        board
            .refuse_dropping_images(Path::new("/work/copy.sawit"))
            .is_ok()
    );
    // Read with no file, it has no file to save over.
    let (board, _) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
    assert!(board.refuse_dropping_images(path).is_ok());
}

/// CANVAS-CORE-12: a save records the page on show, as FigCraft's own does, so the file
/// opens on it again, here and in FigCraft. It opened on the page FigCraft last saved.
#[test]
fn a_save_keeps_the_page_on_show() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let first = board.session.ds().unwrap().page;
    board.execute("page.add", &json!({})).unwrap();
    let second = board.session.ds().unwrap().page;
    assert_ne!(first, second, "page.add shows the new page");
    let page_on_open = |board: &BoardDoc| {
        let back = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
        back.session.ds().unwrap().page
    };
    assert_eq!(page_on_open(&board), second);
    board.execute("page.select", &json!({"id": first})).unwrap();
    assert_eq!(page_on_open(&board), first);
}

/// CANVAS-CORE-9: a panic in what an op does inside its group (a bug in Sawit's code between
/// the engine's commands) is caught there: the group fails and rolls back, and the next edit
/// is the one undo step. Caught outside the group, the sticky made before the panic stayed, a
/// half-done op, and was an undo step too.
#[test]
fn a_panic_in_a_group_rolls_it_back_and_leaves_undo_whole() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let before = board.doc().unwrap();
    let result: Result<(), EngineError> = grouped(&mut board.session, "Add elements", |session| {
        session.execute("board.sticky", &json!({"x": 0, "y": 0}))?;
        panic!("a bug in the op");
    });
    let error = result.unwrap_err().to_string();
    assert!(error.contains("a bug in the op"), "{error}");
    assert!(
        *board.doc().unwrap() == *before,
        "the sticky made before stayed"
    );
    board
        .execute("board.sticky", &json!({"x": 300, "y": 0}))
        .unwrap();
    assert_eq!(board.session.undo_depth(), (1, 0));
}

#[test]
fn kind_survives_a_save_and_reload() {
    let board = BoardDoc::new_board("Canvas").unwrap();
    let board = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
    assert_eq!(board.kind(), DocKind::Board);
    let design = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
    let design = BoardDoc::from_bytes(&design).unwrap();
    assert_eq!(design.kind(), DocKind::Design);
}

#[test]
fn without_a_document_there_is_nothing_to_read() {
    let b = BoardDoc {
        session: Session::empty(),
    };
    assert_eq!(b.revision(), 0);
    assert_eq!(b.kind(), DocKind::Design);
    assert!(matches!(b.doc(), Err(BoardError::Engine(_))));
    assert!(matches!(b.to_bytes(), Err(BoardError::Engine(_))));
}
