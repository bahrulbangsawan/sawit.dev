//! The page's answers to the design ops, and the logic that turns them into a [`Scan`], a
//! [`SheetRules`] or a [`StyleInfo`]. The page (`browser/page/design/`) only collects what it
//! reads from the DOM; the decisions on it are made here.
//!
//! A scan takes two calls to the page: `scan` collects, [`plan`] reads its colours and picks
//! the buttons to sample, `scan_samples` ([`ScanPlan::samples_op`]) samples them with the
//! other components, and [`ScanPlan::finish`] fits the answer to its budget.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::computed::{self, Fallbacks};
use super::inspect::StyleInfo;
use super::scan::{AssetRef, ColorUse, ComponentSample, FontFace, Scan, SheetRules, TextSample};

/// At most this many colours, the most used first.
pub const MAX_COLORS: usize = 500;
/// The scan's answer, as JSON, in bytes: over it, the heaviest list is halved until it fits.
pub const MAX_ANSWER: usize = 2 * 1024 * 1024;
/// "Chromatic": OKLCH chroma at least this.
pub const CHROMATIC: f64 = 0.03;

/// The focus-ring colours a page's rules set, in their order: on `:focus-visible` rules and on
/// `:focus` ones. The first that isn't transparent is the ring.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusRings {
    pub visible: Vec<String>,
    pub plain: Vec<String>,
}

impl FocusRings {
    pub fn ring(&self, fallbacks: &Fallbacks) -> Option<String> {
        let first = |rings: &[String]| {
            rings
                .iter()
                .find(|r| !computed::transparent(Some(r), fallbacks))
                .cloned()
        };
        first(&self.visible).or_else(|| first(&self.plain))
    }
}

/// Button-like elements of one look (background, colour, radius, padding). An element counts
/// when it is filled or has a border, so which ones count waits on the background's reading.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ButtonLook {
    pub background: String,
    /// All of them, and the place in the page of the first.
    pub count: u32,
    pub first: u32,
    /// Those with a border, and the place of the first.
    pub bordered: u32,
    pub first_bordered: Option<u32>,
}

/// What the page's `scan` op answers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageScan {
    pub url: String,
    pub title: String,
    pub viewport: (u32, u32),
    pub truncated: bool,
    /// Every colour seen in each role, transparent ones too, in the order first seen.
    pub colors: Vec<ColorUse>,
    pub texts: Vec<TextSample>,
    pub spacing: Vec<(f64, u32)>,
    pub radii: Vec<(String, u32)>,
    pub shadows: Vec<(String, u32)>,
    /// The body's background and the root's.
    pub body_backgrounds: Vec<Option<String>>,
    pub focus_rings: FocusRings,
    pub font_faces: Vec<FontFace>,
    pub variables: Vec<(String, String)>,
    pub breakpoints: Vec<u32>,
    pub assets: Vec<AssetRef>,
    /// Button looks, in the order first seen; the `scan_samples` op names one by its index.
    pub buttons: Vec<ButtonLook>,
    pub blocked_sheets: Vec<String>,
    pub manifest: Option<String>,
    pub color_fallbacks: Fallbacks,
}

/// One button look to sample: its index in [`PageScan::buttons`], and whether its first
/// element with a border stands for it (an unfilled look) rather than its first element.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ButtonPick {
    pub id: usize,
    pub bordered: bool,
}

/// A button look that counts: where the page met it first, its index, how many count, whether
/// it is chromatic, and how to sample it.
type Look = (u32, usize, u32, bool, ButtonPick);

/// A scan waiting for its component samples.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanPlan {
    pub scan: Scan,
    pub button: Option<ButtonPick>,
    pub secondary: Option<ButtonPick>,
}

/// Reads the scan's colours: drops transparent ones, ranks the rest, sets the body's
/// background and the focus ring, and picks the primary button (the most used chromatic look)
/// and the secondary one (the most used other look).
pub fn plan(page: PageScan) -> ScanPlan {
    let fb = &page.color_fallbacks;
    let mut colors: Vec<ColorUse> = page
        .colors
        .into_iter()
        .filter(|c| !computed::transparent(Some(&c.value), fb))
        .collect();
    colors.sort_by_key(|c| std::cmp::Reverse(c.count));
    colors.truncate(MAX_COLORS);
    let body_background = page
        .body_backgrounds
        .iter()
        .take(2)
        .flatten()
        .find(|c| !computed::transparent(Some(c), fb))
        .cloned();
    let focus_ring = page.focus_rings.ring(fb);

    // The looks that count, as the page met them.
    let mut looks: Vec<Look> = page
        .buttons
        .iter()
        .enumerate()
        .filter_map(|(id, b)| {
            let filled = !computed::transparent(Some(&b.background), fb);
            let (count, first) = if filled {
                (b.count, b.first)
            } else {
                (b.bordered, b.first_bordered?)
            };
            (count > 0).then(|| {
                let chromatic = filled && computed::chroma(&b.background, fb) >= CHROMATIC;
                let pick = ButtonPick {
                    id,
                    bordered: !filled,
                };
                (first, id, count, chromatic, pick)
            })
        })
        .collect();
    looks.sort_by_key(|l| l.0);
    // The first of the most used (a later one must be used more to win).
    let top = |ok: &dyn Fn(&Look) -> bool| {
        looks.iter().filter(|l| ok(l)).fold(
            None::<&Look>,
            |best, l| match best {
                Some(b) if l.2 <= b.2 => Some(b),
                _ => Some(l),
            },
        )
    };
    let primary = top(&|l| l.3);
    let secondary = primary.and_then(|p| top(&|l| l.1 != p.1));
    ScanPlan {
        button: primary.map(|l| l.4),
        secondary: secondary.map(|l| l.4),
        scan: Scan {
            url: page.url,
            title: page.title,
            viewport: page.viewport,
            truncated: page.truncated,
            colors,
            texts: page.texts,
            spacing: page.spacing,
            radii: page.radii,
            shadows: page.shadows,
            body_background,
            focus_ring,
            font_faces: page.font_faces,
            variables: page.variables,
            breakpoints: page.breakpoints,
            assets: page.assets,
            components: Vec::new(),
            blocked_sheets: page.blocked_sheets,
            manifest: page.manifest,
        },
    }
}

