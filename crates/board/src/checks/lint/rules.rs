//! The design lint's rules and thresholds, in one table, each with the section of the design
//! standard it comes from.

use super::Severity;

/// One lint rule: its id, what it checks, how much it matters, and the section of the design
/// standard it comes from.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub id: &'static str,
    pub checks: &'static str,
    pub severity: Severity,
    pub guide: &'static str,
}

/// Most type sizes one screen should use (typ: a type scale of five or six steps).
pub const MAX_TYPE_SIZES: usize = 6;
/// Most solid colours one screen should use, text and fills together.
pub const MAX_COLORS: usize = 10;
/// Near-identical siblings from which a copy should be a component.
pub const REPEATS: usize = 3;
/// A frame with this many children or more is laid out with auto layout, not by hand.
pub const LAYOUT_CHILDREN: usize = 3;
/// Layers with a big glow, a frosted blur or glass, from which it is "everywhere".
pub const GLOWS: usize = 3;
/// A shadow this soft or softer is a glow.
pub const GLOW_RADIUS: f64 = 30.0;

/// Every rule, with the standard's section it comes from.
pub const RULES: &[Rule] = &[
    Rule {
        id: "col.10",
        checks: "text contrast",
        severity: Severity::Error,
        guide: "Color",
    },
    Rule {
        id: "a11y.2",
        checks: "target sizes",
        severity: Severity::Error,
        guide: "Accessibility",
    },
    Rule {
        id: "typ.11",
        checks: "text size floor",
        severity: Severity::Error,
        guide: "Typography",
    },
    Rule {
        id: "spc.1",
        checks: "the 4 px spacing scale",
        severity: Severity::Warning,
        guide: "Spacing",
    },
    Rule {
        id: "lay.18",
        checks: "text that overflows its frame",
        severity: Severity::Error,
        guide: "Layout",
    },
    Rule {
        id: "cpy.19",
        checks: "placeholder copy",
        severity: Severity::Error,
        guide: "Copywriting",
    },
    Rule {
        id: "col.16",
        checks: "colours off the file's tokens",
        severity: Severity::Warning,
        guide: "Color",
    },
    Rule {
        id: "cnv.15",
        checks: "UI without a design system",
        severity: Severity::Warning,
        guide: "Canvas craft",
    },
    Rule {
        id: "cnv.13",
        checks: "copies that should be one component",
        severity: Severity::Warning,
        guide: "Canvas craft",
    },
    Rule {
        id: "cnv.11",
        checks: "frames placed by hand where auto layout belongs",
        severity: Severity::Warning,
        guide: "Canvas craft",
    },
    Rule {
        id: "cnv.12",
        checks: "uneven spacing between siblings",
        severity: Severity::Warning,
        guide: "Canvas craft",
    },
    Rule {
        id: "typ.4",
        checks: "too many type sizes on a screen",
        severity: Severity::Warning,
        guide: "Typography",
    },
    Rule {
        id: "col.4",
        checks: "too many colours on a screen",
        severity: Severity::Warning,
        guide: "Color",
    },
    Rule {
        id: "col.6",
        checks: "a purple-to-blue gradient",
        severity: Severity::Warning,
        guide: "Color",
    },
    Rule {
        id: "cmp.1",
        checks: "emoji used as icons",
        severity: Severity::Warning,
        guide: "Components",
    },
    Rule {
        id: "pol.5",
        checks: "glow, blur or glass everywhere",
        severity: Severity::Warning,
        guide: "Polish",
    },
    Rule {
        id: "cmp.9",
        checks: "a card inside a card",
        severity: Severity::Warning,
        guide: "Components",
    },
    Rule {
        id: "lay.13",
        checks: "everything centred",
        severity: Severity::Warning,
        guide: "Layout",
    },
];

/// The rule `id`.
pub fn rule(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.id == id)
}
