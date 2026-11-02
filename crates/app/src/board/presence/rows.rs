//! Each work's row: one 18 px line just above its frame, never over it. On the left the
//! component's name and its step ("Pricing card · Writing copy"), "Stalled", or the reason it
//! failed; on the right the agent's name in its colour and a badge: a spinner while it runs (a still
//! ring under reduced motion), a check once done, "!" once failed. The name goes 2 s after the work
//! is done and the check fades after it. A stalled or failed work has a button to dismiss it. On a
//! frame too narrow for all that, only the badge shows. The whole row reads on hover.

use std::time::Instant;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agents::work::RowLook;

use super::Work;
use super::paint::FAILED;
use crate::board::BoardView;

/// The row's height, and its gap above the frame (px).
pub(super) const ROW_HEIGHT: f32 = 18.0;
const ROW_GAP: f32 = 6.0;
/// Below this width (px) a frame's row shows its badge alone.
const NARROW: f64 = 150.0;

/// The row's ink on the page: the name, the step, and failure's.
struct Ink {
    name: Hsla,
    step: Hsla,
    failed: Hsla,
}

impl Ink {
    /// Ink that reads on a page of colour `page`.
    fn on(page: [u8; 3]) -> Self {
        let [r, g, b] = page.map(|c| f32::from(c) / 255.0);
        let light = 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.5;
        match light {
            true => Self {
                name: rgb(0x3a414b).into(),
                step: rgb(0x59616d).into(),
                failed: rgb(FAILED).into(),
            },
            false => Self {
                name: rgb(0xe5e8ec).into(),
                step: rgb(0x9ba4af).into(),
                failed: rgb(0xf08a80).into(),
            },
        }
    }
}

impl BoardView {
    /// The rows of the works on page `page` of `doc`, shown as `view`, at `now`.
    pub(super) fn render_rows(
        &self,
        doc: &Document,
        view: &ViewState,
        page: NodeId,
        now: Instant,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let ink = Ink::on(sawit_board::view::page_rgb(doc, page));
        let reduce = crate::agents::reduce_motion(cx);
        let this = cx.entity().downgrade();
        let (w, h) = (
            f64::from(self.bounds.size.width),
            f64::from(self.bounds.size.height),
        );
        let mut rows = Vec::new();
        for (ix, work) in self
            .agents
            .works
            .iter()
            .enumerate()
            .filter(|(_, w)| w.page == page)
        {
            let look = work.row(now);
            let Some(b) = doc
                .world_bounds(work.frame)
                .filter(|_| look != RowLook::Gone)
            else {
                continue;
            };
            let (x0, y0) = view.page_to_screen(b.x0, b.y0);
            let (x1, _) = view.page_to_screen(b.x1, b.y1);
            let top = y0 - f64::from(ROW_GAP + ROW_HEIGHT);
            if x1 < 0.0 || x0 > w || top > h || y0 < 0.0 {
                continue;
            }
            let narrow = x1 - x0 < NARROW;
            let row = div()
                .id(("board-work-row", ix))
                .absolute()
                .left(px(x0 as f32))
                .top(px(top as f32))
                .w(px((x1 - x0).max(ROW_HEIGHT.into()) as f32))
                .h(px(ROW_HEIGHT))
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(11.0))
                .line_height(px(ROW_HEIGHT))
                .whitespace_nowrap()
                // The whole row in words on hover: a narrow frame's, and a step or a reason
                // too long for its frame.
                .tooltip({
                    let said: SharedString = said(work, look).into();
                    move |window, cx| tooltip::Tooltip::new(said.clone()).build(window, cx)
                })
                .when(narrow, |row| row.justify_end())
                .when(!narrow, |row| row.child(what(work, look, &ink)))
                .child(who(work, look, &ink, reduce, narrow, this.clone()));
            rows.push(row.into_any_element());
        }
        rows
    }
}

