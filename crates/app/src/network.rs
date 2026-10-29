//! Network sidebar: diagnoses the internet connection step by step
//! (network, router, DNS, internet, Wi-Fi sign-in) with a plain-language
//! verdict, and pings any host on request.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::network::{
    self, DNS_TEST_HOST, Findings, PUBLIC_HOSTS, PingStats, Portal, Verdict,
};

use crate::rt;
use crate::states::Placeholder;

/// Pings per host: enough to see loss, quick enough to wait for.
const PINGS: u32 = 4;
/// Custom ping results kept on screen.
const MAX_PINGS: usize = 5;
/// Public-host latency above this is flagged as slow.
const SLOW_MS: f64 = 250.;

#[derive(Clone)]
enum State {
    Running,
    Pass(String),
    Warn(String),
    Fail(String),
    Skipped(String),
}

#[derive(Clone, Copy)]
enum Step {
    Network,
    Router,
    Dns,
    Internet,
    SignIn,
}

impl Step {
    const ALL: [Step; 5] = [
        Step::Network,
        Step::Router,
        Step::Dns,
        Step::Internet,
        Step::SignIn,
    ];

    fn label(self) -> &'static str {
        match self {
            Step::Network => "Network connection",
            Step::Router => "Router",
            Step::Dns => "DNS lookup",
            Step::Internet => "Internet",
            Step::SignIn => "Wi-Fi sign-in",
        }
    }
}

struct PingRow {
    host: String,
    result: Option<Result<PingStats, String>>,
}

pub struct NetworkPanel {
    states: Option<[State; 5]>,
    findings: Findings,
    verdict: Option<Verdict>,
    host: Entity<InputState>,
    pings: Vec<PingRow>,
    _subscriptions: Vec<Subscription>,
}

fn ms(d: f64) -> String {
    if d < 10. {
        format!("{d:.1} ms")
    } else {
        format!("{d:.0} ms")
    }
}

/// "41 ms · 0% loss", or why nothing came back.
fn describe(s: &PingStats) -> String {
    match s.avg_ms {
        Some(avg) if s.reachable() => format!("{} · {:.0}% loss", ms(avg), s.loss_percent()),
        _ => format!("No reply ({} sent)", s.sent),
    }
}

fn joined<T, E: ToString>(r: Result<Result<T, E>, futures::channel::oneshot::Canceled>) -> Result<T, String> {
    match r {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("The check was interrupted.".into()),
    }
}

