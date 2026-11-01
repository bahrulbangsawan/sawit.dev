//! The Network sidebar drawn: the verdict, a row for each check as it answers, and the hosts
//! pinged on request.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::network::Verdict;

use super::{NetworkPanel, PINGS, PingRow, State, Step, describe};
use crate::states::Placeholder;

impl NetworkPanel {
    fn render_verdict(&self, verdict: Verdict, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (icon, tint) = match verdict {
            Verdict::Online => (IconName::CircleCheck, theme.success),
            Verdict::Degraded | Verdict::SignInRequired => (IconName::TriangleAlert, theme.warning),
            _ => (IconName::CircleX, theme.danger),
        };
        h_flex()
            .items_start()
            .gap_2()
            .m_2()
            .p_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(tint.opacity(0.35))
            .bg(tint.opacity(0.08))
            .child(
                div()
                    .pt(px(1.))
                    .child(Icon::new(icon).small().text_color(tint)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .text_color(theme.foreground)
                            .whitespace_normal()
                            .child(verdict.title()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(verdict.advice()),
                    ),
            )
    }

    fn render_step(&self, step: Step, state: &State, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let (mark, detail): (AnyElement, Option<&String>) = match state {
            State::Running => (
                Spinner::new()
                    .xsmall()
                    .color(theme.muted_foreground)
                    .into_any_element(),
                None,
            ),
            State::Pass(d) => (
                Icon::new(IconName::CircleCheck)
                    .xsmall()
                    .text_color(theme.success)
                    .into_any_element(),
                Some(d),
            ),
            State::Warn(d) => (
                Icon::new(IconName::TriangleAlert)
                    .xsmall()
                    .text_color(theme.warning)
                    .into_any_element(),
                Some(d),
            ),
            State::Fail(d) => (
                Icon::new(IconName::CircleX)
                    .xsmall()
                    .text_color(theme.danger)
                    .into_any_element(),
                Some(d),
            ),
            State::Skipped(d) => (
                Icon::new(IconName::Minus)
                    .xsmall()
                    .text_color(theme.muted_foreground)
                    .into_any_element(),
                Some(d),
            ),
        };
        h_flex()
            .items_start()
            .gap_2()
            .px_3()
            .py_1p5()
            .child(div().pt(px(2.)).flex_none().child(mark))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_xs().font_medium().child(step.label()))
                    .when_some(detail, |el, d| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .whitespace_normal()
                                .child(d.clone()),
                        )
                    }),
            )
    }

    fn render_ping(&self, row: &PingRow, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let (mark, detail, tint): (AnyElement, String, Hsla) = match &row.result {
            None => (
                Spinner::new()
                    .xsmall()
                    .color(theme.muted_foreground)
                    .into_any_element(),
                "Pinging…".into(),
                theme.muted_foreground,
            ),
            Some(Ok(s)) if s.reachable() => (
                Icon::new(IconName::CircleCheck)
                    .xsmall()
                    .text_color(theme.success)
                    .into_any_element(),
                describe(s),
                theme.muted_foreground,
            ),
            Some(Ok(s)) => (
                Icon::new(IconName::CircleX)
                    .xsmall()
                    .text_color(theme.danger)
                    .into_any_element(),
                describe(s),
                theme.danger,
            ),
            Some(Err(e)) => (
                Icon::new(IconName::CircleX)
                    .xsmall()
                    .text_color(theme.danger)
                    .into_any_element(),
                e.clone(),
                theme.danger,
            ),
        };
        h_flex()
            .items_start()
            .gap_2()
            .px_3()
            .py_1()
            .child(div().pt(px(2.)).flex_none().child(mark))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .truncate()
                            .child(row.host.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(tint)
                            .whitespace_normal()
                            .child(detail),
                    ),
            )
    }
}

fn section(title: &'static str, cx: &App) -> Div {
    div()
        .px_3()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

impl Render for NetworkPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.running();
        let header = h_flex()
            .h(px(32.))
            .flex_none()
            .px_2()
            .gap_0p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .flex_1()
                    .gap_1p5()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::Globe).xsmall())
                    .child("NETWORK"),
            )
            .when(self.states.is_some(), |el| {
                el.child(
                    Button::new("network-rerun")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::RefreshCw)
                        .tooltip("Run Diagnosis Again")
                        .disabled(running)
                        .on_click(cx.listener(|this, _, _, cx| this.diagnose(cx))),
                )
            });

        let diagnosis = match &self.states {
            None => Placeholder::new(IconName::Globe, "Check your internet connection")
                .compact()
                .inline()
                .detail("Tests your network, router, DNS and internet access, then explains anything that's wrong.")
                .action(
                    Button::new("network-run")
                        .small()
                        .primary()
                        .label("Run Diagnosis")
                        .on_click(cx.listener(|this, _, _, cx| this.diagnose(cx))),
                )
                .into_any_element(),
            Some(states) => v_flex()
                .when_some(self.verdict, |el, v| el.child(self.render_verdict(v, cx)))
                .when(self.verdict.is_none(), |el| {
                    el.child(
                        div()
                            .px_3()
                            .pt_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Checking your connection…"),
                    )
                })
                .children(
                    Step::ALL
                        .iter()
                        .zip(states.iter())
                        .map(|(step, state)| self.render_step(*step, state, cx)),
                )
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(header)
            .child(
                v_flex()
                    .id("network-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(diagnosis)
                    .child(section("PING A HOST", cx))
                    .child(
                        h_flex()
                            .px_2()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&self.host).small().cleanable(true)),
                            )
                            .child(Button::new("network-ping").small().label("Ping").on_click(
                                cx.listener(|this, _, window, cx| this.ping_host(window, cx)),
                            )),
                    )
                    .when(self.pings.is_empty(), |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_1p5()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .whitespace_normal()
                                .child(format!(
                                    "Sends {PINGS} pings and shows the reply time and packet loss."
                                )),
                        )
                    })
                    .children(self.pings.iter().map(|row| self.render_ping(row, cx)))
                    .child(div().h_3()),
            )
    }
}
