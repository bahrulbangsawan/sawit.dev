//! Read-only viewers for non-text files: images, PDFs and videos.

mod image;
mod pdf;
mod video;

pub use image::ImageView;
pub use pdf::PdfView;
pub use video::VideoView;

use gpui_kit::component::*;
use gpui_kit::*;

/// Human file size ("245 KB").
pub fn file_size_label(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{} KB", b / (1 << 10)),
        b => format!("{b} B"),
    }
}

/// The thin bar above a viewer: file name on the left, facts and controls
/// on the right (same height and colours as the editor's toolbar).
pub fn toolbar(name: String, cx: &App) -> Div {
    h_flex()
        .h(px(28.))
        .px_3()
        .gap_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(cx.theme().foreground)
                .child(name),
        )
}

#[cfg(test)]
mod tests {
    use super::file_size_label;

    #[test]
    fn sizes_read_like_finder() {
        assert_eq!(file_size_label(512), "512 B");
        assert_eq!(file_size_label(250 * 1024), "250 KB");
        assert_eq!(file_size_label(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
        assert_eq!(file_size_label(2 << 30), "2.0 GB");
    }
}
