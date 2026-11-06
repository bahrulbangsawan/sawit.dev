//! An agent's pointer: Sawit's own set of pointers, drawn as vectors so they stay crisp at every
//! zoom and on every screen ([`Tool`]), the curved, eased path it travels on ([`travel`]), and the
//! small dip it makes when it clicks ([`press_scale`]).

use std::time::Duration;

use serde_json::Value;

/// What the agent's pointer shows it doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::board) enum Tool {
    /// Moving, selecting.
    #[default]
    Arrow,
    /// Dragging out a box: a frame, a shape.
    Crosshair,
    /// Typing.
    IBeam,
    /// Drawing a line, an arrow, a connector, a vector.
    Pen,
    /// ⌥-dragging a copy.
    Duplicate,
    /// Moving or resizing a layer by its handles.
    Hand,
}

impl Tool {
    /// The words the pointer's pill adds while the agent works ("Header agent · typing").
    pub fn verb(self) -> Option<&'static str> {
        match self {
            Tool::Arrow => None,
            Tool::Crosshair => Some("drawing"),
            Tool::IBeam => Some("typing"),
            Tool::Pen => Some("drawing a line"),
            Tool::Duplicate => Some("duplicating"),
            Tool::Hand => Some("moving"),
        }
    }

    /// The pointer's outline, from its hot spot (px), as closed polygons; lines (a crosshair's,
    /// an I-beam's) are thin polygons, so every pointer is filled and edged the same way.
    pub fn shape(self) -> Vec<Vec<(f32, f32)>> {
        match self {
            Tool::Arrow => vec![ARROW.to_vec()],
            Tool::Duplicate => {
                // The arrow with a small plus below and right of it.
                let plus = [
                    (8.5, 9.0),
                    (10.0, 9.0),
                    (10.0, 7.5),
                    (11.5, 7.5),
                    (11.5, 9.0),
                    (13.0, 9.0),
                    (13.0, 10.5),
                    (11.5, 10.5),
                    (11.5, 12.0),
                    (10.0, 12.0),
                    (10.0, 10.5),
                    (8.5, 10.5),
                ];
                vec![ARROW.to_vec(), plus.to_vec()]
            }
            Tool::Crosshair => vec![
                bar((-7.0, -0.75), (7.0, 0.75)),
                bar((-0.75, -7.0), (0.75, 7.0)),
            ],
            Tool::IBeam => vec![
                bar((-0.75, -6.5), (0.75, 6.5)),
                bar((-3.0, -7.0), (3.0, -5.6)),
                bar((-3.0, 5.6), (3.0, 7.0)),
            ],
            Tool::Pen => vec![vec![
                (0.0, 0.0),
                (2.2, 4.0),
                (7.5, 9.3),
                (9.3, 7.5),
                (4.0, 2.2),
            ]],
            Tool::Hand => vec![
                vec![
                    (0.0, 0.0),
                    (0.0, 9.6),
                    (2.4, 7.2),
                    (4.1, 11.0),
                    (5.5, 10.3),
                    (3.9, 6.6),
                    (7.0, 6.5),
                ],
                bar((6.0, 9.0), (12.0, 10.4)),
                bar((8.3, 6.7), (9.7, 12.7)),
            ],
        }
    }

    /// The tool an agent's op shows it using.
    pub fn for_op(op: &Value) -> Tool {
        let name = op.get("op").and_then(Value::as_str).unwrap_or_default();
        match name {
            "mermaid" => Tool::Pen,
            "update" => Tool::Hand,
            "add" => {
                let kinds: Vec<&str> = (op.get("elements").and_then(Value::as_array))
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.get("type").and_then(Value::as_str))
                    .collect();
                match kinds.first().copied() {
                    Some("text") => Tool::IBeam,
                    Some("arrow" | "line") => Tool::Pen,
                    Some("rectangle" | "ellipse" | "diamond" | "frame") => Tool::Crosshair,
                    _ => Tool::Arrow,
                }
            }
            "command" => {
                let id = op
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let id = if id == "command.batch" {
                    let steps = op["params"]["steps"].as_array();
                    let first = steps.and_then(|s| s.first());
                    first
                        .and_then(|s| s.get("command"))
                        .and_then(Value::as_str)
                        .unwrap_or(id)
                } else {
                    id
                };
                if id.starts_with("text.") {
                    Tool::IBeam
                } else if id.starts_with("frame.") || id.starts_with("shape.") {
                    Tool::Crosshair
                } else if id.starts_with("vector.")
                    || id.contains("line")
                    || id.contains("connector")
                {
                    Tool::Pen
                } else if id.contains("duplicate") {
                    Tool::Duplicate
                } else if id == "node.set" || id.starts_with("align.") || id.starts_with("layout.")
                {
                    Tool::Hand
                } else {
                    Tool::Arrow
                }
            }
            "screen_write" => Tool::IBeam,
            _ => Tool::Arrow,
        }
    }
}

