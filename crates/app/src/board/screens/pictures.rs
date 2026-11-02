//! Screens as pictures: a texture's bytes as a PNG ([`png_of`]), encoded off the UI thread, for
//! `canvas_screen_get` and Send to Agent; and the canvas's `export_svg` and `export_png` with each
//! screen's latest picture painted in its frame, where the document alone has an empty frame.

use std::sync::Arc;

use figcraft_doc::NodeId;
use futures::channel::oneshot;
use gpui_kit::*;
use serde_json::Value;

use crate::agents::Look;
use crate::board::BoardView;
use crate::workspace::Answer;

/// The image `image` holds (BGRA, premultiplied, as GPUI's textures are) as a PNG.
pub(in crate::board) fn png_of(image: &RenderImage) -> Option<Vec<u8>> {
    let size = image.size(0);
    let (w, h) = (size.width.0 as u32, size.height.0 as u32);
    let bytes = image.as_bytes(0)?;
    if w == 0 || h == 0 || bytes.len() < (w * h * 4) as usize {
        return None;
    }
    let mut rgba = Vec::with_capacity(bytes.len());
    for &[b, g, r, a] in bytes.as_chunks::<4>().0 {
        let un = |c: u8| match a {
            0 => 0,
            255 => c,
            a => ((u16::from(c) * 255 + u16::from(a) / 2) / u16::from(a)).min(255) as u8,
        };
        rgba.extend_from_slice(&[un(r), un(g), un(b), a]);
    }
    let pixels = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut out = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

impl BoardView {
    /// The screens on the page on show that have a picture, with it.
    pub(in crate::board) fn screen_pictures(&self) -> Vec<(NodeId, Arc<RenderImage>)> {
        let Some(web) = self.web.as_ref() else {
            return Vec::new();
        };
        let mut pictures: Vec<(NodeId, Arc<RenderImage>)> = (web.frames.iter())
            .filter(|(_, frame)| matches!(frame.spec.source, crate::board::web::Source::Screen(_)))
            .filter_map(|(id, frame)| Some((*id, frame.texture.as_ref()?.image.clone())))
            .collect();
        pictures.sort_by_key(|(id, _)| *id);
        pictures
    }

    /// `look`'s agent's `export_svg` or `export_png` of the page on show, with each screen's
    /// picture in its frame: the pictures are encoded off the UI thread, then the page is
    /// exported from a copy that has them ([`sawit_board::ops::export_with_pictures`]).
    pub(super) fn export_with_screens(
        &mut self,
        op: Value,
        look: &Look,
        pictures: Vec<(NodeId, Arc<RenderImage>)>,
        cx: &mut Context<Self>,
    ) -> Answer {
        let encoded = cx.background_spawn(async move {
            (pictures.into_iter())
                .filter_map(|(id, image)| Some((id, png_of(&image)?)))
                .collect::<Vec<_>>()
        });
        let (reply, answer) = oneshot::channel();
        let look = look.clone();
        cx.spawn(async move |this, cx| {
            let pngs = encoded.await;
            let exported = this.update(cx, |this, cx| {
                let now = cx.background_executor().now();
                let action = sawit_core::agents::canvas_action(&op);
                this.agents.saw(&look, &action.doing, now);
                this.after_agents_changed(cx);
                match this.board() {
                    Some(board) => sawit_board::ops::export_with_pictures(board, &op, &pngs),
                    None => serde_json::json!({"ok": false, "error": "the canvas isn't open"}),
                }
            });
            let _ = reply.send(exported.map_err(|_| "The canvas closed.".to_string()));
        })
        .detach();
        Answer::Later(answer)
    }
}
