//! The Assets tab's tests: kinds and counts, Save all, labels, and where previews come from.

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