impl NetworkPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let host = cx.new(|cx| InputState::new(window, cx).placeholder("Host or IP, like github.com"));
        let subscriptions = vec![cx.subscribe_in(&host, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.ping_host(window, cx);
            }
        })];
        Self {
            states: None,
            findings: Findings::default(),
            verdict: None,
            host,
            pings: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    fn running(&self) -> bool {
        self.states
            .as_ref()
            .is_some_and(|s| s.iter().any(|s| matches!(s, State::Running)))
    }

    fn set(&mut self, step: Step, state: State) {
        if let Some(states) = &mut self.states {
            states[step as usize] = state;
        }
    }

    /// Runs every check; rows fill in as each one answers.
    fn diagnose(&mut self, cx: &mut Context<Self>) {
        if self.running() {
            return;
        }
        self.states = Some(std::array::from_fn(|_| State::Running));
        self.findings = Findings::default();
        self.verdict = None;
        cx.notify();

        let route = rt::spawn(cx, async {
            (network::local_ip().await, network::default_gateway().await)
        });
        let dns = rt::spawn(cx, network::resolve(DNS_TEST_HOST));
        let pings: Vec<_> = PUBLIC_HOSTS
            .iter()
            .map(|h| rt::spawn(cx, network::ping(h, PINGS)))
            .collect();
        let portal = rt::spawn(cx, network::captive_portal());

        cx.spawn(async move |this, cx| {
            let (ip, gateway) = route.await.unwrap_or((None, None));
            let router = this
                .update(cx, |this, cx| {
                    this.findings.local_ip = ip;
                    this.findings.gateway = gateway.clone();
                    this.set(
                        Step::Network,
                        match (ip, &gateway) {
                            (Some(ip), _) => State::Pass(format!("This Mac is {ip}")),
                            (None, Some(_)) => State::Pass("Connected".into()),
                            (None, None) => State::Fail("No active Wi-Fi or Ethernet connection.".into()),
                        },
                    );
                    if gateway.is_none() {
                        this.set(Step::Router, State::Skipped("No router found.".into()));
                    }
                    cx.notify();
                    gateway.map(|gw| rt::spawn(cx, async move { network::ping(&gw, PINGS).await }))
                })
                .ok()
                .flatten();

            if let Some(router) = router {
                let result = joined(router.await);
                let _ = this.update(cx, |this, cx| {
                    let gw = this.findings.gateway.clone().unwrap_or_default();
                    this.set(
                        Step::Router,
                        match &result {
                            Ok(s) if s.reachable() => State::Pass(format!("{gw} · {}", describe(s))),
                            // Many routers ignore ping; the internet step decides.
                            Ok(_) => State::Warn(format!("{gw} didn't answer ping. Some routers never do.")),
                            Err(e) => State::Warn(e.clone()),
                        },
                    );
                    this.findings.router = Some(result);
                    cx.notify();
                });
            }

            let result = joined(dns.await);
            let _ = this.update(cx, |this, cx| {
                this.set(
                    Step::Dns,
                    match &result {
                        Ok((addrs, took)) if !addrs.is_empty() => State::Pass(format!(
                            "Found {DNS_TEST_HOST} in {}",
                            ms(took.as_secs_f64() * 1000.)
                        )),
                        Ok(_) => State::Fail(format!("{DNS_TEST_HOST} has no addresses.")),
                        Err(e) => State::Fail(e.clone()),
                    },
                );
                this.findings.dns = Some(result);
                cx.notify();
            });

            let mut internet = Vec::new();
            for rx in pings {
                internet.push(joined(rx.await));
            }
            let _ = this.update(cx, |this, cx| {
                let ok: Vec<&PingStats> = internet.iter().filter_map(|r| r.as_ref().ok()).collect();
                let summary = ok
                    .iter()
                    .map(|s| format!("{} {}", s.host, describe(s)))
                    .collect::<Vec<_>>()
                    .join("\n");
                let reachable = ok.iter().any(|s| s.reachable());
                let shaky = ok
                    .iter()
                    .any(|s| s.loss_percent() > 10. || s.avg_ms.is_some_and(|a| a > SLOW_MS));
                this.set(
                    Step::Internet,
                    if !reachable {
                        State::Fail(format!("No reply from {}.", PUBLIC_HOSTS.join(" or ")))
                    } else if shaky {
                        State::Warn(summary)
                    } else {
                        State::Pass(summary)
                    },
                );
                this.findings.internet = internet;
                cx.notify();
            });

            let result = joined(portal.await);
            let _ = this.update(cx, |this, cx| {
                this.set(
                    Step::SignIn,
                    match &result {
                        Ok((Portal::Open, _)) => State::Pass("Not needed".into()),
                        Ok((Portal::SignInRequired, _)) => {
                            State::Fail("A sign-in page is blocking traffic.".into())
                        }
                        Err(e) => State::Skipped(format!("Unable to check. {e}")),
                    },
                );
                this.findings.portal = Some(result);
                this.verdict = Some(network::verdict(&this.findings));
                cx.notify();
            });
        })
        .detach();
    }

    fn ping_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.host.read(cx).value().trim().to_string();
        if host.is_empty() {
            return;
        }
        self.host.update(cx, |s, cx| s.set_value("", window, cx));
        self.pings.insert(
            0,
            PingRow {
                host: host.clone(),
                result: None,
            },
        );
        self.pings.truncate(MAX_PINGS);
        cx.notify();
        let target = host.clone();
        let rx = rt::spawn(cx, async move { network::ping(&target, PINGS).await });
        cx.spawn(async move |this, cx| {
            let result = joined(rx.await);
            let _ = this.update(cx, |this, cx| {
                // The newest pending row for this host.
                if let Some(row) = this
                    .pings
                    .iter_mut()
                    .find(|r| r.host == host && r.result.is_none())
                {
                    row.result = Some(result);
                }
                cx.notify();
            });
        })
        .detach();
    }

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
            .child(div().pt(px(1.)).child(Icon::new(icon).small().text_color(tint)))
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
            State::Running => (Spinner::new().xsmall().color(theme.muted_foreground).into_any_element(), None),
            State::Pass(d) => (Icon::new(IconName::CircleCheck).xsmall().text_color(theme.success).into_any_element(), Some(d)),
            State::Warn(d) => (Icon::new(IconName::TriangleAlert).xsmall().text_color(theme.warning).into_any_element(), Some(d)),
            State::Fail(d) => (Icon::new(IconName::CircleX).xsmall().text_color(theme.danger).into_any_element(), Some(d)),
            State::Skipped(d) => (Icon::new(IconName::Minus).xsmall().text_color(theme.muted_foreground).into_any_element(), Some(d)),
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
                Spinner::new().xsmall().color(theme.muted_foreground).into_any_element(),
                "Pinging…".into(),
                theme.muted_foreground,
            ),
            Some(Ok(s)) if s.reachable() => (
                Icon::new(IconName::CircleCheck).xsmall().text_color(theme.success).into_any_element(),
                describe(s),
                theme.muted_foreground,
            ),
            Some(Ok(s)) => (
                Icon::new(IconName::CircleX).xsmall().text_color(theme.danger).into_any_element(),
                describe(s),
                theme.danger,
            ),
            Some(Err(e)) => (
                Icon::new(IconName::CircleX).xsmall().text_color(theme.danger).into_any_element(),
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
                    .child(div().text_xs().font_medium().truncate().child(row.host.clone()))
                    .child(div().text_xs().text_color(tint).whitespace_normal().child(detail)),
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
                            .child(div().flex_1().child(Input::new(&self.host).small().cleanable(true)))
                            .child(
                                Button::new("network-ping")
                                    .small()
                                    .label("Ping")
                                    .on_click(cx.listener(|this, _, window, cx| this.ping_host(window, cx))),
                            ),
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
