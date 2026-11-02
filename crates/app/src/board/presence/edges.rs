//! Agents at work off screen: one chip per edge of the canvas for the agents past it ("2 agents
//! →", with their colours), kept clear of the toolbar; a click lists them, and the user's choice
//! takes the view to one. And the polite status line that says what agents did.

use std::time::Instant;

use figcraft_doc::NodeId;
use figcraft_engine::ViewState;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agents as model;

use super::{EDGE_INSET, Presence, Spot};
use crate::agents::Look;
use crate::board::BoardView;

/// Which way off-screen agents are from the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::board) enum Toward {
    Left,
    Right,
    Up,
    Down,
}

/// The chip of the agents past one edge.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Edge {
    pub toward: Toward,
    /// Where along the edge it sits: the middle of where its agents are past it, kept on the
    /// canvas (px from the canvas's top or left).
    pub along: f64,
    /// Its agents, the most recent first, and where each works.
    pub agents: Vec<(Look, Spot, String)>,
}

/// A chip's height, and its width for placing it along an edge (px).
pub(super) const CHIP_HEIGHT: f64 = 24.0;
const CHIP_ROOM: f64 = 140.0;
/// How far above the canvas's bottom a chip for agents below stays: clear of the toolbar, which
/// floats there.
pub(super) const ABOVE_TOOLBAR: f64 = 16.0 + 44.0 + 8.0;

/// The edge a point `point` (px from the canvas's top-left) is past on a canvas of `size`, and
/// where along it the point is; `None` while it is on the canvas.
pub(super) fn edge_of(point: (f64, f64), size: (f64, f64)) -> Option<(Toward, f64)> {
    let ((x, y), (w, h)) = (point, size);
    if !(x.is_finite() && y.is_finite()) || (0.0..=w).contains(&x) && (0.0..=h).contains(&y) {
        return None;
    }
    let (dx, dy) = (x - w / 2.0, y - h / 2.0);
    // The edge it leaves by: the side the point is furthest past, for the canvas's shape.
    Some(if dx.abs() * h >= dy.abs() * w {
        (
            if dx < 0.0 {
                Toward::Left
            } else {
                Toward::Right
            },
            y,
        )
    } else {
        (if dy < 0.0 { Toward::Up } else { Toward::Down }, x)
    })
}

impl Presence {
    /// The chips for the agents at work on page `page` whose pointer is off a canvas of `size`
    /// (px) shown as `view`: one per edge, in the order Left, Right, Up, Down.
    pub fn edges(
        &self,
        view: &ViewState,
        page: NodeId,
        size: (f64, f64),
        now: Instant,
    ) -> Vec<Edge> {
        let mut agents: Vec<_> = self.agents.iter().collect();
        agents.sort_by_key(|a| std::cmp::Reverse(a.seen));
        let mut edges: Vec<(Edge, Vec<f64>)> = Vec::new();
        for agent in agents {
            let idle = now.saturating_duration_since(agent.seen);
            let (Some(at), Some(_)) = (agent.at.filter(|at| at.page == page), model::opacity(idle))
            else {
                continue;
            };
            let point = view.page_to_screen(at.point.0, at.point.1);
            let Some((toward, along)) = edge_of(point, size) else {
                continue;
            };
            let entry = (agent.look.clone(), at, agent.doing.clone());
            match edges.iter_mut().find(|(e, _)| e.toward == toward) {
                Some((edge, alongs)) => {
                    edge.agents.push(entry);
                    alongs.push(along);
                }
                None => edges.push((
                    Edge {
                        toward,
                        along: 0.0,
                        agents: vec![entry],
                    },
                    vec![along],
                )),
            }
        }
        let order = [Toward::Left, Toward::Right, Toward::Up, Toward::Down];
        edges.sort_by_key(|(e, _)| order.iter().position(|t| *t == e.toward));
        (edges.into_iter())
            .map(|(mut edge, alongs)| {
                let middle = alongs.iter().sum::<f64>() / alongs.len() as f64;
                let (room, length) = match edge.toward {
                    Toward::Left | Toward::Right => (CHIP_HEIGHT, size.1 - ABOVE_TOOLBAR),
                    Toward::Up | Toward::Down => (CHIP_ROOM, size.0),
                };
                let high = (length - EDGE_INSET - room).max(EDGE_INSET);
                edge.along = (middle - room / 2.0).clamp(EDGE_INSET, high);
                edge
            })
            .collect()
    }
}

impl BoardView {
    /// Over the canvas: a chip at each edge with agents at work past it, and the polite status
    /// line that says what agents did.
    pub(in crate::board) fn render_agents(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let status = div()
            .id("board-agent-activity")
            .role(Role::Status)
            .aria_label(self.agents.announcer.current.clone())
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
        elements.extend(self.render_rows(&ds.doc, &ds.view, ds.page, now, cx));
        let this = cx.entity().downgrade();
        for edge in self.agents.edges(&ds.view, ds.page, size, now) {
            elements.push(edge_chip(edge, this.clone(), cx));
        }
        elements
    }
}

/// The chip of `edge`: its agents' colours, who they are, and the way to them; a click lists them.
fn edge_chip(edge: Edge, view: WeakEntity<BoardView>, cx: &Context<BoardView>) -> AnyElement {
    let (icon, way, id) = match edge.toward {
        Toward::Left => (Lucide::ArrowLeft, "to the left", "board-agents-left"),
        Toward::Right => (Lucide::ArrowRight, "to the right", "board-agents-right"),
        Toward::Up => (Lucide::ArrowUp, "above", "board-agents-up"),
        Toward::Down => (Lucide::ArrowDown, "below", "board-agents-down"),
    };
    let who: SharedString = match edge.agents.as_slice() {
        [(look, ..)] => look.label.clone(),
        agents => format!("{} agents", agents.len()).into(),
    };
    let name: SharedString = format!("{who} at work off screen {way}: list them").into();
    let dots = h_flex().children(edge.agents.iter().enumerate().map(|(ix, (look, ..))| {
        div()
            .size(px(10.0))
            .rounded_full()
            .border_1()
            .border_color(gpui_kit::white())
            .bg(look.color)
            .when(ix > 0, |dot| dot.ml(px(-3.0)))
    }));
    let agents = edge.agents.clone();
    let chip = Button::new(id)
        .small()
        .rounded(ButtonRounded::Large)
        .accessibility_label(name.clone())
        .tooltip(name)
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(rgb(0x3a414b))
                .child(dots)
                .child(div().max_w(px(120.0)).truncate().child(who))
                .child(Icon::new(icon).size_3()),
        )
        .bg(gpui_kit::white())
        .border_1()
        .border_color(rgb(0xd5d9df))
        .shadow_sm()
        .dropdown_menu(move |mut menu: PopupMenu, _, _| {
            for (look, spot, doing) in &agents {
                let (view, spot) = (view.clone(), *spot);
                let label = format!("{} · {doing}", look.label);
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                    let _ = view.update(cx, |view, cx| view.go_to(spot.page, spot.point, cx));
                }));
            }
            menu
        });
    let _ = cx;
    let at = div().absolute().occlude();
    let at = match edge.toward {
        Toward::Left => at.left(px(EDGE_INSET as f32)).top(px(edge.along as f32)),
        Toward::Right => at.right(px(EDGE_INSET as f32)).top(px(edge.along as f32)),
        Toward::Up => at.top(px(EDGE_INSET as f32)).left(px(edge.along as f32)),
        Toward::Down => at
            .bottom(px(ABOVE_TOOLBAR as f32))
            .left(px(edge.along as f32)),
    };
    at.child(chip).into_any_element()
}
