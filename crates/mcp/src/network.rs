//! Internet connection diagnosis and ping, as in Sawit's Network panel.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::network::{self, DNS_TEST_HOST, Findings, PUBLIC_HOSTS, PingStats, Portal};

use crate::{Sawit, fail, ok};

/// Pings per host during the diagnosis (as the app).
const PINGS: u32 = 5;
const MAX_PINGS: u32 = 50;
/// Slower average than this (ms) counts as shaky (as `network::verdict`).
const SLOW_MS: f64 = 250.;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PingArgs {
    #[schemars(description = "Host name or IP address")]
    pub host: String,
    #[schemars(description = "Number of pings (default 5, max 50)")]
    pub count: Option<u32>,
}

fn ms(v: f64) -> String {
    if v < 10. {
        format!("{v:.1} ms")
    } else {
        format!("{v:.0} ms")
    }
}

/// `5/5 replies, avg 12 ms (min 10, max 15)`.
pub(crate) fn describe(s: &PingStats) -> String {
    let mut out = format!("{}/{} replies", s.received, s.sent);
    if s.loss_percent() > 0. {
        out.push_str(&format!(" ({:.0}% loss)", s.loss_percent()));
    }
    if let (Some(min), Some(avg), Some(max)) = (s.min_ms, s.avg_ms, s.max_ms) {
        out.push_str(&format!(
            ", avg {} (min {}, max {})",
            ms(avg),
            ms(min),
            ms(max)
        ));
    }
    out
}

/// The diagnosis as text: one line per step, then the verdict.
pub(crate) fn report(f: &Findings) -> String {
    let mut out = Vec::new();
    out.push(match (f.local_ip, &f.gateway) {
        (Some(ip), _) => format!("✓ Network: this Mac is {ip}"),
        (None, Some(_)) => "✓ Network: connected".to_string(),
        (None, None) => "✗ Network: no active Wi-Fi or Ethernet connection".to_string(),
    });
    let gw = f.gateway.clone().unwrap_or_default();
    out.push(match &f.router {
        None => "- Router: skipped (no router found)".to_string(),
        Some(Ok(s)) if s.reachable() => format!("✓ Router: {gw} · {}", describe(s)),
        Some(Ok(_)) => format!("! Router: {gw} didn't answer ping (some routers never do)"),
        Some(Err(e)) => format!("! Router: {e}"),
    });
    out.push(match &f.dns {
        Some(Ok((addrs, took))) if !addrs.is_empty() => format!(
            "✓ DNS: found {DNS_TEST_HOST} in {}",
            ms(took.as_secs_f64() * 1000.)
        ),
        Some(Ok(_)) => format!("✗ DNS: {DNS_TEST_HOST} has no addresses"),
        Some(Err(e)) => format!("✗ DNS: {e}"),
        None => "- DNS: not checked".to_string(),
    });
    for r in &f.internet {
        out.push(match r {
            Ok(s) if !s.reachable() => format!("✗ Internet: no reply from {}", s.host),
            Ok(s) if s.loss_percent() > 10. || s.avg_ms.is_some_and(|a| a > SLOW_MS) => {
                format!("! Internet: {} · {}", s.host, describe(s))
            }
            Ok(s) => format!("✓ Internet: {} · {}", s.host, describe(s)),
            Err(e) => format!("✗ Internet: {e}"),
        });
    }
    out.push(match &f.portal {
        Some(Ok((Portal::Open, _))) => "✓ Wi-Fi sign-in: not needed".to_string(),
        Some(Ok((Portal::SignInRequired, _))) => {
            "✗ Wi-Fi sign-in: a sign-in page is blocking traffic".to_string()
        }
        Some(Err(e)) => format!("- Wi-Fi sign-in: unable to check. {e}"),
        None => "- Wi-Fi sign-in: not checked".to_string(),
    });
    let verdict = network::verdict(f);
    format!(
        "{}\n{}\n\n{}",
        verdict.title(),
        verdict.advice(),
        out.join("\n")
    )
}

impl Sawit {
    async fn diagnose_network_tool(&self) -> CallToolResult {
        let route = async { (network::local_ip().await, network::default_gateway().await) };
        let dns = network::resolve(DNS_TEST_HOST);
        let pings = futures::future::join_all(PUBLIC_HOSTS.iter().map(|h| network::ping(h, PINGS)));
        let portal = network::captive_portal();
        let ((local_ip, gateway), dns, internet, portal) = tokio::join!(route, dns, pings, portal);
        let router = match &gateway {
            Some(gw) => Some(network::ping(gw, PINGS).await.map_err(|e| e.to_string())),
            None => None,
        };
        let findings = Findings {
            local_ip,
            gateway,
            router,
            dns: Some(dns.map_err(|e| e.to_string())),
            internet: internet
                .into_iter()
                .map(|r| r.map_err(|e| e.to_string()))
                .collect(),
            portal: Some(portal.map_err(|e| e.to_string())),
        };
        ok(report(&findings))
    }

    async fn ping_tool(&self, a: PingArgs) -> CallToolResult {
        let count = a.count.unwrap_or(PINGS).clamp(1, MAX_PINGS);
        match network::ping(a.host.trim(), count).await {
            Ok(s) if s.reachable() => ok(format!("{} · {}", s.host, describe(&s))),
            Ok(s) => fail(format!("No reply from {} ({} sent).", s.host, s.sent)),
            Err(e) => fail(e.to_string()),
        }
    }
}

#[tool_router(router = network_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Diagnose the internet connection: local network, router, DNS, reachability of 1.1.1.1/8.8.8.8 and Wi-Fi sign-in pages. Returns a verdict with advice and each step's result (takes a few seconds)"
    )]
    async fn diagnose_network(&self) -> CallToolResult {
        self.diagnose_network_tool().await
    }

    #[tool(description = "Ping a host (name or IP) and report replies, loss and round-trip times")]
    async fn ping(&self, Parameters(a): Parameters<PingArgs>) -> CallToolResult {
        self.ping_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(received: u32, avg: f64) -> PingStats {
        PingStats {
            host: "1.1.1.1".into(),
            sent: 5,
            received,
            min_ms: Some(avg - 1.),
            avg_ms: Some(avg),
            max_ms: Some(avg + 1.),
        }
    }

    #[test]
    fn describes_pings() {
        assert_eq!(
            describe(&stats(5, 12.)),
            "5/5 replies, avg 12 ms (min 11 ms, max 13 ms)"
        );
        assert!(describe(&stats(4, 5.)).contains("(20% loss)"));
    }

    #[test]
    fn report_has_verdict_first() {
        let f = Findings {
            local_ip: Some("192.168.1.2".parse().unwrap()),
            gateway: Some("192.168.1.1".into()),
            router: Some(Ok(stats(5, 2.))),
            dns: Some(Ok((
                vec!["1.2.3.4".parse().unwrap()],
                std::time::Duration::from_millis(20),
            ))),
            internet: vec![Ok(stats(5, 12.)), Ok(stats(5, 14.))],
            portal: Some(Ok((Portal::Open, std::time::Duration::from_millis(30)))),
        };
        let text = report(&f);
        assert!(text.starts_with("You're online\n"), "{text}");
        assert!(text.contains("✓ Router: 192.168.1.1"));
        let offline = report(&Findings::default());
        assert!(offline.starts_with("Not connected to a network"));
    }
}
