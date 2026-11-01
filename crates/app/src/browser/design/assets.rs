//! The Assets tab: images, SVG, icons, video and Lottie found on the page, each with a
//! preview, where it is used and a Save button, and Save all in the header.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::scan::{AssetKind, AssetRef, Scan};

use super::{DesignEvent, DesignPanel, Thumb};
use crate::browser::panel;

/// The tabs, in order.
pub(super) const KINDS: [AssetKind; 5] = [
    AssetKind::Image,
    AssetKind::Svg,
    AssetKind::Icon,
    AssetKind::Video,
    AssetKind::Lottie,
];

/// The most rows drawn for one tab.
const MAX_ROWS: usize = 200;
/// The most previews fetched for one tab.
pub(super) const MAX_THUMBS: usize = 60;
/// The preview's side.
const THUMB: f32 = 40.;

pub(super) fn kind_title(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "Images",
        AssetKind::Svg => "SVG",
        AssetKind::Icon => "Icons",
        AssetKind::Video => "Video",
        AssetKind::Lottie => "Lottie",
        AssetKind::Font => "Fonts",
    }
}

fn kind_word(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "image",
        AssetKind::Svg => "svg",
        AssetKind::Icon => "icon",
        AssetKind::Video => "video",
        AssetKind::Lottie => "lottie",
        AssetKind::Font => "font",
    }
}

/// What an empty tab says: none of that kind, and where they would come from.
fn empty_text(kind: AssetKind) -> (&'static str, &'static str) {
    match kind {
        AssetKind::Image => (
            "No images on this page",
            "Images in <img>, <picture> and CSS backgrounds show here.",
        ),
        AssetKind::Svg => (
            "No SVG on this page",
            "Inline <svg> and .svg files the page draws show here.",
        ),
        AssetKind::Icon => (
            "No icons on this page",
            "The favicon, touch icons, the manifest's icons and small inline SVGs show here.",
        ),
        AssetKind::Video => (
            "No video on this page",
            "<video> sources and their posters show here.",
        ),
        AssetKind::Lottie => (
            "No Lottie animations on this page",
            "Lottie players and their JSON files show here.",
        ),
        AssetKind::Font => ("No font files", "Web fonts show in the Fonts tab."),
    }
}

/// How many assets of each tab, zero counts kept.
pub(super) fn asset_counts(scan: &Scan) -> Vec<(AssetKind, usize)> {
    KINDS
        .into_iter()
        .map(|k| (k, scan.assets.iter().filter(|a| a.kind == k).count()))
        .collect()
}

/// The tab shown first: the first that has something.
pub(super) fn first_kind(scan: &Scan) -> AssetKind {
    asset_counts(scan)
        .into_iter()
        .find(|(_, n)| *n > 0)
        .map_or(AssetKind::Image, |(k, _)| k)
}

/// The assets of `kind`, as the tab lists them.
pub(super) fn shown(scan: &Scan, kind: AssetKind) -> Vec<&AssetRef> {
    scan.assets.iter().filter(|a| a.kind == kind).collect()
}

/// What a row calls the asset: a logo is "logo".
pub(super) fn row_label(a: &AssetRef) -> String {
    if a.logo {
        "logo".to_string()
    } else if a.label.trim().is_empty() {
        "(unnamed)".to_string()
    } else {
        a.label.clone()
    }
}

/// `W×H` when both are known.
pub(super) fn dimensions(a: &AssetRef) -> Option<String> {
    Some(format!("{}×{}", a.width?, a.height?))
}

/// What "Save all" downloads.
pub(super) fn save_all_set(scan: &Scan, include_videos: bool) -> Vec<AssetRef> {
    scan.assets
        .iter()
        .filter(|a| a.kind != AssetKind::Font && (include_videos || a.kind != AssetKind::Video))
        .cloned()
        .collect()
}

/// The key a preview is kept under: the asset's URL, else a hash of its inline SVG.
pub(super) fn thumb_key(a: &AssetRef) -> String {
    match (&a.url, &a.svg) {
        (Some(url), _) => url.clone(),
        (None, Some(svg)) => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            svg.hash(&mut h);
            format!("svg:{:x}", h.finish())
        }
        (None, None) => format!("none:{}", a.label),
    }
}

/// A preview made without a download: inline SVG markup, or a `data:` URL.
pub(super) fn local_thumb(a: &AssetRef) -> Option<Thumb> {
    if a.kind == AssetKind::Video || a.kind == AssetKind::Lottie {
        return None;
    }
    let bytes = match (&a.url, &a.svg) {
        (Some(url), _) if url.starts_with("data:") => {
            sawit_core::site_design::assets::decode_data_url(url)?.1
        }
        (None, Some(svg)) => svg.as_bytes().to_vec(),
        _ => return None,
    };
    Some(match image_format(&bytes) {
        Some(format) => Thumb::Ready(Arc::new(Image::from_bytes(format, bytes))),
        None => Thumb::None,
    })
}

