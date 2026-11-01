//! Copy, cut and paste on the canvas, with the system clipboard and the other canvas tabs: of its
//! layers, or of the characters of the text being edited.

use gpui_kit::*;
use sawit_board::BoardError;
use sawit_board::clipboard::{self, PasteSource};
use serde_json::json;

use super::BoardView;

impl BoardView {
    /// Copies, cuts or pastes (`id` is one of `sawit_board::policy::CLIPBOARD`), with the system
    /// clipboard and the other canvas tabs (`sawit_board::clipboard`). The keyboard's handler for
    /// these ids, which the menus share with Plan 6:
    ///
    /// - Copy and cut run in the engine, then the copy becomes the app's, which any canvas tab
    ///   pastes, and the system clipboard gets its marker.
    /// - Paste pastes what was copied last: an image, SVG or text another app copied since the
    ///   canvas's copy, else the copied layers. `at` is the page point to paste at (Paste here);
    ///   `None` pastes where FigCraft does. Paste over the selection and paste to replace it take
    ///   the copied layers only, as in FigCraft.
    /// - While text is edited, they act on its characters, as in any text field: the system
    ///   clipboard gets the selected text, and a paste types its text.
    ///
    /// A paste with nothing to paste does nothing and says nothing. A command the engine refuses
    /// says why, in a toast, in plain words; from a key (`key`), it says nothing when it had
    /// nothing to act on, as [`Self::finish_key`] has it.
    pub(super) fn run_clipboard(
        &mut self,
        id: &str,
        at: Option<(f64, f64)>,
        key: bool,
        cx: &mut Context<Self>,
    ) {
        let done = if self.editing_text() {
            self.clipboard_text(id, cx)
        } else {
            self.clipboard_layers(id, at, cx)
        };
        self.handle_ui_requests(Vec::new(), cx);
        if let Err(error) = done {
            let said = if key {
                error.for_key()
            } else {
                Some(error.for_user())
            };
            if let Some(said) = said {
                self.show_toast(said, cx);
            }
        }
        cx.notify();
    }

    /// Copy and paste of the canvas's layers ([`Self::run_clipboard`]).
    fn clipboard_layers(
        &mut self,
        id: &str,
        at: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) -> Result<(), BoardError> {
        let pasted = (id == "edit.paste").then(|| system_clipboard(cx));
        let Some(board) = self.board_mut() else {
            return Ok(());
        };
        match id {
            "edit.copy" | "edit.cut" => {
                board.execute(id, &json!({}))?;
                let marker = clipboard::after_copy(board);
                cx.write_to_clipboard(ClipboardItem::new_string(marker));
            }
            "edit.paste" => {
                let (text, image) = pasted.unwrap_or_default();
                let marker = clipboard::marker();
                clipboard::before_paste(board);
                let source =
                    clipboard::paste_source(text.as_deref(), image.as_deref(), marker.as_deref());
                // Nothing newer from another app, and no layers copied: nothing to paste.
                if source == PasteSource::Canvas && board.session.clipboard.nodes.is_empty() {
                    return Ok(());
                }
                clipboard::paste(board, source, at)?;
            }
            // Paste over the selection and paste to replace it.
            _ => {
                clipboard::before_paste(board);
                if board.session.clipboard.nodes.is_empty() {
                    return Ok(());
                }
                board.execute(id, &json!({}))?;
            }
        }
        Ok(())
    }

    /// Copy and paste of the characters of the text being edited ([`Self::run_clipboard`]).
    fn clipboard_text(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), BoardError> {
        let pasted = matches!(id, "edit.paste" | "edit.pasteOver")
            .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
            .flatten();
        let Some(board) = self.board_mut() else {
            return Ok(());
        };
        match id {
            "edit.copy" | "edit.cut" => {
                let selected = if id == "edit.cut" {
                    clipboard::cut_text(board)?
                } else {
                    clipboard::copy_text(board)?
                };
                if let Some(text) = selected {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "edit.paste" | "edit.pasteOver" => {
                // Text, SVG among it; the canvas's marker stands for layers, which no text takes.
                let marker = clipboard::marker();
                match clipboard::paste_source(pasted.as_deref(), None, marker.as_deref()) {
                    PasteSource::Text(text) | PasteSource::Svg(text) => {
                        clipboard::paste_text(board, &text)?;
                    }
                    PasteSource::Canvas | PasteSource::Image(_) => {}
                }
            }
            // Paste to replace replaces layers: nothing while text is edited.
            _ => {}
        }
        Ok(())
    }
}

/// What the system clipboard holds for a paste: its text, and its image when FigCraft reads the
/// image's format (PNG, JPEG, WebP, GIF, BMP or TIFF). SVG copied as an image comes back as its
/// text, as which a paste takes SVG in.
fn system_clipboard(cx: &App) -> (Option<String>, Option<Vec<u8>>) {
    let Some(item) = cx.read_from_clipboard() else {
        return (None, None);
    };
    let mut text = item.text();
    let mut image = None;
    for entry in item.entries() {
        let ClipboardEntry::Image(picture) = entry else {
            continue;
        };
        match picture.format {
            ImageFormat::Svg if text.is_none() => {
                text = String::from_utf8(picture.bytes.clone()).ok();
            }
            ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Webp
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Tiff
                if image.is_none() =>
            {
                image = Some(picture.bytes.clone());
            }
            _ => {}
        }
    }
    (text, image)
}
