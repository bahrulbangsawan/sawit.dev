//! Devices, sizes and how they fit the tab.

use serde_json::json;

use crate::browser::{BrowserOp, DEVICES, Fit, ViewportMode, device_by_name, fit, resize_mode};

#[test]
fn fits_devices_to_areas() {
    let f = fit((393, 852), (1000., 1000.));
    assert_eq!(
        f,
        Fit {
            scale: 1.0,
            width: 393.,
            height: 852.
        }
    );
    let f = fit((1440, 900), (720., 900.));
    assert_eq!((f.width, f.height), (720., 450.));
    assert!((f.scale - 0.5).abs() < 1e-3);
    assert!((fit((393, 852), (393., 426.)).scale - 0.5).abs() < 5e-3);
}

/// WebKit rounds the view to whole points, and `innerWidth` is whole too:
/// 393 × 0.3644 = 143.2 pt became 143 pt, so `innerWidth` was 392.
#[test]
fn scaled_devices_keep_their_css_width() {
    for (size, area) in [
        ((393, 852), (500., 310.5)),
        ((393, 852), (232., 384.)),
        ((412, 915), (301.7, 600.)),
        ((1440, 900), (700., 435.)),
        ((834, 1194), (333.3, 333.3)),
    ] {
        let f = fit(size, area);
        assert_eq!(f.width, f.width.round(), "{size:?} in {area:?}");
        assert_eq!(f.height, f.height.round(), "{size:?} in {area:?}");
        assert!(f.width <= area.0, "{size:?} in {area:?}");
        let css_width = f64::from(f.width) / f.scale;
        assert_eq!(css_width as u32, size.0, "{size:?} in {area:?}");
        let css_height = f64::from(f.height) / f.scale;
        assert!(
            (css_height - f64::from(size.1)).abs() < 1.0 / f.scale,
            "{size:?}"
        );
    }
}

#[test]
fn devices_by_name() {
    for name in [
        "iphone 15",
        "IPAD PRO 11",
        "Desktop",
        "iPad Pro 11\"",
        "iPad Pro 11″",
    ] {
        assert!(device_by_name(name).is_some(), "{name}");
    }
    assert!(device_by_name("watch").is_none());
    assert_eq!(DEVICES.len(), 6);
}

#[test]
fn resize_modes() {
    assert_eq!(
        resize_mode(Some("responsive"), None, None),
        Ok(ViewportMode::Responsive)
    );
    assert_eq!(
        resize_mode(Some("iPhone 15"), None, None),
        Ok(ViewportMode::Fixed {
            name: Some("iPhone 15".into()),
            width: 393,
            height: 852
        })
    );
    assert_eq!(
        resize_mode(None, Some(800), Some(600)),
        Ok(ViewportMode::Fixed {
            name: None,
            width: 800,
            height: 600
        })
    );
    assert_eq!(
        resize_mode(None, Some(100), Some(600)),
        Err("Width and height must be between 200 and 3840.".into())
    );
    assert_eq!(
            resize_mode(None, None, None),
            Err("Give a device (iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive) or a width and height.".into())
        );
    assert_eq!(
            resize_mode(Some("watch"), None, None),
            Err("Unknown device “watch”. Use iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive.".into())
        );
    let op: BrowserOp = serde_json::from_str(r#"{"op":"resize","device":"iPhone 15"}"#).unwrap();
    assert_eq!(
        op,
        BrowserOp::Resize {
            device: Some("iPhone 15".into()),
            width: None,
            height: None
        }
    );
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        json!({"op": "resize", "device": "iPhone 15"})
    );
    assert!(op.is_native() && op.shows_tab());
}
