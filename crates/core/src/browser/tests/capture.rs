//! Capture sizing, tiles, names and options.

use crate::browser::capture::*;
use crate::browser::{DEVICES, DeviceClass, device_by_name};

#[test]
fn every_class_has_devices_and_names_are_unique() {
    for class in DeviceClass::ALL {
        assert!(DEVICES.iter().any(|d| d.class == class), "{class:?}");
    }
    let mut names = std::collections::HashSet::new();
    for d in DEVICES {
        assert!(names.insert(d.name.to_lowercase()), "{}", d.name);
    }
    // The six the toolbar had keep their names and sizes.
    for (name, w, h) in [
        ("iPhone 15", 393, 852),
        ("Pixel 8", 412, 915),
        ("iPad mini", 744, 1133),
        ("iPad Pro 11″", 834, 1194),
        ("Laptop", 1280, 800),
        ("Desktop", 1440, 900),
    ] {
        let d = device_by_name(name).unwrap();
        assert_eq!((d.width, d.height), (w, h), "{name}");
    }
}

#[test]
fn default_scales_follow_the_device() {
    assert_eq!(capture_scale(None, Some("iPhone 15")), 3.);
    assert_eq!(capture_scale(None, Some("iPad Air")), 2.);
    assert_eq!(capture_scale(None, Some("Laptop")), 2.);
    assert_eq!(capture_scale(None, Some("Full HD")), 1.);
    assert_eq!(capture_scale(Some(5.), Some("iPhone 15")), 3.);
    assert_eq!(capture_scale(Some(0.5), None), 1.);
    assert_eq!(capture_scale(None, None), 2.);
}

#[test]
fn pixel_sizes_and_caps() {
    assert_eq!(pixel_size(393, 852, 3., None, false), (1179, 2556));
    assert_eq!(pixel_size(1920, 1080, 1., None, false), (1920, 1080));
    assert_eq!(pixel_size(1440, 900, 2., Some(1568), false), (1568, 980));
    assert_eq!(pixel_size(375, 667, 1., None, true), (374, 666));
}

#[test]
fn video_sizes_are_even_and_follow_resolutions() {
    assert_eq!(resolution("720p"), Some(720));
    assert_eq!(resolution("Full HD"), Some(1080));
    assert_eq!(resolution("4K"), Some(2160));
    assert_eq!(resolution("nope"), None);
    // A landscape page at 720p: 720 tall.
    assert_eq!(video_size(1280, 800, 2., Some(720), None), (1152, 720));
    // A phone at 1080p: 1080 wide (its short side).
    let (w, h) = video_size(393, 852, 3., Some(1080), None);
    assert_eq!(w, 1080);
    assert_eq!(h % 2, 0);
    // Capped at 3840 wide.
    assert_eq!(video_size(3840, 2160, 2., None, None).0, 3840);
    assert_eq!(video_size(1440, 900, 2., None, Some(1000)).0, 1000);
}

#[test]
fn mp4_wider_than_h264_moves_to_hevc() {
    assert_eq!(video_format_for(VideoFormat::Mp4, 1920).0, VideoFormat::Mp4);
    let (f, note) = video_format_for(VideoFormat::Mp4, 5000);
    assert_eq!(f, VideoFormat::Mov);
    assert!(note.unwrap().contains("HEVC"));
}

#[test]
fn tall_pages_are_tiled() {
    assert_eq!(
        tiles(1000, 2., 16_000),
        vec![Tile {
            top: 0,
            height: 1000
        }]
    );
    let t = tiles(20_000, 3., 16_000);
    assert_eq!(t.len(), 4);
    assert_eq!(
        t[0],
        Tile {
            top: 0,
            height: 5333
        }
    );
    assert_eq!(t.iter().map(|t| t.height).sum::<u32>(), 20_000);
    assert!(t.iter().all(|t| f64::from(t.height) * 3. <= 16_000.));
}

#[test]
fn file_names_say_host_device_scale() {
    assert_eq!(
        file_name("https://example.com/a?b", "iPhone 15", 3., true, "png", 1),
        "example.com-iphone-15-3x-full.png"
    );
    assert_eq!(
        file_name(
            "http://localhost:3000/",
            "iPad Pro 11″",
            2.,
            false,
            "jpg",
            2
        ),
        "localhost-3000-ipad-pro-11-2x-2.jpg"
    );
    assert_eq!(
        file_name("about:blank", "Pixel 8", 2.625, false, "png", 1),
        "about-blank-pixel-8-2_6x.png"
    );
}

#[test]
fn formats_parse() {
    assert_eq!(ImageFormat::parse("JPG").unwrap(), ImageFormat::Jpeg);
    assert!(ImageFormat::parse("tiff").is_err());
    assert_eq!(VideoFormat::parse("hevc").unwrap(), VideoFormat::Mov);
    assert!(VideoFormat::parse("avi").unwrap_err().contains("mp4"));
}
