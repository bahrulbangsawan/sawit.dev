//! Network sidebar: diagnoses the internet connection step by step
//! (network, router, DNS, internet, Wi-Fi sign-in) with a plain-language
//! verdict, and pings any host on request.

mod render;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::network::{
    self, DNS_TEST_HOST, Findings, PUBLIC_HOSTS, PingStats, Portal, Verdict,
};

use crate::rt;

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

fn joined<T, E: ToString>(
    r: Result<Result<T, E>, futures::channel::oneshot::Canceled>,
) -> Result<T, String> {
    match r {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("The check was interrupted.".into()),
    }
}

impl NetworkPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let host =
            cx.new(|cx| InputState::new(window, cx).placeholder("Host or IP, like github.com"));
        let subscriptions =
            vec![
                cx.subscribe_in(&host, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.ping_host(window, cx);
                    }
                }),
            ];
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
    pub fn diagnose(&mut self, cx: &mut Context<Self>) {
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
                            (None, None) => {
                                State::Fail("No active Wi-Fi or Ethernet connection.".into())
                            }
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
                            Ok(s) if s.reachable() => {
                                State::Pass(format!("{gw} · {}", describe(s)))
                            }
                            // Many routers ignore ping; the internet step decides.
                            Ok(_) => State::Warn(format!(
                                "{gw} didn't answer ping. Some routers never do."
                            )),
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
        self.ping(host, cx);
    }

    /// Pings `host` and adds its row to the top of the list.
    pub fn ping(&mut self, host: String, cx: &mut Context<Self>) {
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
}
