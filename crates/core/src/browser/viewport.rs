//! How the page is sized in its tab: filling it, or as a device or a fixed size, scaled to fit.

use serde::{Deserialize, Serialize};

/// A device the browser tab can imitate.
#[derive(Debug, PartialEq)]
pub struct Device {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
}

pub const DEVICES: &[Device] = &[
    Device {
        name: "iPhone 15",
        width: 393,
        height: 852,
    },
    Device {
        name: "Pixel 8",
        width: 412,
        height: 915,
    },
    Device {
        name: "iPad mini",
        width: 744,
        height: 1133,
    },
    Device {
        name: "iPad Pro 11″",
        width: 834,
        height: 1194,
    },
    Device {
        name: "Laptop",
        width: 1280,
        height: 800,
    },
    Device {
        name: "Desktop",
        width: 1440,
        height: 900,
    },
];

fn device_key(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !matches!(c, '″' | '"'))
        .collect::<String>()
        .to_lowercase()
}

/// A device by name, ignoring case and the inch mark.
pub fn device_by_name(name: &str) -> Option<&'static Device> {
    let key = device_key(name);
    DEVICES.iter().find(|d| device_key(d.name) == key)
}

/// How the page is sized in its tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum ViewportMode {
    /// Fills the tab.
    #[default]
    Responsive,
    Fixed {
        name: Option<String>,
        width: u32,
        height: u32,
    },
}

/// A device size scaled down to fit an area.
#[derive(Debug, PartialEq)]
pub struct Fit {
    pub scale: f64,
    pub width: f32,
    pub height: f32,
}

/// `size` (CSS px) at the largest scale up to 1 that fits `area`.
///
/// Scaled down, the view gets a whole number of points (WebKit rounds the
/// view's size, then `innerWidth` is that size over the zoom, whole), and
/// the zoom is set so the width comes out a quarter pixel over the device's:
/// `innerWidth` is exactly the device width. The height lands within a pixel
/// or so of the device's.
pub fn fit(size: (u32, u32), area: (f32, f32)) -> Fit {
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let scale = (f64::from(area.0) / w)
        .min(f64::from(area.1) / h)
        .clamp(0.0, 1.0);
    if scale >= 1.0 {
        return Fit {
            scale: 1.0,
            width: w as f32,
            height: h as f32,
        };
    }
    let width = (w * scale).floor().max(1.0);
    let scale = width / (w + 0.25);
    Fit {
        scale,
        width: width as f32,
        height: ((h + 0.25) * scale).round().max(1.0) as f32,
    }
}

/// The mode an agent asks for: a device name, `responsive`, or a size.
pub fn resize_mode(
    device: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<ViewportMode, String> {
    if let Some(name) = device {
        if name.trim().eq_ignore_ascii_case("responsive") {
            return Ok(ViewportMode::Responsive);
        }
        return device_by_name(name)
            .map(|d| ViewportMode::Fixed {
                name: Some(d.name.into()),
                width: d.width,
                height: d.height,
            })
            .ok_or_else(|| {
                format!(
                    "Unknown device “{}”. Use iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive.",
                    name.trim()
                )
            });
    }
    match (width, height) {
        (Some(w), Some(h)) => {
            if (200..=3840).contains(&w) && (200..=3840).contains(&h) {
                Ok(ViewportMode::Fixed { name: None, width: w, height: h })
            } else {
                Err("Width and height must be between 200 and 3840.".into())
            }
        }
        _ => Err("Give a device (iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive) or a width and height.".into()),
    }
}
