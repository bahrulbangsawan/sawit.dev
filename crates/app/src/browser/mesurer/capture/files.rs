//! mesurer's downloads (`file.save`), saved in the captures folder under the name their bytes
//! call for, and its copied screenshots (`clipboard.image`), to the pasteboard. The bytes are
//! decoded and written off the main thread.

use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::browser::base64_decode;
use sawit_core::browser::mesurer::files::{captures_dir, png_size, save_capture, shown_folder};
use serde_json::json;

use super::super::super::BrowserView;
use super::super::super::engine::WorldReply;
use super::records_copies;

/// The home folder: with no project, captures go to its `Downloads/sawit-captures`.
fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Saves a download mesurer made (a screenshot, a recording) and replies `{path, name}`; the
/// Annotate panel says where it went.
pub(super) fn save(
    view: &mut BrowserView,
    name: String,
    mime: String,
    base64: String,
    reply: WorldReply,
    cx: &mut Context<BrowserView>,
) {
    let project = view.project.clone();
    let home = home();
    let asked = name.clone();
    let saved = cx.background_executor().spawn(async move {
        let bytes = base64_decode(&base64).ok_or_else(|| format!("{name} came as bad base64."))?;
        let path = save_capture(project.as_deref(), &home, &name, &mime, &bytes)?;
        let folder = captures_dir(project.as_deref(), &home);
        Ok::<_, String>((path, shown_folder(project.as_deref(), &home, &folder)))
    });
    cx.spawn(async move |this, cx| {
        let saved = saved.await;
        let _ = this.update(cx, |this, cx| match saved {
            Ok((path, folder)) => {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let note = format!("Saved {file} to {folder}");
                this.annotate.update(cx, |a, cx| a.set_note(note, cx));
                this.mesurer.captures.push(path.clone());
                reply.ok_value(&json!({ "path": path, "name": file }));
            }
            Err(e) => {
                let note = format!("Couldn't save {asked}: {e}");
                this.annotate.update(cx, |a, cx| a.set_note(note, cx));
                reply.err(e);
            }
        });
    })
    .detach();
}

/// Puts a PNG mesurer copied (a region screenshot) on the pasteboard, or, under
/// `SAWIT_E2E_RUN`, keeps its size in `last_clipboard` instead; replies `{width, height}`.
pub(super) fn copy_image(png_base64: String, reply: WorldReply, cx: &mut Context<BrowserView>) {
    let decoded = cx.background_executor().spawn(async move {
        let png =
            base64_decode(&png_base64).ok_or("The image mesurer copied came as bad base64.")?;
        let size = png_size(&png).ok_or("The image mesurer copied isn't a PNG.")?;
        Ok::<_, &str>((png, size))
    });
    cx.spawn(async move |this, cx| {
        let decoded = decoded.await;
        let _ = this.update(cx, |this, cx| match decoded {
            Ok((png, (width, height))) => {
                this.mesurer.capture.last_image = Some((width, height));
                if records_copies(this) {
                    this.mesurer.last_clipboard = Some(format!("image/png {width}x{height}"));
                } else {
                    let image = Image::from_bytes(ImageFormat::Png, png);
                    cx.write_to_clipboard(ClipboardItem::new_image(&image));
                }
                reply.ok_value(&json!({ "width": width, "height": height }));
            }
            Err(e) => reply.err(e),
        });
    })
    .detach();
}
