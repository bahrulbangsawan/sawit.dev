//! Reading a scene from a file, and a file with nothing in it.

use super::*;

#[test]
fn read_file_reads_a_scene_or_says_why_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.excalidraw");

    let error = read_file(&path).unwrap_err();
    assert!(matches!(error, BoardError::Io(_)), "{error:?}");
    assert!(error.to_string().contains("board.excalidraw"), "{error}");

    std::fs::write(&path, "{ not json").unwrap();
    let error = read_file(&path).unwrap_err();
    assert!(matches!(error, BoardError::Format(_)), "{error:?}");
    let message = error.to_string();
    assert!(
        message.contains("board.excalidraw") && message.contains("isn't valid JSON"),
        "{message}"
    );

    // A byte order mark, which some editors write, is not part of the JSON.
    std::fs::write(&path, "\u{feff}{\"elements\": []}").unwrap();
    assert_eq!(read_file(&path).unwrap(), json!({"elements": []}));
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/excalidraw/basic.excalidraw"),
    )
    .unwrap();
    assert_eq!(read_file(&path).unwrap(), basic());
}

#[test]
fn a_file_with_nothing_in_it_is_a_blank_scene_only_when_asked_to_be() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("canvas.excalidraw");
    // Today's canvas opens a file like these blank: empty, white space, a byte order mark
    // (which some editors write), or any mix of them.
    for blank in [
        "",
        " ",
        " \n",
        "\t\r\n",
        "\u{feff}",
        "\u{feff}\n  ",
        " \u{feff} ",
        "\u{a0}",
    ] {
        std::fs::write(&path, blank).unwrap();
        // `read_file` is for a file somebody asked to load, and takes none of them.
        let error = read_file(&path).unwrap_err();
        assert!(
            matches!(error, BoardError::Format(_)),
            "{blank:?}: {error:?}"
        );
        assert!(
            error.to_string().contains("isn't valid JSON"),
            "{blank:?}: {error}"
        );

        let scene = read_file_or_blank(&path).unwrap();
        let imported = to_skeletons(&scene).unwrap();
        assert_eq!(imported, Imported::default(), "{blank:?}");
        // And it opens as the blank board it is, with nothing to say.
        let (mut board, notes) = to_board("Canvas", &scene).unwrap();
        assert_eq!(notes, Vec::<String>::new());
        assert!(elements(&mut board).is_empty());
    }

    // Anything with something in it is read as `read_file` reads it: what isn't there, what
    // isn't text, and what isn't JSON are errors that name the file. So is a scene with no
    // `elements`, once it is imported.
    let missing = dir.path().join("missing.excalidraw");
    assert_eq!(
        read_file_or_blank(&missing).unwrap_err(),
        read_file(&missing).unwrap_err()
    );
    for text in [&b"{ not json"[..], b"\0", b"[", &[0xFF, 0xFE, 0x00]] {
        std::fs::write(&path, text).unwrap();
        let error = read_file_or_blank(&path).unwrap_err();
        assert_eq!(error, read_file(&path).unwrap_err(), "{text:?}");
        assert!(error.to_string().contains("canvas.excalidraw"), "{error}");
    }
    for text in ["{}", "null", r#"{"type": "excalidraw"}"#] {
        std::fs::write(&path, text).unwrap();
        let scene = read_file_or_blank(&path).unwrap();
        assert_eq!(scene, read_file(&path).unwrap(), "{text}");
        assert!(to_skeletons(&scene).is_err(), "{text}");
    }
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/excalidraw/basic.excalidraw"),
    )
    .unwrap();
    assert_eq!(read_file_or_blank(&path).unwrap(), basic());
}
