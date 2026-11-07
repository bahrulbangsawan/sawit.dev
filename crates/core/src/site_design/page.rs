//! The page's answers to the design ops, and the logic that turns them into a [`Scan`], a
//! [`SheetRules`] or a [`StyleInfo`]. The page (`browser/page/design/`) only collects what it
//! reads from the DOM; the decisions on it are made here.
//!
//! Every op carries the page's [`Limits`] ([`with_limits`]). A scan takes two calls to the
//! page: `scan` collects, [`plan`] reads, groups, ranks and caps it and picks the components to
//! sample, `scan_samples` ([`ScanPlan::samples_op`]) samples them with the
//! other components, and [`ScanPlan::finish`] fits the answer to its budget.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::computed::{self, Fallbacks};
use super::inspect::StyleInfo;
use super::scan::{AssetRef, ColorUse, ComponentSample, FontFace, Scan, SheetRules, TextSample};

mod budget;

pub use budget::{fit, js_json_len, js_number};

/// At most this many colours, the most used first.
pub const MAX_COLORS: usize = 500;
/// At most this many text styles, the most characters first.
pub const MAX_TEXTS: usize = 400;
/// At most this many CSS variables, the first the sheets set.
pub const MAX_VARIABLES: usize = 500;
/// Spacing is grouped to this step, in px.
pub const SPACING_STEP: f64 = 0.5;

/// How much of the page a design op reads, sent with every op (`limits`): the page has no
/// numbers of its own.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    /// Visible elements a scan reads.
    pub elements: u32,
    /// Elements it walks, hidden ones too.
    pub walked: u32,
    /// Assets it lists.
    pub assets: u32,
    /// The longest inline SVG it keeps, in characters.
    pub svg: u32,
    /// The longest `data:` address it keeps.
    pub data_url: u32,
    /// The longest element HTML it keeps.
    pub html: u32,
    /// Elements whose computed styles an element's HTML inlines.
    pub inlined: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            elements: 6000,
            walked: 60000,
            assets: 400,
            svg: 100 * 1024,
            data_url: 100 * 1024,
            html: 30 * 1024,
            inlined: 200,
        }
    }
}

/// `op` with the [`Limits`] the page reads within.
pub fn with_limits(mut op: Value) -> Value {
    if let Some(map) = op.as_object_mut() {
        map.insert("limits".into(), json!(Limits::default()));
    }
    op
}

/// A text style as the page computed it: its font size, weight, line height and letter spacing
/// as CSS text, which Rust reads ([`TextSample`]'s numbers) and groups by.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageText {
    pub tag: String,
    pub in_main: bool,
    pub family: String,
    pub used_family: Option<String>,
    pub size: String,
    pub weight: String,
    pub line_height: String,
    pub letter_spacing: String,
    pub color: String,
    pub chars: u32,
}

/// The page's `num`: a CSS number's value, 0 when it has none.
fn num(css: &str) -> f64 {
    let n = computed::parse_float(css);
    if n.is_finite() { n } else { 0.0 }
}

/// To 3 decimals, as the page rounded.
fn round3(n: f64) -> f64 {
    computed::js_round(n * 1000.0) / 1000.0 + 0.0
}

/// `normal`, or the px value to 3 decimals.
fn length(css: &str) -> Option<f64> {
    (css != "normal").then(|| round3(num(css)))
}

impl From<PageText> for TextSample {
    fn from(t: PageText) -> Self {
        let weight = computed::js_round(num(&t.weight));
        TextSample {
            tag: t.tag,
            in_main: t.in_main,
            family: t.family,
            used_family: t.used_family,
            size: num(&t.size) + 0.0,
            weight: if weight == 0.0 { 400 } else { weight as u16 },
            line_height: length(&t.line_height),
            letter_spacing: length(&t.letter_spacing),
            color: t.color,
            chars: t.chars,
        }
    }
}

/// The text styles: read, grouped by what they show (a style met again adds its characters to
/// the first), the most characters first, at most [`MAX_TEXTS`].
pub fn texts(page: Vec<PageText>) -> Vec<TextSample> {
    let mut out: Vec<TextSample> = Vec::new();
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for t in page.into_iter().map(TextSample::from) {
        let key = format!(
            "{}\n{}\n{}\n{}\n{:?}\n{:?}\n{}\n{}",
            t.tag, t.family, t.size, t.weight, t.line_height, t.letter_spacing, t.color, t.in_main
        );
        match at.get(&key) {
            Some(&i) => out[i].chars += t.chars,
            None => {
                at.insert(key, out.len());
                out.push(t);
            }
        }
    }
    out.sort_by_key(|t| std::cmp::Reverse(t.chars));
    out.truncate(MAX_TEXTS);
    out
}