/// Whether a preview of `a` is worth a download: an image on the web.
pub(super) fn fetchable(a: &AssetRef) -> bool {
    matches!(a.kind, AssetKind::Image | AssetKind::Svg | AssetKind::Icon)
        && a.url.as_deref().is_some_and(|u| {
            let u = u.to_ascii_lowercase();
            u.starts_with("http://") || u.starts_with("https://")
        })
}

/// The image format of `bytes` by their first bytes (SVG by its markup): what GPUI can draw.
pub(super) fn image_format(bytes: &[u8]) -> Option<ImageFormat> {
    let head = &bytes[..bytes.len().min(512)];
    if head.starts_with(b"\x89PNG") {
        Some(ImageFormat::Png)
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if head.starts_with(b"GIF8") {
        Some(ImageFormat::Gif)
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if head.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else if head.starts_with(&[0, 0, 1, 0]) {
        Some(ImageFormat::Ico)
    } else if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        Some(ImageFormat::Tiff)
    } else {
        let text = String::from_utf8_lossy(head).to_ascii_lowercase();
        text.contains("<svg").then_some(ImageFormat::Svg)
    }
}

fn kind_icon(kind: AssetKind) -> Lucide {
    match kind {
        AssetKind::Video => Lucide::Film,
        AssetKind::Lottie => Lucide::Play,
        AssetKind::Font => Lucide::Type,
        _ => Lucide::Image,
    }
}

/// The preview tile: the image on a light tile (most marks are drawn for a light page), or
/// the kind's icon while it loads or when there is none.
fn preview(a: &AssetRef, thumbs: &HashMap<String, Thumb>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let tile = div()
        .size(px(THUMB))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .overflow_hidden();
    match thumbs.get(&thumb_key(a)) {
        Some(Thumb::Ready(image)) => tile
            .bg(gpui_kit::rgb(0xf4f4f5))
            .p(px(4.))
            .child(img(image.clone()).max_w_full().max_h_full())
            .into_any_element(),
        Some(Thumb::Loading) => tile.bg(theme.muted).into_any_element(),
        _ => tile
            .bg(theme.secondary)
            .child(panel::glyph(kind_icon(a.kind), cx))
            .into_any_element(),
    }
}

pub(super) fn render(
    scan: &Scan,
    kind: AssetKind,
    include_videos: bool,
    thumbs: &HashMap<String, Thumb>,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let counts = asset_counts(scan);
    // Kinds as light filters: the tab bar above stays the one strong row.
    let filters = h_flex()
        .gap_1()
        .flex_wrap()
        .children(counts.iter().enumerate().map(|(ix, (k, n))| {
            let k = *k;
            Button::new(("design-asset-kind", ix))
                .ghost()
                .xsmall()
                .selected(k == kind)
                .label(format!("{} {n}", kind_title(k)))
                .accessibility_label(format!("{} ({n})", kind_title(k)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.asset_kind = Some(k);
                    this.request_thumbnails(cx);
                    cx.notify();
                }))
        }));
    let all = save_all_set(scan, include_videos).len();
    let videos = counts
        .iter()
        .find(|(k, _)| *k == AssetKind::Video)
        .map_or(0, |(_, n)| *n);
    let header = h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(muted)
                .child(if all == 1 {
                    "1 file to save".to_string()
                } else {
                    format!("{all} files to save")
                }),
        )
        .when(videos > 0, |el| {
            el.child(
                Checkbox::new("design-assets-videos")
                    .label("Videos")
                    .checked(include_videos)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.include_videos = *checked;
                        cx.notify();
                    })),
            )
        })
        .child(
            Button::new("design-assets-save-all")
                .outline()
                .xsmall()
                .icon(Lucide::Download)
                .label(format!("Save all {all}"))
                .disabled(all == 0)
                .tooltip("Download them to the export folder's assets/")
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(DesignEvent::SaveAll { include_videos });
                })),
        );

    let list = shown(scan, kind);
    let extra = list.len().saturating_sub(MAX_ROWS);
    let body = if list.is_empty() {
        let (title, detail) = empty_text(kind);
        panel::empty(title, detail, None, cx).into_any_element()
    } else {
        let rows: Vec<AnyElement> = list
            .into_iter()
            .take(MAX_ROWS)
            .enumerate()
            .map(|(ix, a)| asset_row(ix, a, thumbs, cx))
            .collect();
        v_flex()
            .gap_1()
            .children(rows)
            .when(extra > 0, |el| {
                el.child(
                    div()
                        .pt_1()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{extra} more not shown; Save all includes them.")),
                )
            })
            .into_any_element()
    };
    v_flex()
        .gap_3()
        .child(filters)
        .child(header)
        .child(panel::section(
            kind_title(kind),
            Some(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(shown(scan, kind).len().to_string())
                    .into_any_element(),
            ),
            body,
            cx,
        ))
}

