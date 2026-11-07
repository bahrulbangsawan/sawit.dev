//! The design lint (B12): what an agent drew on the canvas, checked against Sawit's design
//! standard (`skills/sawit-canvas-design/SKILL.md`) in Rust: values off the file's tokens, copies
//! that should be components, frames placed by hand where auto layout belongs, uneven spacing, too
//! many type sizes or colours, placeholder copy, the usual signs of generic generated design
//! ("slop"), and the machine checks of [`super::layers`] (contrast, targets, text floors). Each
//! finding names its layer, why, and a fix, with a command when one does it. The rules and their
//! thresholds are one table, [`RULES`], each with the guide section it comes from.
//!
//! [`design_status`] says where the file is in the workflow the tools lead to: tokens, then
//! components, then screens built from instances, then a cover.

use std::collections::{BTreeMap, BTreeSet};

use figcraft_color::PaintKind;
use figcraft_doc::{Document, EffectKind, Node, NodeId, NodeKind, TextAlignH, VariableValue};
use serde::Serialize;
use serde_json::{Value, json};

use super::{Finding, Rgba};

mod rules;
mod signs;
mod status;

pub use rules::{
    GLOW_RADIUS, GLOWS, LAYOUT_CHILDREN, MAX_COLORS, MAX_TYPE_SIZES, REPEATS, RULES, Rule, rule,
};
use signs::{is_emoji, purple_to_blue};
pub use status::design_status;

/// How much a finding matters: an error fails `canvas_lint`; a warning is advice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// Copy that is a placeholder, not the product's words.
const PLACEHOLDERS: &[&str] = &[
    "lorem ipsum",
    "dolor sit amet",
    "click here",
    "welcome to our platform",
    "your text here",
    "insert text",
    "placeholder text",
    "sample text",
    "john doe",
    "jane doe",
];

/// A lint finding: the check's finding, how much it matters, and the command that fixes it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LintFinding {
    #[serde(flatten)]
    pub finding: Finding,
    pub severity: Severity,
    /// The section of the design standard the rule comes from.
    pub guide: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Value>,
}

/// The lint of a page or a frame.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Lint {
    pub errors: usize,
    pub warnings: usize,
    pub findings: Vec<LintFinding>,
}

impl Lint {
    /// A short summary, for the reply of every write: the counts and the first few findings.
    pub fn summary(&self, first: usize) -> Value {
        json!({
            "errors": self.errors,
            "warnings": self.warnings,
            "first": self.findings.iter().take(first).map(|f| json!({
                "rule": f.finding.rule, "layer": f.finding.place(), "id": f.finding.id, "fix": f.finding.fix,
            })).collect::<Vec<_>>(),
        })
    }
}

/// The frames a lint of `scope` looks at: `scope` itself when it is a frame, else the top frames
/// of the page `scope` is (or is on).
fn screens(doc: &Document, scope: NodeId) -> Vec<NodeId> {
    match doc.node(scope) {
        Some(node) if node.is_page() => doc.top_frames(scope),
        Some(_) => vec![scope],
        None => Vec::new(),
    }
}

/// Lints `scope`, a page or a frame of `doc`. `sketch` keeps the workflow's warnings quiet (a
/// wireframe or a diagram), leaving the errors.
pub fn lint(doc: &Document, scope: NodeId, sketch: bool) -> Lint {
    let mut out: Vec<(Finding, Option<Value>)> = Vec::new();
    let colors = color_tokens(doc);
    let frames = screens(doc, scope);
    let ui = frames.iter().any(|f| is_screen(doc, *f));
    if ui && colors.is_empty() && !sketch {
        let first = frames.first().copied().unwrap_or(scope);
        let name = doc.node(first).map(|n| n.name.clone()).unwrap_or_default();
        out.push((
            Finding::at_layer(
                "cnv.15",
                &name,
                first.to_string(),
                "This UI uses no design system: the file has no colour variables. Start with \
                 tokens (variable.create: colours with light and dark, type, spacing, radii), \
                 then components, then screens from instances.",
            ),
            None,
        ));
    }
    for frame in &frames {
        if let Some(node) = doc.node(*frame) {
            for f in super::layers::check(doc, *frame) {
                out.push((f, None));
            }
            screen_rules(doc, node, &colors, sketch, &mut out);
        }
    }
    let mut lint = Lint::default();
    let mut seen = BTreeSet::new();
    for (finding, command) in out {
        let Some(rule) = rule(&finding.rule) else {
            continue;
        };
        if sketch && rule.severity == Severity::Warning {
            continue;
        }
        if !seen.insert((finding.rule.clone(), finding.place().to_string())) {
            continue;
        }
        match rule.severity {
            Severity::Error => lint.errors += 1,
            Severity::Warning => lint.warnings += 1,
        }
        lint.findings.push(LintFinding {
            finding,
            severity: rule.severity,
            guide: rule.guide,
            command,
        });
    }
    // Errors first, the order found within each.
    lint.findings.sort_by_key(|f| f.severity != Severity::Error);
    lint
}