/// The arrow: 7 × 11 px, which its 1.5 px white edge makes 10 × 14.
const ARROW: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.0, 9.6),
    (2.4, 7.2),
    (4.1, 11.0),
    (5.5, 10.3),
    (3.9, 6.6),
    (7.0, 6.5),
];

/// A thin bar from `a` to `b` (corners), as a polygon.
fn bar(a: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    vec![a, (b.0, a.1), b, (a.0, b.1)]
}

/// The shortest and the longest a pointer's trip takes, and how much it adds per page unit.
const SHORTEST: Duration = Duration::from_millis(120);
const LONGEST: Duration = Duration::from_millis(450);
const PER_UNIT: f64 = 0.45;
/// How far the path bows sideways, as a share of its length, and at most (page units).
const BOW: f64 = 0.12;
const MAX_BOW: f64 = 60.0;

/// How long a trip from `from` to `to` takes: longer the farther it goes, within 120–450 ms.
pub(in crate::board) fn trip_time(from: (f64, f64), to: (f64, f64)) -> Duration {
    let distance = (to.0 - from.0).hypot(to.1 - from.1);
    let ms = (SHORTEST.as_millis() as f64 + distance * PER_UNIT).min(LONGEST.as_millis() as f64);
    Duration::from_millis(ms as u64)
}

/// Where a pointer is `t` (0–1) into its trip from `from` to `to`: on a quadratic Bézier that
/// bows a little to one side (`side`, ±1), eased in and out.
pub(in crate::board) fn travel(from: (f64, f64), to: (f64, f64), t: f64, side: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let e = t * t * (3.0 - 2.0 * t);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    let bow = (length * BOW).min(MAX_BOW) * side;
    let (nx, ny) = if length > 0.0 {
        (-dy / length, dx / length)
    } else {
        (0.0, 0.0)
    };
    let control = (from.0 + dx / 2.0 + nx * bow, from.1 + dy / 2.0 + ny * bow);
    let u = 1.0 - e;
    (
        u * u * from.0 + 2.0 * u * e * control.0 + e * e * to.0,
        u * u * from.1 + 2.0 * u * e * control.1 + e * e * to.1,
    )
}

/// How long a click's dip takes.
pub(in crate::board) const PRESS: Duration = Duration::from_millis(160);

/// How big the pointer is `since` a click: dipping to 85% and back.
pub(in crate::board) fn press_scale(since: Duration) -> f32 {
    if since >= PRESS {
        return 1.0;
    }
    let t = since.as_secs_f32() / PRESS.as_secs_f32();
    1.0 - 0.15 * (std::f32::consts::PI * t).sin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trips_curve_ease_and_take_their_distances_time() {
        assert_eq!(trip_time((0.0, 0.0), (0.0, 0.0)), SHORTEST);
        assert_eq!(trip_time((0.0, 0.0), (5000.0, 0.0)), LONGEST);
        let mid = travel((0.0, 0.0), (100.0, 0.0), 0.5, 1.0);
        assert!(
            (mid.0 - 50.0).abs() < 1e-9 && mid.1.abs() > 1.0,
            "it bows: {mid:?}"
        );
        assert_eq!(travel((0.0, 0.0), (100.0, 0.0), 1.0, 1.0), (100.0, 0.0));
        assert_eq!(travel((0.0, 0.0), (100.0, 0.0), 0.0, -1.0), (0.0, 0.0));
        assert_eq!(press_scale(PRESS), 1.0);
        assert!(press_scale(PRESS / 2) < 0.9);
    }

    #[test]
    fn each_op_picks_its_tool() {
        let add = |kind: &str| json!({"op": "add", "elements": [{"type": kind}]});
        assert_eq!(Tool::for_op(&add("text")), Tool::IBeam);
        assert_eq!(Tool::for_op(&add("rectangle")), Tool::Crosshair);
        assert_eq!(Tool::for_op(&add("arrow")), Tool::Pen);
        assert_eq!(Tool::for_op(&json!({"op": "mermaid"})), Tool::Pen);
        assert_eq!(
            Tool::for_op(&json!({"op": "command", "command": "frame.create"})),
            Tool::Crosshair
        );
        assert_eq!(
            Tool::for_op(&json!({"op": "command", "command": "edit.duplicate"})),
            Tool::Duplicate
        );
        assert_eq!(Tool::for_op(&json!({"op": "update"})), Tool::Hand);
        for tool in [
            Tool::Arrow,
            Tool::Crosshair,
            Tool::IBeam,
            Tool::Pen,
            Tool::Duplicate,
            Tool::Hand,
        ] {
            assert!(!tool.shape().is_empty());
        }
    }
}