/// What the page's `scan_samples` op answers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageSamples {
    pub components: Vec<ComponentSample>,
}

impl ScanPlan {
    /// The page op that samples the components: the buttons picked here, and the input, card,
    /// nav and link the page picked.
    pub fn samples_op(&self) -> Value {
        json!({ "op": "scan_samples", "button": self.button, "secondary": self.secondary })
    }

    /// The scan with its samples, within [`MAX_ANSWER`].
    pub fn finish(self, samples: PageSamples) -> Scan {
        let mut scan = self.scan;
        scan.components = samples.components;
        fit(&mut scan, MAX_ANSWER);
        scan
    }
}

/// Over `max` bytes (as the page's `JSON.stringify` wrote the scan), halves the heaviest list
/// until it fits, at most 40 times, and marks the scan truncated.
pub fn fit(scan: &mut Scan, max: usize) {
    const LISTS: [&str; 9] = [
        "assets",
        "components",
        "texts",
        "colors",
        "variables",
        "font_faces",
        "shadows",
        "radii",
        "spacing",
    ];
    let size = |scan: &Scan| js_json_len(&serde_json::to_value(scan).unwrap_or_default());
    let mut bytes = size(scan);
    for _ in 0..40 {
        if bytes <= max {
            break;
        }
        let value = serde_json::to_value(&*scan).unwrap_or_default();
        let mut heaviest = None;
        let mut weight = 0;
        for name in LISTS {
            let list = &value[name];
            let w = if list.as_array().is_some_and(|a| !a.is_empty()) {
                js_json_len(list)
            } else {
                0
            };
            if w > weight {
                weight = w;
                heaviest = Some(name);
            }
        }
        let Some(name) = heaviest else { break };
        fn halve<T>(v: &mut Vec<T>) {
            v.truncate(v.len() / 2);
        }
        match name {
            "assets" => halve(&mut scan.assets),
            "components" => halve(&mut scan.components),
            "texts" => halve(&mut scan.texts),
            "colors" => halve(&mut scan.colors),
            "variables" => halve(&mut scan.variables),
            "font_faces" => halve(&mut scan.font_faces),
            "shadows" => halve(&mut scan.shadows),
            "radii" => halve(&mut scan.radii),
            _ => halve(&mut scan.spacing),
        }
        scan.truncated = true;
        bytes = size(scan);
    }
}

/// The length in UTF-8 bytes of `JSON.stringify(v)` in a browser: numbers as JavaScript writes
/// them (`16`, not `16.0`).
pub fn js_json_len(v: &Value) -> usize {
    match v {
        Value::Null => 4,
        Value::Bool(b) => {
            if *b {
                4
            } else {
                5
            }
        }
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => js_number(f).len(),
            _ => n.to_string().len(),
        },
        Value::String(s) => js_string_len(s),
        Value::Array(a) => 2 + a.len().saturating_sub(1) + a.iter().map(js_json_len).sum::<usize>(),
        Value::Object(m) => {
            2 + m.len().saturating_sub(1)
                + m.iter()
                    .map(|(k, v)| js_string_len(k) + 1 + js_json_len(v))
                    .sum::<usize>()
        }
    }
}

fn js_string_len(s: &str) -> usize {
    2 + s
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\u{8}' | '\u{c}' | '\n' | '\r' | '\t' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        })
        .sum::<usize>()
}

/// A number as JavaScript's `String(n)` writes it.
pub fn js_number(f: f64) -> String {
    if !f.is_finite() {
        // JSON.stringify writes null.
        return "null".into();
    }
    if f == 0.0 {
        return "0".into();
    }
    let a = f.abs();
    if (1e-6..1e21).contains(&a) {
        if f.fract() == 0.0 {
            format!("{}", f as i128)
        } else {
            format!("{f}")
        }
    } else {
        let e = format!("{f:e}");
        match e.split_once('e') {
            Some((m, x)) if !x.starts_with('-') => format!("{m}e+{x}"),
            _ => e,
        }
    }
}

/// What the page's `rules` op answers, for the sheets Sawit fetched for it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageRules {
    pub font_faces: Vec<FontFace>,
    pub variables: Vec<(String, String)>,
    pub breakpoints: Vec<u32>,
    pub focus_rings: FocusRings,
    pub color_fallbacks: Fallbacks,
}

impl From<PageRules> for SheetRules {
    fn from(page: PageRules) -> Self {
        SheetRules {
            focus_ring: page.focus_rings.ring(&page.color_fallbacks),
            font_faces: page.font_faces,
            variables: page.variables,
            breakpoints: page.breakpoints,
        }
    }
}

/// What the page's `style` op answers: the element's styles, with the background colours of
/// it and its ancestors (from it up) for its effective background.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageStyle {
    #[serde(flatten)]
    pub info: StyleInfo,
    #[serde(default)]
    pub backgrounds: Vec<String>,
    #[serde(default)]
    pub color_fallbacks: Fallbacks,
}

impl From<PageStyle> for StyleInfo {
    fn from(page: PageStyle) -> Self {
        let mut info = page.info;
        info.effective_background = Some(computed::effective_background(
            &page.backgrounds,
            &page.color_fallbacks,
        ));
        info
    }
}

#[cfg(test)]
#[path = "tests/page.rs"]
mod tests;