fn asset_row(
    ix: usize,
    a: &AssetRef,
    thumbs: &HashMap<String, Thumb>,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let selector = a.used_on.first().cloned();
    let one = a.clone();
    let label = row_label(a);
    let meta = [Some(kind_word(a.kind).to_string()), dimensions(a)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let where_used = selector.clone();
    let full = selector.clone().unwrap_or_default();
    let flash = selector.clone();
    let save_label = format!("Save {label}");
    h_flex()
        .id(("design-asset", ix))
        .min_h(px(52.))
        .px_1()
        .mx(px(-4.))
        .gap_3()
        .items_center()
        .rounded(px(6.))
        .when(selector.is_some(), |el| el.cursor_pointer())
        .hover(|el| el.bg(theme.secondary))
        .child(preview(a, thumbs, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().truncate().child(label))
                .child(div().text_xs().truncate().text_color(muted).child(meta))
                .children(where_used.map(|s| {
                    div()
                        .id(("design-asset-used", ix))
                        .text_xs()
                        .truncate()
                        .text_color(muted)
                        .font_family(theme.mono_font_family.clone())
                        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
                        .child(s)
                })),
        )
        .child(
            div()
                .flex_none()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(("design-asset-save", ix))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Download)
                        .tooltip("Save to the export folder's assets/")
                        .accessibility_label(save_label)
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            cx.emit(DesignEvent::SaveAssets(vec![one.clone()]));
                        })),
                ),
        )
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
            if let Some(s) = &flash {
                cx.emit(DesignEvent::Flash(s.clone()));
            }
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{
        asset_counts, fetchable, first_kind, image_format, local_thumb, row_label, save_all_set,
        thumb_key,
    };
    use crate::browser::design::Thumb;
    use gpui_kit::ImageFormat;
    use sawit_core::site_design::scan::{AssetKind, AssetRef, Scan};

    fn asset(kind: AssetKind, label: &str) -> AssetRef {
        AssetRef {
            kind,
            url: Some("https://a.dev/x".into()),
            svg: None,
            label: label.into(),
            width: None,
            height: None,
            used_on: vec![],
            logo: false,
        }
    }

    #[test]
    fn asset_counts_keep_the_five_tabs_in_order() {
        let scan = Scan {
            assets: vec![
                asset(AssetKind::Svg, "a"),
                asset(AssetKind::Image, "b"),
                asset(AssetKind::Image, "c"),
                asset(AssetKind::Font, "f"),
            ],
            ..Scan::default()
        };
        assert_eq!(
            asset_counts(&scan),
            [
                (AssetKind::Image, 2),
                (AssetKind::Svg, 1),
                (AssetKind::Icon, 0),
                (AssetKind::Video, 0),
                (AssetKind::Lottie, 0),
            ]
        );
    }

    /// The tab opens on the first kind the page has, not on an empty Images.
    #[test]
    fn the_first_kind_with_assets_shows_first() {
        let scan = Scan {
            assets: vec![asset(AssetKind::Icon, "a"), asset(AssetKind::Svg, "b")],
            ..Scan::default()
        };
        assert_eq!(first_kind(&scan), AssetKind::Svg);
        assert_eq!(first_kind(&Scan::default()), AssetKind::Image);
    }

    #[test]
    fn save_all_leaves_videos_unless_asked() {
        let scan = Scan {
            assets: vec![asset(AssetKind::Image, "a"), asset(AssetKind::Video, "v")],
            ..Scan::default()
        };
        assert_eq!(save_all_set(&scan, false).len(), 1);
        assert_eq!(save_all_set(&scan, true).len(), 2);
    }

    #[test]
    fn a_logo_is_labelled_logo() {
        let mut a = asset(AssetKind::Svg, "Apple");
        a.logo = true;
        assert_eq!(row_label(&a), "logo");
    }

    #[test]
    fn image_formats_are_read_from_their_bytes() {
        assert_eq!(image_format(b"\x89PNG\r\n"), Some(ImageFormat::Png));
        assert_eq!(
            image_format(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            image_format(b"RIFF\0\0\0\0WEBPVP8"),
            Some(ImageFormat::Webp)
        );
        assert_eq!(image_format(&[0, 0, 1, 0, 1]), Some(ImageFormat::Ico));
        assert_eq!(
            image_format(b"<?xml version=\"1.0\"?>\n<svg xmlns=\"\">"),
            Some(ImageFormat::Svg)
        );
        assert_eq!(image_format(b"{\"v\":\"5.7\"}"), None);
    }

    /// Inline SVG and `data:` images preview at once; images on the web are fetched; video
    /// and Lottie show their kind's icon.
    #[test]
    fn previews_come_from_where_the_asset_is() {
        let mut svg = asset(AssetKind::Svg, "logo");
        svg.url = None;
        svg.svg = Some("<svg viewBox=\"0 0 1 1\"></svg>".into());
        assert!(matches!(local_thumb(&svg), Some(Thumb::Ready(_))));
        assert!(thumb_key(&svg).starts_with("svg:"));
        assert!(!fetchable(&svg));

        let mut data = asset(AssetKind::Image, "dot");
        data.url = Some("data:image/svg+xml,%3Csvg%3E%3C/svg%3E".into());
        assert!(matches!(local_thumb(&data), Some(Thumb::Ready(_))));
        assert!(!fetchable(&data));

        let web = asset(AssetKind::Image, "hero");
        assert!(local_thumb(&web).is_none());
        assert!(fetchable(&web));
        assert!(!fetchable(&asset(AssetKind::Video, "v")));
    }
}
