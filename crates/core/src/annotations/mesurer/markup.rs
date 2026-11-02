//! A page's markup in mesurer besides its comments, for the notes an agent gets: text notes,
//! arrows and pen strokes with the elements under them, pinned distances and measurements, guides,
//! layout guides, X-ray, and the captures saved this session. The page's agent names the elements
//! (`elements_at`), at the [`MarkPoint`]s; [`PageMarkup::markdown`] writes the section.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::Rect;
use super::super::markdown::{code, one_line};

/// At most this many marks of a kind are listed; the rest are counted.
const LISTED: usize = 30;

/// The element under a mark, as the page's agent names it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Near {
    pub selector: String,
    #[serde(default)]
    pub label: String,
}

/// A point of the page (CSS px from its top left) whose element a mark names, and the key its
/// answer is kept under: the mark and where it is, so a mark moved since is asked about again.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkPoint {
    pub key: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMark {
    pub id: String,
    pub text: String,
    /// Its top left on the page.
    pub at: (f64, f64),
    pub near: Option<Near>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrowMark {
    pub id: String,
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub curved: bool,
    pub from_near: Option<Near>,
    pub to_near: Option<Near>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeMark {
    pub id: String,
    /// `ink` or `marker`.
    pub pen: String,
    /// The box its points cover, on the page.
    pub bounds: Rect,
    pub near: Option<Near>,
}

/// A distance pinned with ⌥: the gaps it shows, CSS px.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistanceMark {
    pub horizontal: Option<f64>,
    pub vertical: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuideMark {
    pub vertical: bool,
    /// From the viewport's left (vertical) or top (horizontal), CSS px.
    pub position: f64,
}

/// A layout guide (`LayoutGuide`): columns, rows or a pixel grid.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutMark {
    pub kind: String,
    pub count: u32,
    pub size: f64,
    pub gutter: f64,
    pub offset: f64,
    pub align: String,
    pub visible: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageMarkup {
    pub texts: Vec<TextMark>,
    pub arrows: Vec<ArrowMark>,
    pub strokes: Vec<StrokeMark>,
    pub distances: Vec<DistanceMark>,
    /// Measured boxes, on the page.
    pub measurements: Vec<Rect>,
    pub guides: Vec<GuideMark>,
    pub layout_guides: Vec<LayoutMark>,
    pub xray: bool,
    /// Screenshots and recordings mesurer saved this session.
    pub captures: Vec<PathBuf>,
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn point(v: &Value) -> Option<(f64, f64)> {
    Some((number(&v["x"])?, number(&v["y"])?))
}

fn rect(v: &Value) -> Option<Rect> {
    Some(Rect {
        x: number(&v["left"])?,
        y: number(&v["top"])?,
        width: number(&v["width"])?,
        height: number(&v["height"])?,
    })
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap_or_default().to_string()
}

impl PageMarkup {
    /// The markup of a page's artifacts (`MesurerPageArtifacts` JSON, `null` for none), with
    /// X-ray as the workspace has it. No element is named yet ([`Self::place`]).
    pub fn of(page: &Value, xray: bool) -> PageMarkup {
        let texts = list(&page["textAnnotations"])
            .iter()
            .filter_map(|t| {
                Some(TextMark {
                    id: id(t),
                    text: t["text"].as_str()?.to_string(),
                    at: point(t)?,
                    near: None,
                })
            })
            .collect();
        let arrows = list(&page["arrows"])
            .iter()
            .filter_map(|a| {
                let (from, to) = (point(&a["start"])?, point(&a["end"])?);
                Some(ArrowMark {
                    id: id(a),
                    from,
                    to,
                    curved: point(&a["control"]).is_some_and(|c| off_line(from, to, c) > 2.),
                    from_near: None,
                    to_near: None,
                })
            })
            .collect();
        let strokes = list(&page["penStrokes"])
            .iter()
            .filter_map(|s| {
                let points: Vec<(f64, f64)> = list(&s["points"]).iter().filter_map(point).collect();
                let (x0, y0) = points
                    .iter()
                    .fold((f64::MAX, f64::MAX), |(x, y), p| (x.min(p.0), y.min(p.1)));
                let (x1, y1) = points
                    .iter()
                    .fold((f64::MIN, f64::MIN), |(x, y), p| (x.max(p.0), y.max(p.1)));
                (!points.is_empty()).then(|| StrokeMark {
                    id: id(s),
                    pen: s["kind"].as_str().unwrap_or("ink").to_string(),
                    bounds: Rect {
                        x: x0,
                        y: y0,
                        width: x1 - x0,
                        height: y1 - y0,
                    },
                    near: None,
                })
            })
            .collect();
        let distances = list(&page["heldDistances"])
            .iter()
            .map(|d| DistanceMark {
                horizontal: number(&d["horizontal"]["value"]),
                vertical: number(&d["vertical"]["value"]),
            })
            .filter(|d| d.horizontal.is_some() || d.vertical.is_some())
            .collect();
        let measurements = list(&page["measurements"])
            .iter()
            .filter_map(|m| rect(&m["rect"]))
            .collect();
        let guides = list(&page["guides"])
            .iter()
            .filter_map(|g| {
                Some(GuideMark {
                    vertical: g["orientation"] != "horizontal",
                    position: number(&g["position"])?,
                })
            })
            .collect();
        let layout_guides = list(&page["layoutGuides"])
            .iter()
            .map(|l| LayoutMark {
                kind: l["kind"].as_str().unwrap_or("columns").to_string(),
                count: number(&l["count"]).map_or(1, |c| c.round().max(1.) as u32),
                size: number(&l["size"]).unwrap_or_default(),
                gutter: number(&l["gutter"]).unwrap_or_default(),
                offset: number(&l["offset"]).unwrap_or_default(),
                align: l["align"].as_str().unwrap_or("stretch").to_string(),
                visible: l["visible"] != false,
            })
            .collect();
        PageMarkup {
            texts,
            arrows,
            strokes,
            distances,
            measurements,
            guides,
            layout_guides,
            xray,
            captures: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
            && self.arrows.is_empty()
            && self.strokes.is_empty()
            && self.distances.is_empty()
            && self.measurements.is_empty()
            && self.guides.is_empty()
            && self.layout_guides.is_empty()
            && !self.xray
            && self.captures.is_empty()
    }

    /// The points whose elements the marks name: each text note's top left, each arrow's two ends,
    /// each stroke's middle.
    pub fn points(&self) -> Vec<MarkPoint> {
        let at = |kind: &str, id: &str, (x, y): (f64, f64)| MarkPoint {
            key: format!("{kind}:{id}@{},{}", x.round(), y.round()),
            x,
            y,
        };
        let mut points = Vec::new();
        for t in &self.texts {
            points.push(at("text", &t.id, t.at));
        }
        for a in &self.arrows {
            points.push(at("from", &a.id, a.from));
            points.push(at("to", &a.id, a.to));
        }
        for s in &self.strokes {
            let b = &s.bounds;
            points.push(at(
                "stroke",
                &s.id,
                (b.x + b.width / 2., b.y + b.height / 2.),
            ));
        }
        points
    }

    /// Names the elements under the marks, from what the page's agent answered for the points'
    /// keys ([`Self::points`]); a point it didn't answer for stays unnamed.
    pub fn place(&mut self, found: &HashMap<String, Option<Near>>) {
        let points = self.points();
        let mut answers = points.iter().map(|p| found.get(&p.key).cloned().flatten());
        for t in &mut self.texts {
            t.near = answers.next().flatten();
        }
        for a in &mut self.arrows {
            a.from_near = answers.next().flatten();
            a.to_near = answers.next().flatten();
        }
        for s in &mut self.strokes {
            s.near = answers.next().flatten();
        }
    }

    /// The `## Page markup (Mesurer)` section. What the page or the user wrote (text notes,
    /// selectors) is in code spans; positions are CSS px, on the page except the guides'.
    pub fn markdown(&self) -> String {
        let mut out = String::from("## Page markup (Mesurer)\n");
        out.push_str(
            "Positions are CSS px from the page's top left; guides, from the viewport's.\n",
        );
        let xy = |(x, y): (f64, f64)| format!("({}, {})", x.round(), y.round());
        let on = |near: &Option<Near>| {
            near.as_ref()
                .map(|n| format!(" on {}", code(&n.selector)))
                .unwrap_or_default()
        };
        section(&mut out, "Text notes", &self.texts, |t| {
            format!(
                "{} at {}{}",
                code(&one_line(&t.text)),
                xy(t.at),
                on(&t.near)
            )
        });
        section(&mut out, "Arrows", &self.arrows, |a| {
            format!(
                "from {}{} to {}{}{}",
                xy(a.from),
                on(&a.from_near),
                xy(a.to),
                on(&a.to_near),
                if a.curved { " (curved)" } else { "" }
            )
        });
        section(&mut out, "Pen strokes", &self.strokes, |s| {
            let around = s
                .near
                .as_ref()
                .map(|n| format!(", around {}", code(&n.selector)))
                .unwrap_or_default();
            format!("{}, {}{around}", one_line(&s.pen), size_at(&s.bounds))
        });
        section(&mut out, "Pinned distances", &self.distances, |d| {
            let mut parts = Vec::new();
            if let Some(h) = d.horizontal {
                parts.push(format!("{} px across", h.round()));
            }
            if let Some(v) = d.vertical {
                parts.push(format!("{} px down", v.round()));
            }
            parts.join(", ")
        });
        section(&mut out, "Measurements", &self.measurements, size_at);
        if !self.guides.is_empty() {
            let guides: Vec<String> = (self.guides.iter().take(LISTED))
                .map(|g| match g.vertical {
                    true => format!("vertical at x {}", g.position.round()),
                    false => format!("horizontal at y {}", g.position.round()),
                })
                .collect();
            let more = more(self.guides.len());
            let n = self.guides.len();
            out.push_str(&format!(
                "- **Guides ({n}):** {}{more}\n",
                guides.join(", ")
            ));
        }
        section(&mut out, "Layout guides", &self.layout_guides, |l| {
            let mut line = match l.kind.as_str() {
                "grid" => format!("grid, {} px squares", l.size.round()),
                kind => format!("{} {}", l.count, one_line(kind)),
            };
            if l.kind != "grid" {
                if l.align != "stretch" {
                    line.push_str(&format!(", {} px wide", l.size.round()));
                }
                line.push_str(&format!(
                    ", {} px gutter, {} px offset, align {}",
                    l.gutter.round(),
                    l.offset.round(),
                    one_line(&l.align)
                ));
            }
            if !l.visible {
                line.push_str(" (hidden)");
            }
            line
        });
        if self.xray {
            out.push_str("- **X-ray:** on (every element's box outlined)\n");
        }
        section(&mut out, "Captures", &self.captures, |p| {
            p.display().to_string()
        });
        out
    }
}

/// How far `p` is from the line through `a` and `b`, px: mesurer keeps a control point for every
/// arrow, a straight one's on its line.
fn off_line(a: (f64, f64), b: (f64, f64), p: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.hypot(dy);
    if length == 0. {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / length
}

/// `120×40 at (10, 20)`.
fn size_at(r: &Rect) -> String {
    format!(
        "{}×{} at ({}, {})",
        r.width.round(),
        r.height.round(),
        r.x.round(),
        r.y.round()
    )
}

/// `, and 4 more` past [`LISTED`].
fn more(n: usize) -> String {
    match n.checked_sub(LISTED) {
        Some(rest) if rest > 0 => format!(", and {rest} more"),
        _ => String::new(),
    }
}

/// `- **Title (n):**` and a line per item, at most [`LISTED`] of them; nothing for none.
fn section<T>(out: &mut String, title: &str, items: &[T], line: impl Fn(&T) -> String) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("- **{title} ({}):**\n", items.len()));
    for item in items.iter().take(LISTED) {
        out.push_str(&format!("  - {}\n", line(item)));
    }
    if items.len() > LISTED {
        out.push_str(&format!("  - and {} more\n", items.len() - LISTED));
    }
}