/// Whether frame `id` is a screen: an artboard at a device's size or larger.
fn is_screen(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .is_some_and(|n| matches!(n.kind, NodeKind::Frame) && n.width >= 320.0 && n.height >= 400.0)
}

/// The file's colour variables: name and colour (their first mode's value).
fn color_tokens(doc: &Document) -> Vec<(String, Rgba)> {
    doc.variables
        .variables
        .iter()
        .filter_map(|v| {
            let value = v.values.values().next()?;
            match value {
                VariableValue::Color(c) => Some((v.name.clone(), rgba(c))),
                _ => None,
            }
        })
        .collect()
}

fn rgba(c: &figcraft_color::Color) -> Rgba {
    Rgba {
        r: f64::from(c.r),
        g: f64::from(c.g),
        b: f64::from(c.b),
        a: f64::from(c.a),
    }
}

/// The token nearest to `c` among `tokens`, and how far (0 to about 1.7, in sRGB).
pub fn nearest_token(tokens: &[(String, Rgba)], c: Rgba) -> Option<(&str, f64)> {
    tokens
        .iter()
        .map(|(name, t)| {
            let d = ((t.r - c.r).powi(2) + (t.g - c.g).powi(2) + (t.b - c.b).powi(2)).sqrt();
            (name.as_str(), d)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The rules of one screen (a top frame) and everything in it.
fn screen_rules(
    doc: &Document,
    root: &Node,
    colors: &[(String, Rgba)],
    sketch: bool,
    out: &mut Vec<(Finding, Option<Value>)>,
) {
    let mut sizes = BTreeSet::new();
    let mut fills = BTreeSet::new();
    let mut glows = Vec::new();
    let (mut texts, mut centred) = (0usize, 0usize);
    let mut stack = vec![(root, 0usize)];
    while let Some((node, cards)) = stack.pop() {
        if !node.visible {
            continue;
        }
        let id = node.id.to_string();
        // Solid fills: off the tokens (col.16), and counted (col.4).
        for (i, paint) in node.fills.iter().enumerate().filter(|(_, p)| p.visible) {
            match &paint.kind {
                PaintKind::Solid { color } => {
                    let c = rgba(color);
                    fills.insert(c.hex());
                    let field = format!("fills/{i}/color");
                    if !colors.is_empty() && !node.bound.contains_key(&field) && !sketch {
                        let (token, d) = nearest_token(colors, c).unwrap_or(("", 0.0));
                        let fix = if d < 0.02 {
                            format!(
                                "{} is the token “{token}” as a raw value: bind it.",
                                c.hex()
                            )
                        } else {
                            format!("{} is not in the system; nearest token “{token}”.", c.hex())
                        };
                        let command = json!({"command": "variable.bind",
                            "params": {"ids": [node.id.0], "field": field, "variable": token}});
                        out.push((
                            Finding::at_layer("col.16", &node.name, id.clone(), fix),
                            Some(command),
                        ));
                    }
                }
                PaintKind::Gradient(g) if purple_to_blue(g) => {
                    out.push((
                        Finding::at_layer(
                            "col.6",
                            &node.name,
                            id.clone(),
                            "A purple-to-blue gradient is the stock generated look: use a solid \
                             surface from the tokens, or the brand's own colours.",
                        ),
                        None,
                    ));
                }
                _ => {}
            }
        }
        // Glows, frosted blurs and glass (pol.5).
        let glowing = node
            .effects
            .iter()
            .filter(|e| e.visible)
            .any(|e| match &e.kind {
                EffectKind::DropShadow(s) => s.radius >= GLOW_RADIUS,
                EffectKind::BackgroundBlur(_) | EffectKind::Glass(_) => true,
                _ => false,
            });
        if glowing {
            glows.push((node.name.clone(), id.clone()));
        }
        let card = is_card(node);
        if card && cards > 0 && node.id != root.id {
            out.push((
                Finding::at_layer(
                    "cmp.9",
                    &node.name,
                    id.clone(),
                    "A card inside a card: flatten the inner one (no fill, border or shadow) and \
                     group by spacing.",
                ),
                None,
            ));
        }
        if let NodeKind::Text(text) = &node.kind {
            let chars = text.characters.trim();
            if !chars.is_empty() {
                texts += 1;
                sizes.insert((text.style.font_size * 10.0).round() as i64);
                if text.align_h == TextAlignH::Center {
                    centred += 1;
                }
                let lower = chars.to_lowercase();
                if let Some(p) = PLACEHOLDERS.iter().find(|p| lower.contains(*p)) {
                    let fix = format!(
                        "“{p}” is placeholder copy: write the product's own words for what this \
                         says or does."
                    );
                    out.push((
                        Finding::at_layer("cpy.19", &node.name, id.clone(), fix),
                        None,
                    ));
                }
                if is_emoji(chars) {
                    out.push((
                        Finding::at_layer(
                            "cmp.1",
                            &node.name,
                            id.clone(),
                            "An emoji used as an icon: use an icon from the set (Lucide), sized \
                             to the text, with a name for screen readers.",
                        ),
                        None,
                    ));
                }
            }
        }
        if node.kind.is_frame_like() {
            frame_rules(doc, node, out);
        }
        for child in node.children.iter().rev() {
            if let Some(child) = doc.node(*child) {
                stack.push((child, cards + usize::from(card)));
            }
        }
    }
    let name = root.name.as_str();
    let id = root.id.to_string();
    if sizes.len() > MAX_TYPE_SIZES {
        let fix = format!(
            "{} type sizes on one screen; a scale has {MAX_TYPE_SIZES} at most. Map each text to \
             the nearest step of the type scale.",
            sizes.len()
        );
        out.push((Finding::at_layer("typ.4", name, id.clone(), fix), None));
    }
    if fills.len() > MAX_COLORS {
        let fix = format!(
            "{} colours on one screen; keep to {MAX_COLORS} or fewer from the tokens.",
            fills.len()
        );
        out.push((Finding::at_layer("col.4", name, id.clone(), fix), None));
    }
    if glows.len() >= GLOWS {
        let fix = format!(
            "{} layers glow, blur or are glass: keep effects for one thing that must stand out, \
             and use the tokens' shadows elsewhere.",
            glows.len()
        );
        out.push((
            Finding::at_layer("pol.5", &glows[0].0, glows[0].1.clone(), fix),
            None,
        ));
    }
    if texts >= 5 && centred * 10 >= texts * 9 {
        let fix = format!(
            "{centred} of {texts} texts are centred: left-align body text and lists; centre only \
             short headings or empty states."
        );
        out.push((Finding::at_layer("lay.13", name, id, fix), None));
    }
}

/// Whether `node` is a card: a frame with a fill and rounded corners, and a border or a shadow.
fn is_card(node: &Node) -> bool {
    let filled = node.fills.iter().any(|p| p.visible);
    let rounded = node.corners.radii.iter().any(|r| *r > 0.0);
    let edged = node.strokes.iter().any(|p| p.visible)
        || node
            .effects
            .iter()
            .any(|e| e.visible && matches!(e.kind, EffectKind::DropShadow(_)));
    matches!(node.kind, NodeKind::Frame | NodeKind::Instance { .. }) && filled && rounded && edged
}

/// The rules of one frame's own children: copies (cnv.13), hand placement (cnv.11), uneven gaps
/// (cnv.12).
fn frame_rules(doc: &Document, frame: &Node, out: &mut Vec<(Finding, Option<Value>)>) {
    let children: Vec<&Node> = (frame.children.iter())
        .filter_map(|c| doc.node(*c))
        .filter(|c| c.visible)
        .collect();
    // Copies: siblings alike in kind, size and contents that aren't instances of one component.
    let mut alike: BTreeMap<String, Vec<&Node>> = BTreeMap::new();
    for child in &children {
        if matches!(child.kind, NodeKind::Frame | NodeKind::Group) && !child.children.is_empty() {
            alike.entry(signature(doc, child)).or_default().push(child);
        }
    }
    for group in alike.values().filter(|g| g.len() >= REPEATS) {
        let first = group[0];
        let fix = format!(
            "{} copies of “{}”: make it a component and use instances, so a change reaches \
             every one.",
            group.len(),
            first.name
        );
        let command = json!({"command": "component.create", "params": {"ids": [first.id.0]}});
        out.push((
            Finding::at_layer("cnv.13", &first.name, first.id.to_string(), fix),
            Some(command),
        ));
    }
    // A UI frame of several children laid out by hand.
    let screenish = frame.width >= 120.0 && frame.height >= 40.0;
    if frame.layout.is_none()
        && screenish
        && children.len() >= LAYOUT_CHILDREN
        && !is_canvas_group(frame)
    {
        let fix = format!(
            "“{}” places its {} children by x and y: give it auto layout (layout.add), with gap \
             and padding from the spacing scale.",
            frame.name,
            children.len()
        );
        let command = json!({"command": "layout.add", "params": {"ids": [frame.id.0]}});
        out.push((
            Finding::at_layer("cnv.11", &frame.name, frame.id.to_string(), fix),
            Some(command),
        ));
        uneven_gaps(frame, &children, out);
    }
}

/// A frame used to hold a diagram or a board's group rather than UI: a section's or a group's
/// contents are let be.
fn is_canvas_group(frame: &Node) -> bool {
    matches!(frame.kind, NodeKind::Section { .. } | NodeKind::Group)
}

/// cnv.12: children stacked down a frame by hand with three or more different gaps.
fn uneven_gaps(frame: &Node, children: &[&Node], out: &mut Vec<(Finding, Option<Value>)>) {
    let mut rows: Vec<(f64, f64)> = children
        .iter()
        .map(|c| {
            let y = c.transform.translation().y;
            (y, y + c.height)
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let gaps: BTreeSet<i64> = rows
        .windows(2)
        .map(|w| (w[1].0 - w[0].1).round() as i64)
        .filter(|g| *g >= 0)
        .collect();
    if gaps.len() >= 3 {
        let list: Vec<String> = gaps.iter().map(|g| format!("{g}px")).collect();
        let fix = format!(
            "Gaps of {} between the layers of “{}”: use one gap from the spacing scale (auto \
             layout keeps it).",
            list.join(", "),
            frame.name
        );
        out.push((
            Finding::at_layer("cnv.12", &frame.name, frame.id.to_string(), fix),
            None,
        ));
    }
}

/// What makes two layers copies: their kind, size (to the px), and their children's kinds.
fn signature(doc: &Document, node: &Node) -> String {
    let kids: Vec<&str> = (node.children.iter())
        .filter_map(|c| doc.node(*c))
        .map(|c| kind_name(&c.kind))
        .collect();
    format!(
        "{}:{}x{}:{}",
        kind_name(&node.kind),
        node.width.round(),
        node.height.round(),
        kids.join(",")
    )
}

fn kind_name(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Frame => "frame",
        NodeKind::Group => "group",
        NodeKind::Text(_) => "text",
        NodeKind::Rectangle => "rect",
        NodeKind::Ellipse { .. } => "ellipse",
        NodeKind::Instance { .. } => "instance",
        _ => "other",
    }
}

#[cfg(test)]
mod tests;
