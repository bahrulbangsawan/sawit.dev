//! Lane 1 (mesurer-capture): tests of captures, downloads, the pasteboard's images and colour
//! sampling, as mesurer's world asks for them: with no web view, as GPUI's tests have none.

use std::path::Path;

use gpui_kit::{AppContext as _, Entity, VisualTestContext};
use sawit_core::browser::{NOT_OPEN, base64_encode};
use serde_json::{Value, json};

use super::{engine_event, test_browser};
use crate::browser::BrowserView;
use crate::browser::engine::{EngineEvent, WorldReply};

/// A browser tab of `project`, never drawn, in a test window.
fn browser_of<'a>(
    project: &Path,
    cx: &'a mut gpui_kit::TestAppContext,
) -> (Entity<BrowserView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::browser::init(cx);
    });
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        let project = Some(project.to_path_buf());
        cx.new(|cx| BrowserView::new(project, Some("about:blank".into()), window, cx))
    });
    (view, cx)
}

/// Posts `{kind, payload}` from mesurer's world; what the app answered, once it has run.
fn post(
    view: &Entity<BrowserView>,
    kind: &str,
    payload: Value,
    cx: &mut VisualTestContext,
) -> Result<Value, String> {
    let body = json!({"kind": kind, "payload": payload, "url": "about:blank"}).to_string();
    let (reply, mut answer) = WorldReply::channel();
    engine_event(view, EngineEvent::World { body, reply }, cx);
    let answer = answer.try_recv().ok().flatten();
    answer
        .unwrap_or_else(|| Err("no answer".into()))
        .map(|json| serde_json::from_str(&json).unwrap())
}

/// The smallest PNG of `width` × `height`.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::new(width, height)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// A download lands in `<project>/.nyawit/browser/captures/` (made, with `.nyawit/`'s ignore
/// file), under a free name and the extension its bytes really have: the MP4 WebKit recorded
/// for a "WebM" is saved as `.mp4`. Each path is recorded for the `state` op.
#[gpui_kit::test]
fn a_download_is_saved_in_the_projects_captures(cx: &mut gpui_kit::TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let (view, cx) = browser_of(project.path(), cx);
    let shot = png(4, 2);
    let saved = post(
        &view,
        "file.save",
        json!({"name": "mesurer-2026-11-03-101112.png", "mime": "image/png", "base64": base64_encode(&shot)}),
        cx,
    )
    .unwrap();
    let captures = project.path().join(".nyawit/browser/captures");
    let first = captures.join("mesurer-2026-11-03-101112.png");
    assert_eq!(saved["path"], json!(first));
    assert_eq!(saved["name"], "mesurer-2026-11-03-101112.png");
    assert_eq!(std::fs::read(&first).unwrap(), shot);
    assert!(project.path().join(".nyawit/.gitignore").exists());

    // The same name again: the next free one, nothing overwritten.
    let again = post(
        &view,
        "file.save",
        json!({"name": "mesurer-2026-11-03-101112.png", "mime": "image/png", "base64": base64_encode(&png(2, 2))}),
        cx,
    )
    .unwrap();
    assert_eq!(
        again["path"],
        json!(captures.join("mesurer-2026-11-03-101112-2.png"))
    );
    assert_eq!(std::fs::read(&first).unwrap(), shot);

    // A recording named WebM whose bytes are MP4.
    let mp4 = [&[0, 0, 0, 0x18][..], b"ftypmp42", &[0; 12]].concat();
    let video = post(
        &view,
        "file.save",
        json!({"name": "mesurer-recording-2026-11-03T10-11-12-000Z.webm", "mime": "video/mp4", "base64": base64_encode(&mp4)}),
        cx,
    )
    .unwrap();
    assert_eq!(
        video["name"],
        "mesurer-recording-2026-11-03T10-11-12-000Z.mp4"
    );
    let recorded = cx.update(|_, cx| view.read(cx).mesurer.captures.clone());
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0], first);
    assert!(recorded[2].ends_with("mesurer-recording-2026-11-03T10-11-12-000Z.mp4"));

    // Bytes that aren't base64 are refused, and nothing is written.
    let bad = post(
        &view,
        "file.save",
        json!({"name": "x.png", "mime": "image/png", "base64": "a$b"}),
        cx,
    );
    assert!(bad.unwrap_err().contains("bad base64"));
    assert_eq!(std::fs::read_dir(&captures).unwrap().count(), 3);
}

