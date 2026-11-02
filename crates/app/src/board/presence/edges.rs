//! Agents at work off screen: a chip at the canvas's edge for each, which takes the view there
//! when clicked; and the polite status line that says what agents did.

use std::time::Instant;

use figcraft_doc::NodeId;
use figcraft_engine::ViewState;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;
use sawit_core::agents as model;

use super::{EDGE_CHIP, EDGE_INSET, Presence};
use crate::agents::Look;
use crate::board::BoardView;

/// Which way an off-screen agent is from the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::board) enum Toward {
    Left,
    Right,
    Up,
    Down,
}

/// An off-screen agent's chip at the canvas's edge.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Edge {
    pub look: Look,
    /// The chip's top-left from the canvas's top-left (px).
    pub at: (f64, f64),
    pub toward: Toward,
    /// Where the agent is on the page: the view goes there when the chip is clicked.
    pub point: (f64, f64),
}

/// Where the chip of an agent whose pointer is at `point` (px from the canvas's top-left) goes on
/// a canvas of `size`, and which way it points; `None` while the pointer is on the canvas.
pub(super) fn edge_chip(point: (f64, f64), size: (f64, f64)) -> Option<((f64, f64), Toward)> {
    let (x, y) = point;
    let (w, h) = size;
    if !(x.is_finite() && y.is_finite()) || (0.0..=w).contains(&x) && (0.0..=h).contains(&y) {
        return None;
    }
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (dx, dy) = (x - cx, y - cy);
    // The edge it leaves by: the side the pointer is furthest past, for the canvas's shape.
    let toward = if dx.abs() * h >= dy.abs() * w {
        if dx < 0.0 {
            Toward::Left
        } else {
            Toward::Right
        }
    } else if dy < 0.0 {
        Toward::Up
    } else {
        Toward::Down
    };
    let clamp = |v: f64, lo: f64, hi: f64| v.max(lo).min(hi.max(lo));
    let left = clamp(
        x - EDGE_CHIP.0 / 2.0,
        EDGE_INSET,
        w - EDGE_INSET - EDGE_CHIP.0,
    );
    let top = clamp(
        y - EDGE_CHIP.1 / 2.0,
        EDGE_INSET,
        h - EDGE_INSET - EDGE_CHIP.1,
    );
    Some(((left, top), toward))
}

impl Presence {
    /// The chips for the agents at work on page `page` whose pointer is off a canvas of `size`
    /// (px) shown as `view`: each kept just inside the edge nearest it, with the way to it.
    pub fn edges(
        &self,
        view: &ViewState,
        page: NodeId,
        size: (f64, f64),
        now: Instant,
    ) -> Vec<Edge> {
        let mut edges = Vec::new();
        for agent in &self.agents {
            let idle = now.saturating_duration_since(agent.seen);
            let (Some(at), Some(_)) = (agent.at, model::opacity(idle)) else {
                continue;
            };
            if at.page != page {
                continue;
            }
            let (x, y) = view.page_to_screen(at.point.0, at.point.1);
            if let Some(edge) = edge_chip((x, y), size) {
                edges.push(Edge {
                    look: agent.look.clone(),
                    at: edge.0,
                    toward: edge.1,
                    point: at.point,
                });
            }
        }
        edges
    }
}

impl BoardView {
    /// Takes the view to `point` on the page, where an off-screen agent works: a click on its
    /// chip. The zoom stays as it is.
    fn go_to_agent(&mut self, point: (f64, f64), cx: &mut Context<Self>) {
        if let Some(view) = self.view_mut() {
            view.cx = point.0;
            view.cy = point.1;
            cx.notify();
        }
    }

    /// Over the canvas: a chip at its edge for each agent at work off screen, which takes the
    /// view there, and the polite status line that says what agents did.
    pub(in crate::board) fn render_agents(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let status = div()
            .id("board-agent-activity")
            .role(Role::Status)
            .aria_label(self.agents.announcement.clone())
            .absolute()
            .top_0()
            .left_0()
            .size(px(1.0))
            .overflow_hidden()
            .into_any_element();
        let mut elements = vec![status];
        if !crate::agents::shown(cx) {
            return elements;
        }
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return elements;
        };
        let size = (
            f64::from(self.bounds.size.width),
            f64::from(self.bounds.size.height),
        );
        let now = cx.background_executor().now();
        let this = cx.entity().downgrade();
        let edges = self.agents.edges(&ds.view, ds.page, size, now);
        for (ix, edge) in edges.into_iter().enumerate() {
            let (icon, way) = match edge.toward {
                Toward::Left => (Lucide::ArrowLeft, "to the left"),
                Toward::Right => (Lucide::ArrowRight, "to the right"),
                Toward::Up => (Lucide::ArrowUp, "above"),
                Toward::Down => (Lucide::ArrowDown, "below"),
            };
            let name: SharedString =
                format!("Go to {}, at work off screen {way}", edge.look.label).into();
            let this = this.clone();
            let point = edge.point;
            elements.push(
                div()
                    .id(("board-agent-edge", ix))
                    .role(Role::Button)
                    .aria_label(name.clone())
                    .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
                    .absolute()
                    .left(px(edge.at.0 as f32))
                    .top(px(edge.at.1 as f32))
                    .h(px(EDGE_CHIP.1 as f32))
                    .max_w(px(EDGE_CHIP.0 as f32))
                    .px_2()
                    .gap_1()
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(white())
                    .bg(edge.look.color)
                    .text_color(white())
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .shadow_md()
                    .cursor_pointer()
                    .occlude()
                    .child(Icon::new(icon).size_3())
                    .child(div().min_w_0().truncate().child(edge.look.label.clone()))
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |view, cx| view.go_to_agent(point, cx));
                    })
                    .into_any_element(),
            );
        }
        elements
    }
}
