//! Convert to Layers (B15): an HTML screen becomes a frame of editable layers. The screen's page,
//! as its web view renders it, is read by [`CAPTURE`] (boxes, computed styles, text, pictures);
//! [`plan`] turns that into a tree of [`Layer`]s: frames for the boxes that draw something or
//! group several things, text, images and SVG pictures as editable vectors, and auto layout where
//! the page's flex rows and columns, or its block flow, place children at even gaps
//! ([`auto_layout`]); and [`build`] makes them on the canvas, beside the screen, in one undo step,
//! with the page's colours and type sizes as variables. The screen and its folder stay.
//!
//! What a layer can't carry is left out and counted in the answer: CSS backgrounds that are
//! images or gradients, `::before` / `::after` content, pictures from elsewhere.

mod build;
mod looks;
mod plan;
#[cfg(test)]
mod tests;

use serde::Deserialize;

pub use build::{BuildReport, build};
pub use plan::{AutoLayout, Kind, Layer, Sizing, plan};

/// The script a screen's web view runs to read its page; it answers a [`Capture`] as JSON text.
pub const CAPTURE: &str = include_str!("capture.js");

/// What [`CAPTURE`] answered: the page's viewport width, its height, its background, its title,
/// its body as a tree of [`Item`]s, and what it couldn't read.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Capture {
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub root: Option<Item>,
    #[serde(default)]
    pub skipped: Skipped,
}

impl Capture {
    /// The capture in what the web view answered (WebKit hands the JSON string back as JSON).
    pub fn parse(json: &str) -> Result<Self, String> {
        let text = serde_json::from_str::<String>(json).unwrap_or_else(|_| json.to_string());
        serde_json::from_str(&text).map_err(|e| format!("the page couldn't be read: {e}"))
    }
}

/// What the page has that layers don't carry.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Skipped {
    /// `::before` / `::after` content.
    #[serde(default)]
    pub pseudo: u32,
    /// CSS background images and gradients.
    #[serde(default)]
    pub backgrounds: u32,
    /// Pictures that couldn't be read (from elsewhere, or not loaded).
    #[serde(default)]
    pub images: u32,
    /// Elements past the most the script reads.
    #[serde(default)]
    pub over: u32,
}

/// One element of the page (or one run of its text, `t: "#text"`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Item {
    /// The tag, lowercase, or `#text`.
    pub t: String,
    /// Its box: x, y, width, height in CSS px from the page's top-left.
    pub r: [f64; 4],
    #[serde(default)]
    pub s: Style,
    #[serde(default)]
    pub k: Vec<Item>,
    /// A text run's text.
    #[serde(default)]
    pub tx: Option<String>,
    /// A picture as a `data:` URL.
    #[serde(default)]
    pub img: Option<String>,
    /// An SVG's markup, paints resolved.
    #[serde(default)]
    pub svg: Option<String>,
    /// A name: its label, alt text, id or first class.
    #[serde(default)]
    pub nm: Option<String>,
}

/// The computed styles a layer can carry (CSS px, CSS colour strings).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Style {
    /// Text colour, family, size, weight, line height (px, `None` for normal), letter spacing,
    /// alignment, transform, decoration and italic.
    #[serde(default)]
    pub c: Option<String>,
    #[serde(default)]
    pub ff: Option<String>,
    #[serde(default)]
    pub fs: f64,
    #[serde(default)]
    pub fw: f64,
    #[serde(default)]
    pub lh: Option<f64>,
    #[serde(default)]
    pub ls: f64,
    #[serde(default)]
    pub ta: Option<String>,
    #[serde(default)]
    pub tt: Option<String>,
    #[serde(default)]
    pub td: Option<String>,
    #[serde(default)]
    pub it: bool,
    /// Background colour, border widths (top, right, bottom, left) and colour, corner radii
    /// (top-left, top-right, bottom-right, bottom-left), opacity, box shadow.
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub bd: Option<[f64; 4]>,
    #[serde(default)]
    pub bc: Option<String>,
    #[serde(default)]
    pub rd: Option<[f64; 4]>,
    #[serde(default)]
    pub op: Option<f64>,
    #[serde(default)]
    pub sh: Option<String>,
    /// Display, position, and a flex box's direction, wrap, justify and align; gaps (row,
    /// column), padding and border widths (top, right, bottom, left); a flex item's grow and
    /// align-self; whether it clips.
    #[serde(default)]
    pub d: Option<String>,
    #[serde(default)]
    pub pos: Option<String>,
    #[serde(default)]
    pub fd: Option<String>,
    #[serde(default)]
    pub wr: bool,
    #[serde(default)]
    pub jc: Option<String>,
    #[serde(default)]
    pub ai: Option<String>,
    #[serde(default)]
    pub g: Option<[f64; 2]>,
    #[serde(default)]
    pub p: Option<[f64; 4]>,
    #[serde(default)]
    pub bw: Option<[f64; 4]>,
    #[serde(default)]
    pub grow: f64,
    #[serde(default, rename = "self")]
    pub align_self: Option<String>,
    #[serde(default)]
    pub clip: bool,
}

impl Style {
    /// Whether the element draws something of its own: a fill, a border or a shadow.
    pub fn draws(&self) -> bool {
        self.bg.is_some() || self.bd.is_some() || self.sh.is_some()
    }

    /// Padding plus border, per side.
    pub fn inset(&self) -> [f64; 4] {
        let p = self.p.unwrap_or_default();
        let b = self.bw.unwrap_or_default();
        [p[0] + b[0], p[1] + b[1], p[2] + b[2], p[3] + b[3]]
    }

    /// Whether it is placed out of the flow (absolute or fixed).
    pub fn out_of_flow(&self) -> bool {
        matches!(self.pos.as_deref(), Some("absolute" | "fixed"))
    }
}