/// The row in words, for a narrow frame's tooltip and the roster.
pub(super) fn said(work: &Work, look: RowLook) -> String {
    let state = match (look, &work.status) {
        (RowLook::Working { stalled: true }, _) => "Stalled".to_string(),
        (RowLook::Failed, sawit_core::agents::work::WorkStatus::Failed(reason)) => {
            format!("Failed: {reason}")
        }
        (RowLook::Done { .. } | RowLook::Gone, _) => "Done".to_string(),
        _ => work.step.clone().unwrap_or_else(|| "Building".into()),
    };
    format!("{} · {state} · {}", work.name, work.look.label)
}

/// The left of a row: the component's name, then its step, "Stalled" or why it failed.
fn what(work: &Work, look: RowLook, ink: &Ink) -> impl IntoElement + use<> {
    let after: Option<(String, Hsla)> = match (look, &work.status) {
        (RowLook::Working { stalled: true }, _) => Some(("· Stalled".into(), ink.step)),
        (RowLook::Working { .. }, _) => work
            .step
            .clone()
            .map(|step| (format!("· {step}"), ink.step)),
        (RowLook::Failed, sawit_core::agents::work::WorkStatus::Failed(reason)) => {
            Some((reason.clone(), ink.failed))
        }
        _ => None,
    };
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .overflow_hidden()
        .child(
            div()
                .flex_none()
                .max_w(px(160.0))
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ink.name)
                .child(work.name.clone()),
        )
        .when_some(after, |row, (text, color)| {
            let weight = if color == ink.failed {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            };
            row.child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(weight)
                    .text_color(color)
                    .child(text),
            )
        })
}

/// The right of a row: the agent's colour and name, and the work's badge, with a dismiss button on
/// a stalled or failed work; on a `narrow` frame, the badge alone.
fn who(
    work: &Work,
    look: RowLook,
    ink: &Ink,
    reduce: bool,
    narrow: bool,
    view: WeakEntity<BoardView>,
) -> impl IntoElement + use<> {
    let color = work.look.color;
    let named = !narrow
        && matches!(
            look,
            RowLook::Working { .. } | RowLook::Failed | RowLook::Done { name: true, .. }
        );
    let stalled = look == RowLook::Working { stalled: true };
    let badge: AnyElement = match look {
        RowLook::Working { stalled: true } => Icon::new(Lucide::Clock)
            .size_3()
            .text_color(ink.step)
            .into_any_element(),
        RowLook::Working { .. } if !reduce => {
            Spinner::new().xsmall().color(color).into_any_element()
        }
        RowLook::Working { .. } => Icon::new(Lucide::Circle)
            .size_3()
            .text_color(color)
            .into_any_element(),
        RowLook::Done { check, .. } => Icon::new(Lucide::CircleCheck)
            .size_3()
            .text_color(color.opacity(check))
            .into_any_element(),
        RowLook::Failed => div()
            .size(px(12.0))
            .rounded_full()
            .bg(rgb(FAILED))
            .flex()
            .items_center()
            .justify_center()
            .text_color(gpui_kit::white())
            .text_size(px(9.0))
            .font_weight(FontWeight::BOLD)
            .child("!")
            .into_any_element(),
        RowLook::Gone => div().into_any_element(),
    };
    let id = work.id.clone();
    let dismiss = (stalled || look == RowLook::Failed).then(|| {
        Button::new(SharedString::from(format!("board-work-dismiss-{id}")))
            .ghost()
            .xsmall()
            // In the row's ink: the button sits on the canvas's page, not on the app's chrome.
            .icon(Icon::new(Lucide::X).text_color(ink.name))
            .accessibility_label(format!("Dismiss {}", work.name))
            .tooltip(format!("Dismiss {}", work.name))
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.dismiss_work(&id, cx));
            })
    });
    h_flex()
        .flex_none()
        .gap(px(5.0))
        .items_center()
        .max_w(px(150.0))
        .when(named, |who| {
            who.child(
                div()
                    .size(px(8.0))
                    .rounded_full()
                    .bg(if stalled { ink.step } else { color }),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if stalled { ink.step } else { color })
                    .child(work.look.label.clone()),
            )
        })
        .child(badge)
        .children(dismiss)
}
