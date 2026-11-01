//! Asset file names, data URLs, and the assets a page's fonts become.

use std::collections::HashSet;

use super::*;

#[test]
fn file_names() {
    let mut t = HashSet::new();
    let hero = asset(
        AssetKind::Image,
        Some("https://x.com/a/hero_large.jpg?x=1"),
        None,
        "Hero photo",
    );
    assert_eq!(file_name(&hero, &mut t), "hero-photo.jpg");
    assert_eq!(file_name(&hero, &mut t), "hero-photo-2.jpg");
    let cart = asset(
        AssetKind::Icon,
        Some("https://x.com/icons/cart.svg"),
        None,
        "",
    );
    assert_eq!(file_name(&cart, &mut t), "cart.svg");
    let logo = asset(AssetKind::Svg, None, Some("<svg/>"), "logo");
    assert_eq!(file_name(&logo, &mut t), "logo.svg");
    let eot = asset(
        AssetKind::Font,
        Some("https://x.com/f/icons.eot"),
        None,
        "Icons",
    );
    assert_eq!(file_name(&eot, &mut t), "icons.eot");
    let small = asset(AssetKind::Icon, None, Some("<svg/>"), "logo");
    assert_eq!(file_name(&small, &mut t), "logo-2.svg");
    let data = asset(
        AssetKind::Image,
        Some("data:image/png;base64,AAAA"),
        None,
        "",
    );
    assert_eq!(file_name(&data, &mut t), "image.png");
}

#[test]
fn data_urls() {
    assert_eq!(
        decode_data_url("data:image/svg+xml;base64,PHN2Zy8+"),
        Some(("image/svg+xml".into(), b"<svg/>".to_vec()))
    );
    assert_eq!(
        decode_data_url("data:text/plain,hello%20world").unwrap().1,
        b"hello world"
    );
}

#[test]
fn fonts_become_assets() {
    let f = FontFace {
        family: "SF Pro Text".into(),
        weight: "400".into(),
        style: "normal".into(),
        src: vec![
            "https://x.com/a.woff2".into(),
            "https://x.com/a.woff".into(),
        ],
    };
    let a = font_assets(&[f]);
    assert_eq!(a.len(), 2);
    assert!(
        a.iter()
            .all(|x| x.kind == AssetKind::Font && x.label == "SF Pro Text-400-normal")
    );
}
