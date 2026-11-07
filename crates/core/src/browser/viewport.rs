//! How the page is sized in its tab: filling it, or as a device or a fixed size, scaled to fit.

use serde::{Deserialize, Serialize};

/// What kind of screen a device has: the groups of the device menu and of MCP's errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceClass {
    Mobile,
    Tablet,
    Laptop,
    Desktop,
}

impl DeviceClass {
    pub const ALL: [DeviceClass; 4] = [
        DeviceClass::Mobile,
        DeviceClass::Tablet,
        DeviceClass::Laptop,
        DeviceClass::Desktop,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DeviceClass::Mobile => "Mobile",
            DeviceClass::Tablet => "Tablet",
            DeviceClass::Laptop => "Laptop",
            DeviceClass::Desktop => "Desktop",
        }
    }

    /// Whether its devices turn on their side.
    pub fn rotates(self) -> bool {
        matches!(self, DeviceClass::Mobile | DeviceClass::Tablet)
    }
}

/// A device the browser tab can imitate.
#[derive(Debug, PartialEq)]
pub struct Device {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    /// The device's pixel ratio, which the page sees as `devicePixelRatio` while the tab
    /// imitates it. `None` for computers: their pages draw at the Mac's own ratio, as forcing 1×
    /// would blur them on a Retina screen.
    pub dpr: Option<f64>,
    pub class: DeviceClass,
}

impl Device {
    /// The scale a capture of it is taken at by default: its own ratio (3× for phones, 2× for
    /// Apple tablets), 2× for laptops, 1× for desktops (their CSS size is their pixel size).
    pub fn default_scale(&self) -> f64 {
        if let Some(r) = self.dpr {
            return r;
        }
        match self.class {
            DeviceClass::Desktop => 1.,
            _ => 2.,
        }
    }
}

const fn d(
    name: &'static str,
    width: u32,
    height: u32,
    dpr: Option<f64>,
    class: DeviceClass,
) -> Device {
    Device {
        name,
        width,
        height,
        dpr,
        class,
    }
}

use DeviceClass::{Desktop as Dk, Laptop as Lp, Mobile as Mb, Tablet as Tb};

/// Every preset, by class. The first six names are the ones the toolbar had before (kept so saved
/// tabs and scripts still find them).
pub const DEVICES: &[Device] = &[
    d("iPhone SE", 375, 667, Some(2.), Mb),
    d("iPhone 15", 393, 852, Some(3.), Mb),
    d("iPhone 15 Pro Max", 430, 932, Some(3.), Mb),
    d("Pixel 8", 412, 915, Some(2.625), Mb),
    d("Galaxy S", 360, 780, Some(3.), Mb),
    d("Small Android", 320, 568, Some(2.), Mb),
    d("iPad mini", 744, 1133, Some(2.), Tb),
    d("iPad Air", 820, 1180, Some(2.), Tb),
    d("iPad Pro 11″", 834, 1194, Some(2.), Tb),
    d("iPad Pro 13″", 1032, 1376, Some(2.), Tb),
    d("Android tablet", 800, 1280, Some(2.), Tb),
    d("Laptop", 1280, 800, None, Lp),
    d("Laptop 1366", 1366, 768, None, Lp),
    d("MacBook Air", 1440, 900, None, Lp),
    d("MacBook Pro 14″", 1512, 982, None, Lp),
    d("Laptop 1536", 1536, 864, None, Lp),
    d("Desktop", 1440, 900, None, Dk),
    d("Full HD", 1920, 1080, None, Dk),
    d("QHD", 2560, 1440, None, Dk),
    d("4K", 3840, 2160, None, Dk),
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
/// Scaled down, the view gets a whole number of points (WebKit rounds the view's size), and the
/// page's `innerWidth` and `innerHeight` are the view's size over the scale, cut to whole pixels.
/// So the view's width and height are whole points that over one scale both come out within half
/// a pixel above the device's size: the page sees exactly the device's width and height (393 ×
/// 852, not 851). The scale gives up a hair of the largest fit when it must to find such a pair.
pub fn fit(size: (u32, u32), area: (f32, f32)) -> Fit {
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let largest = (f64::from(area.0) / w)
        .min(f64::from(area.1) / h)
        .clamp(0.0, 1.0);
    if largest >= 1.0 {
        return Fit {
            scale: 1.0,
            width: w as f32,
            height: h as f32,
        };
    }
    exact_fit(w, h, largest, area).unwrap_or_else(|| {
        // No exact pair this small (a view a few points across): the width exact, as before.
        let width = (w * largest).floor().max(1.0);
        let scale = width / (w + 0.25);
        Fit {
            scale,
            width: width as f32,
            height: ((h + 0.25) * scale).round().max(1.0) as f32,
        }
    })
}

/// The largest scale up to `largest` at which a view of whole points `W` × `H` shows `w` × `h`
/// CSS px: `W / scale` and `H / scale` both in `[w, w + 0.5)`. Narrower views are tried until
/// one has a height that fits too; the scale is the middle of the range both allow.
fn exact_fit(w: f64, h: f64, largest: f64, area: (f32, f32)) -> Option<Fit> {
    let (max_w, max_h) = (f64::from(area.0).floor(), f64::from(area.1).floor());
    let top = ((w + 0.5) * largest).floor().min(max_w);
    for step in 0..64 {
        let width = top - f64::from(step);
        if width < 1.0 {
            return None;
        }
        // The scales that show this width: (width / (w + 0.5), width / w], and no larger than
        // the area allows.
        let (lo, hi) = (width / (w + 0.5), (width / w).min(largest));
        if lo >= hi {
            continue;
        }
        // The heights those scales allow, whole: the tallest fitting one first.
        let mut height = ((h + 0.5) * hi).floor().min(max_h);
        while height >= (h * lo).ceil().max(1.0) {
            let lo = lo.max(height / (h + 0.5));
            let hi = hi.min(height / h);
            if lo < hi {
                return Some(Fit {
                    scale: (lo + hi) / 2.0,
                    width: width as f32,
                    height: height as f32,
                });
            }
            height -= 1.0;
        }
    }
    None
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
                    "Unknown device “{}”. Use responsive, or one of {}.",
                    name.trim(),
                    device_list()
                )
            });
    }
    match (width, height) {
        (Some(w), Some(h)) => {
            if (200..=3840).contains(&w) && (200..=3840).contains(&h) {
                Ok(ViewportMode::Fixed {
                    name: None,
                    width: w,
                    height: h,
                })
            } else {
                Err("Width and height must be between 200 and 3840.".into())
            }
        }
        _ => Err(format!(
            "Give a device (responsive, or one of {}) or a width and height.",
            device_list()
        )),
    }
}

/// The devices by class, for messages: "Mobile: iPhone SE, iPhone 15, …; Tablet: …".
pub fn device_list() -> String {
    DeviceClass::ALL
        .iter()
        .map(|class| {
            let names: Vec<&str> = DEVICES
                .iter()
                .filter(|d| d.class == *class)
                .map(|d| d.name)
                .collect();
            format!("{}: {}", class.name(), names.join(", "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}
