//! Tests of the file editor: the breadcrumb, preview images, and a file that comes back.

// Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
use super::render::{breadcrumb, resolve_image};
use gpui_kit::{ImageSource, Resource, SharedUri};
use std::path::Path;

/// A file moved away and put back where it was, as the Files tab's ⌫ and ⌘Z do, is no
/// longer said to be deleted. Its tab said "This file was deleted from disk" until the next
/// save: the file came back with the time it was loaded with, so nothing looked at it again,
/// and the notice asked for a save that wasn't needed.
#[gpui_kit::test]
fn a_file_put_back_where_it_was_is_no_longer_said_to_be_deleted(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::AppContext as _;

    // The file is read and looked at on the runtime's real threads.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
    });
    let dir = crate::files::Scratch::new("editor-put-back");
    let (file, away) = (dir.0.join("notes.md"), dir.0.join("away.md"));
    std::fs::write(&file, "notes\n").unwrap();
    let path = file.clone();
    let (_, editor) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| super::FileEditor::open(path, None, window, cx))
        })
        .unwrap()
    });
    let notice = |cx: &mut gpui_kit::TestAppContext| {
        cx.update(|cx| editor.read(cx).notice.as_ref().map(|n| n.text.clone()))
    };
    let wait =
        |cx: &mut gpui_kit::TestAppContext, what: &str, ok: &dyn Fn(Option<String>) -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                cx.run_until_parked();
                if ok(notice(cx)) {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting until {what}"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
                cx.executor().advance_clock(super::DISK_POLL);
            }
        };
    let loaded =
        |cx: &mut gpui_kit::TestAppContext| cx.update(|cx| editor.read(cx).state.is_some());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !loaded(cx) {
        assert!(
            std::time::Instant::now() < deadline,
            "the file never loaded"
        );
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    std::fs::rename(&file, &away).unwrap();
    wait(cx, "the tab says the file was deleted", &|n| {
        n.is_some_and(|n| n.contains("deleted"))
    });
    std::fs::rename(&away, &file).unwrap();
    wait(cx, "the notice is gone", &|n| n.is_none());
}

#[test]
fn breadcrumb_is_relative_to_the_project() {
    let root = Path::new("/w/app");
    assert_eq!(
        breadcrumb(Path::new("/w/app/src/a.ts"), Some(root)),
        vec!["src", "a.ts"]
    );
    // Outside the project: the full path, without the leading "/".
    assert_eq!(
        breadcrumb(Path::new("/etc/hosts"), Some(root)),
        vec!["etc", "hosts"]
    );
}

fn path_of(source: ImageSource) -> Option<String> {
    match source {
        ImageSource::Resource(Resource::Path(p)) => Some(p.display().to_string()),
        _ => None,
    }
}

#[test]
fn preview_images_resolve_relative_to_the_file() {
    let dir = Path::new("/w/app/docs");
    assert_eq!(
        path_of(resolve_image(dir, &SharedUri::from("img/a.png"))).as_deref(),
        Some("/w/app/docs/img/a.png")
    );
    assert_eq!(
        path_of(resolve_image(dir, &SharedUri::from("/abs/b.png"))).as_deref(),
        Some("/abs/b.png")
    );
    assert_eq!(
        path_of(resolve_image(dir, &SharedUri::from("https://x.dev/c.png"))),
        None
    );
}