/// A captures folder that is a link is never written through.
#[gpui_kit::test]
fn a_linked_captures_folder_is_refused(cx: &mut gpui_kit::TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".nyawit/browser")).unwrap();
    std::os::unix::fs::symlink(
        elsewhere.path(),
        project.path().join(".nyawit/browser/captures"),
    )
    .unwrap();
    let (view, cx) = browser_of(project.path(), cx);
    let saved = post(
        &view,
        "file.save",
        json!({"name": "a.png", "mime": "image/png", "base64": base64_encode(&png(1, 1))}),
        cx,
    );
    assert!(saved.unwrap_err().contains("link"));
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    assert!(cx.update(|_, cx| view.read(cx).mesurer.captures.is_empty()));
}

/// Under `SAWIT_E2E_RUN` a copied screenshot is recorded by its size and the pasteboard is left
/// alone; otherwise the PNG goes to the pasteboard. Bytes that aren't a PNG are refused.
#[gpui_kit::test]
fn a_copied_screenshot_goes_to_the_pasteboard_or_is_recorded(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    cx.update(|_, cx| view.update(cx, |b, _| b.mesurer.capture.record_copies = Some(true)));
    let copied = post(
        &view,
        "clipboard.image",
        json!({"png_base64": base64_encode(&png(240, 160))}),
        cx,
    );
    assert_eq!(copied, Ok(json!({"width": 240, "height": 160})));
    let (recorded, size) = cx.update(|_, cx| {
        let m = &view.read(cx).mesurer;
        (m.last_clipboard.clone(), m.capture.last_image)
    });
    assert_eq!(recorded.as_deref(), Some("image/png 240x160"));
    assert_eq!(size, Some((240, 160)));
    assert!(cx.update(|_, cx| cx.read_from_clipboard()).is_none());

    cx.update(|_, cx| view.update(cx, |b, _| b.mesurer.capture.record_copies = Some(false)));
    let shot = png(3, 2);
    let copied = post(
        &view,
        "clipboard.image",
        json!({"png_base64": base64_encode(&shot)}),
        cx,
    );
    assert_eq!(copied, Ok(json!({"width": 3, "height": 2})));
    let image = cx.update(|_, cx| {
        cx.read_from_clipboard()
            .and_then(|c| c.entries().first().cloned())
    });
    match image {
        Some(gpui_kit::ClipboardEntry::Image(image)) => assert_eq!(image.bytes, shot),
        other => panic!("no image on the pasteboard: {other:?}"),
    }

    let not_png = post(
        &view,
        "clipboard.image",
        json!({"png_base64": base64_encode(b"GIF89a....")}),
        cx,
    );
    assert!(not_png.unwrap_err().contains("isn't a PNG"));
}

/// With no web view, the page can't be captured nor a colour sampled: each says the browser
/// isn't open, and no turn among screenshots is left taken. Cancelling a sample nothing waits on
/// answers.
#[gpui_kit::test]
fn captures_and_colours_need_the_page(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    for (kind, payload) in [
        ("capture.visible", json!({})),
        ("capture.frame", json!({"max_width": 1280, "quality": 0.8})),
        ("color.sample", json!({})),
    ] {
        assert_eq!(
            post(&view, kind, payload, cx),
            Err(NOT_OPEN.into()),
            "{kind}"
        );
    }
    let free = cx.update(|_, cx| view.update(cx, |b, _| b.shots.enter().is_none()));
    assert!(free, "no shot holds the turn");
    assert_eq!(
        post(&view, "color.cancel", json!({}), cx),
        Ok(json!({"open": false}))
    );
    let sampling = cx.update(|_, cx| view.read(cx).mesurer.capture.sampling.is_some());
    assert!(!sampling);
}