/// Most counted first; equal counts in the page's order.
fn by_count<K>(mut list: Vec<(K, u32)>) -> Vec<(K, u32)> {
    list.sort_by_key(|e| std::cmp::Reverse(e.1));
    list
}

/// Spacing values grouped to [`SPACING_STEP`], most used first.
pub fn spacing(page: Vec<(f64, u32)>) -> Vec<(f64, u32)> {
    let mut out: Vec<(f64, u32)> = Vec::new();
    for (v, n) in page {
        let v = computed::js_round(v / SPACING_STEP) * SPACING_STEP + 0.0;
        match out.iter_mut().find(|(w, _)| *w == v) {
            Some(entry) => entry.1 += n,
            None => out.push((v, n)),
        }
    }
    by_count(out)
}

/// The index of the most used, the first of equals.
fn top(counts: &[u32]) -> Option<usize> {
    counts
        .iter()
        .enumerate()
        .fold(None::<(usize, u32)>, |best, (i, &n)| match best {
            Some((_, b)) if n <= b => best,
            _ => Some((i, n)),
        })
        .map(|(i, _)| i)
}
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
    /// Text styles as computed, in the order first seen.
    pub texts: Vec<PageText>,
    /// Spacing values (margins, paddings, gaps) as computed, and how often each was seen.
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
    /// How many card-like elements each card look has, in the order first seen.
    pub cards: Vec<u32>,
    /// The same for links in the page's main part.
    pub links: Vec<u32>,
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
    /// The most used card and link looks, by index.
    pub card: Option<usize>,
    pub link: Option<usize>,
}

/// Makes the scan of what the page read: drops transparent colours and ranks the rest, groups
/// and ranks text styles and spacing, ranks radii and shadows, caps the lists, sets the body's
/// background and the focus ring, and picks what to sample: the primary button (the most used
/// chromatic look), the secondary one (the most used other look), the card and the link.
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
    let top_look = |ok: &dyn Fn(&Look) -> bool| {
        looks
            .iter()
            .filter(|l| ok(l))
            .fold(None::<&Look>, |best, l| match best {
                Some(b) if l.2 <= b.2 => Some(b),
                _ => Some(l),
            })
    };
    let primary = top_look(&|l| l.3);
    let secondary = primary.and_then(|p| top_look(&|l| l.1 != p.1));
    let mut variables = page.variables;
    variables.truncate(MAX_VARIABLES);
    let mut breakpoints = page.breakpoints;
    breakpoints.sort_unstable();
    ScanPlan {
        button: primary.map(|l| l.4),
        secondary: secondary.map(|l| l.4),
        card: top(&page.cards),
        link: top(&page.links),
        scan: Scan {
            url: page.url,
            title: page.title,
            viewport: page.viewport,
            truncated: page.truncated,
            colors,
            texts: texts(page.texts),
            spacing: spacing(page.spacing),
            radii: by_count(page.radii),
            shadows: by_count(page.shadows),
            body_background,
            focus_ring,
            font_faces: page.font_faces,
            variables,
            breakpoints,
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
    /// The page op that samples the components: the buttons, card and link picked here, and
    /// the input and nav the page picked.
    pub fn samples_op(&self) -> Value {
        with_limits(json!({
            "op": "scan_samples",
            "button": self.button,
            "secondary": self.secondary,
            "card": self.card,
            "link": self.link,
        }))
    }

    /// The scan with its samples, within [`MAX_ANSWER`].
    pub fn finish(self, samples: PageSamples) -> Scan {
        let mut scan = self.scan;
        scan.components = samples.components;
        fit(&mut scan, MAX_ANSWER);
        scan
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
        let mut variables = page.variables;
        variables.truncate(MAX_VARIABLES);
        let mut breakpoints = page.breakpoints;
        breakpoints.sort_unstable();
        SheetRules {
            focus_ring: page.focus_rings.ring(&page.color_fallbacks),
            font_faces: page.font_faces,
            variables,
            breakpoints,
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
