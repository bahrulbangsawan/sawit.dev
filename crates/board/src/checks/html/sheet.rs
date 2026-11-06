//! The checks a page's style sheets answer whole, rule by rule: the spacing scale (spc.1) and
//! focus rings (sta.8, a11y.8).

use super::super::css::Sheet;
use super::super::{Finding, on_scale, spacing_fix};
use super::style::length;

/// The properties whose lengths are on the spacing scale (spc.1).
const SPACING: &[&str] = &[
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-block",
    "margin-inline",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding-block",
    "padding-inline",
    "gap",
    "row-gap",
    "column-gap",
];

/// spc.1: every margin, padding and gap length the sheets set is on the scale.
pub(super) fn spacing(sheet: &Sheet, findings: &mut Vec<Finding>) {
    for rule in &sheet.rules {
        for (name, value, _) in &rule.decls {
            if !SPACING.contains(&name.as_str()) || value.contains("var(") {
                continue;
            }
            let off = value
                .split_whitespace()
                .filter_map(|p| length(p, 16.0))
                .find(|px| !on_scale(*px));
            if let Some(px) = off {
                findings.push(Finding::at_selector(
                    "spc.1",
                    &rule.selector,
                    spacing_fix(name, px),
                ));
            }
        }
    }
}

/// sta.8 and a11y.8: a page with controls draws a focus ring, and never takes the browser's away
/// without one of its own.
pub(super) fn focus(sheet: &Sheet, interactive: bool, findings: &mut Vec<Finding>) {
    if !interactive {
        return;
    }
    let ring = sheet.rules.iter().any(|r| {
        r.selector.contains(":focus")
            && r.decls.iter().any(|(n, v, _)| {
                (n.starts_with("outline") && !matches!(v.trim(), "none" | "0")) || n == "box-shadow"
            })
    });
    let visible = sheet.text.contains(":focus-visible");
    for rule in &sheet.rules {
        let removes = rule.decls.iter().any(|(n, v, _)| {
            matches!(n.as_str(), "outline" | "outline-style" | "outline-width")
                && matches!(v.trim(), "none" | "0" | "0px")
        });
        if removes && !ring {
            let fix = "outline: none takes the focus ring away and nothing draws one: add a \
                       :focus-visible ring, 2px solid with a 2px offset, at 3:1 against what it \
                       crosses.";
            findings.push(Finding::at_selector("sta.8", &rule.selector, fix));
        }
    }
    if !visible {
        let fix = "Nothing styles :focus-visible: give every control a visible ring, 2px solid \
                   with a 2px offset, at 3:1 against what it crosses.";
        findings.push(Finding::at_selector("a11y.8", ":focus-visible", fix));
    }
}
