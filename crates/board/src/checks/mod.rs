//! The machine checks of Sawit's design standard (`skills/sawit-canvas-design/SKILL.md`): the
//! rules a program can check in what an agent built, run after each write of a UI screen and when
//! a work is done, so the agent fixes what fails before it says it is done. Each failure names its
//! rule by the id SKILL.md gives it, the element or layer, and the fix ([`Finding`]).
//!
//! A screen's HTML and CSS are read as they are written ([`html`]): the styles cascade as a
//! browser's do for the selectors pages use, without laying the page out, so sizes are the ones
//! the CSS gives or the text and padding make. An artboard's layers are read from the document
//! ([`layers`]). Both check text contrast (col.10, with the formula of the skill's
//! `scripts/contrast.py`, [`contrast`]), touch targets (a11y.2), text sizes (typ.11), the spacing
//! scale (spc.1) and text that overflows its box (lay.18); a screen also images without alt text
//! (a11y.7), fields without labels and focus styles (a11y.8, sta.8).

mod color;
mod css;
pub mod html;
pub mod layers;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use color::{Rgba, parse_color};

/// The plugin data key a frame's last check is kept under, for the canvas's badge.
pub const CHECKS_KEY: &str = "sawit.checks";

/// One failure of a rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// The rule's id in SKILL.md: `col.10`.
    pub rule: String,
    /// What fails: a screen's element as a selector (`button.save`), an artboard's layer by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// The layer's id, for an artboard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What was measured and what to do: one or two plain sentences.
    pub fix: String,
}

impl Finding {
    /// A failure of `rule` by a screen's element `selector`.
    pub fn at_selector(rule: &str, selector: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            selector: Some(selector.into()),
            layer: None,
            id: None,
            fix: fix.into(),
        }
    }

    /// A failure of `rule` by an artboard's layer `name` (`id`).
    pub fn at_layer(rule: &str, name: &str, id: String, fix: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            selector: None,
            layer: Some(name.into()),
            id: Some(id),
            fix: fix.into(),
        }
    }

    /// Where it is, as the badge's list shows it.
    pub fn place(&self) -> &str {
        self.selector
            .as_deref()
            .or(self.layer.as_deref())
            .unwrap_or_default()
    }
}

/// `findings` without repeats: one per rule and place, in order.
pub fn dedup(findings: Vec<Finding>) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for finding in findings {
        let seen = out
            .iter()
            .any(|f| f.rule == finding.rule && f.place() == finding.place());
        if !seen {
            out.push(finding);
        }
    }
    out
}

/// The most findings a check answers: past it, the rest are counted, not listed.
pub const MAX_FINDINGS: usize = 40;

/// WCAG 2 contrast ratio of `fg` over the opaque `bg`, unrounded: `fg`'s alpha is composited over
/// `bg` first, as `scripts/contrast.py` does.
pub fn contrast(fg: Rgba, bg: Rgba) -> f64 {
    let mixed = fg.over(bg);
    let (a, b) = (luminance(mixed), luminance(bg));
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// The ratio text of `size` px at `weight` needs: 3:1 when large (24px+, or 18.67px+ bold),
/// else 4.5:1 (col.10).
pub fn needed_contrast(size: f64, weight: u32) -> f64 {
    let large = size >= 24.0 || (size >= 18.66 && weight >= 700);
    if large { 3.0 } else { 4.5 }
}

/// WCAG relative luminance.
fn luminance(c: Rgba) -> f64 {
    let channel = |v: f64| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// The spacing scale's steps (spc.1): multiples of 4, and 0 to 2 for hairlines and nudges.
pub fn on_scale(px: f64) -> bool {
    let px = px.abs();
    if px <= 2.0 + 1e-6 {
        return true;
    }
    let steps = px / 4.0;
    (steps - steps.round()).abs() < 1e-3
}

/// The text size floor (typ.11): functional text never below 11px.
pub const MIN_TEXT: f64 = 11.0;
/// The input text floor on touch frames (typ.11: iOS zooms smaller inputs), and elsewhere.
pub const MIN_INPUT_TOUCH: f64 = 16.0;
pub const MIN_INPUT: f64 = 14.0;
/// Target floors (a11y.2): 24×24 everywhere, 44×44 on touch frames.
pub const MIN_TARGET: f64 = 24.0;
pub const MIN_TARGET_TOUCH: f64 = 44.0;
/// Frames narrower than this are touch frames (phones and tablets).
pub const TOUCH_WIDTH: f64 = 1024.0;

/// The fix for a contrast failure of `fg` on `bg`.
fn contrast_fix(fg: Rgba, bg: Rgba, ratio: f64, needed: f64) -> String {
    format!(
        "Text {} on {} is {:.2}:1; it needs {needed}:1. Move the text's lightness away from the \
         background's (keep the hue) and measure again.",
        fg.hex(),
        bg.hex(),
        (ratio * 100.0).floor() / 100.0
    )
}

/// The fix for text of `size` px under `floor`.
fn size_fix(size: f64, floor: f64) -> String {
    format!(
        "Text is {}px; the floor is {floor}px. Use {floor}px or more.",
        round(size)
    )
}

/// The fix for a target of `w` × `h` under `floor`.
fn target_fix(w: f64, h: f64, floor: f64) -> String {
    format!(
        "The target is {} × {}px; it needs {floor} × {floor}px here. Grow its padding or \
         min-height (a small icon can keep its size inside a larger hit area).",
        round(w),
        round(h)
    )
}

/// The fix for a gap, padding or margin of `px` off the scale.
fn spacing_fix(what: &str, px: f64) -> String {
    let px = px.abs();
    let down = (px / 4.0).floor() * 4.0;
    let up = down + 4.0;
    format!(
        "{what} is {}px, off the 4px scale. Use {} or {}px.",
        round(px),
        round(down.max(4.0)),
        round(up)
    )
}

/// `v` with at most one decimal.
fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
