//! The Assets segment: images, SVG, icons, video and Lottie found on the page, with save
//! buttons.

use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::scan::{AssetKind, AssetRef, Scan};

use super::{DesignEvent, DesignPanel};

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

/// How many assets of each tab, zero counts kept.
pub(super) fn asset_counts(scan: &Scan) -> Vec<(AssetKind, usize)> {
    KINDS
        .into_iter()
        .map(|k| (k, scan.assets.iter().filter(|a| a.kind == k).count()))
        .collect()
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

pub(super) fn render(
    scan: &Scan,
    kind: AssetKind,
    include_videos: bool,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    let chips =
        h_flex()
            .gap_1()
            .flex_wrap()
            .children(
                asset_counts(scan)
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (k, n))| {
                        Button::new(("design-asset-kind", ix))
                            .xsmall()
                            .label(format!("{} {n}", kind_title(k)))
                            .selected(k == kind)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.asset_kind = k;
                                cx.notify();
                            }))
                    }),
            );
    let total_saved = save_all_set(scan, include_videos).len();
    let toolbar = h_flex()
        .gap_2()
        .items_center()
        .child(
            Button::new("design-assets-save-all")
                .xsmall()
                .label("Save all")
                .disabled(total_saved == 0)
                .tooltip("Download into the export folder's assets/")
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(DesignEvent::SaveAll { include_videos });
                })),
        )
        .child(
            Checkbox::new("design-assets-videos")
                .label("Include videos")
                .checked(include_videos)
                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                    this.include_videos = *checked;
                    cx.notify();
                })),
        );
    let shown: Vec<&AssetRef> = scan.assets.iter().filter(|a| a.kind == kind).collect();
    let extra = shown.len().saturating_sub(MAX_ROWS);
    let rows = shown.into_iter().take(MAX_ROWS).enumerate().map(|(ix, a)| {
        let selector = a.used_on.first().cloned();
        let one = a.clone();
        let detail = [
            Some(kind_word(a.kind).to_string()),
            dimensions(a),
            selector.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        h_flex()
            .id(("design-asset", ix))
            .gap_2()
            .px_1()
            .py_1()
            .items_center()
            .rounded(theme.radius)
            .when(selector.is_some(), |el| el.cursor_pointer())
            .hover(|el| el.bg(theme.accent))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_xs().truncate().child(row_label(a)))
                    .child(div().text_xs().truncate().text_color(muted).child(detail)),
            )
            .child(
                Button::new(("design-asset-save", ix))
                    .xsmall()
                    .label("Save")
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(DesignEvent::SaveAssets(vec![one.clone()]));
                    })),
            )
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                if let Some(s) = &selector {
                    cx.emit(DesignEvent::Flash(s.clone()));
                }
            }))
    });
    v_flex()
        .gap_2()
        .child(chips)
        .child(toolbar)
        .child(
            v_flex()
                .gap_0p5()
                .border_t_1()
                .border_color(border)
                .pt_1()
                .children(rows),
        )
        .when(extra > 0, |el| {
            el.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{extra} more not shown.")),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{asset_counts, row_label, save_all_set};
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
}
