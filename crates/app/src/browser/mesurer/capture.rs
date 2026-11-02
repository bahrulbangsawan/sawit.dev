//! Lane 1 (mesurer-capture): what mesurer's world asks of the Mac. `clipboard.image` (a PNG to
//! the pasteboard) and `file.save` (a download, saved under `.nyawit/browser/captures/`) in
//! `capture/files.rs`; `capture.visible` and `capture.frame` (the page as WebKit draws it,
//! `engine/capture.rs`) in `capture/shots.rs`; `color.sample` and `color.cancel` (`NSColorSampler`,
//! `engine/color.rs`) in `capture/color.rs`.

mod color;
mod files;
mod shots;

use gpui_kit::*;
use sawit_core::browser::mesurer::FromMesurer;

use super::super::BrowserView;
use super::super::engine::WorldReply;

/// Lane 1's state: the colour sampler, the last image copied.
#[derive(Default)]
pub(in crate::browser) struct CaptureState {
    /// The `color.sample` waiting on macOS's sampler: one sampler at a time.
    pub sampling: Option<WorldReply>,
    /// macOS's sampler is open: its session hasn't ended.
    pub sampler_open: bool,
    /// The size of the PNG mesurer copied last.
    pub last_image: Option<(u32, u32)>,
    /// Tests: copies kept from the pasteboard as under `SAWIT_E2E_RUN`, or not.
    #[cfg(test)]
    pub record_copies: Option<bool>,
}

/// Answers a capture kind ([`sawit_core::browser::mesurer::Lane::Capture`]).
pub(super) fn handle(
    view: &mut BrowserView,
    message: FromMesurer,
    reply: WorldReply,
    cx: &mut Context<BrowserView>,
) {
    match message {
        FromMesurer::ClipboardImage { png_base64 } => files::copy_image(png_base64, reply, cx),
        FromMesurer::FileSave { name, mime, base64 } => {
            files::save(view, name, mime, base64, reply, cx)
        }
        FromMesurer::CaptureVisible {} => shots::visible(view, reply, cx),
        FromMesurer::CaptureFrame { max_width, quality } => {
            shots::frame(view, max_width, quality, reply, cx)
        }
        FromMesurer::ColorSample {} => color::sample(view, reply, cx),
        FromMesurer::ColorCancel {} => color::cancel(view, reply),
        other => reply.err(format!("“{}” isn't a capture.", other.kind())),
    }
}

/// Whether mesurer's copies are kept from the user's pasteboard ([`super::e2e_run`]).
fn records_copies(view: &BrowserView) -> bool {
    #[cfg(test)]
    if let Some(record) = view.mesurer.capture.record_copies {
        return record;
    }
    let _ = view;
    super::e2e_run()
}
