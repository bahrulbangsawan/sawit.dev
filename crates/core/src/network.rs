//! Internet connection diagnosis: is there a network, does the router
//! answer, does DNS resolve, are public hosts reachable, and is a Wi-Fi
//! sign-in page (captive portal) in the way. Plus a plain ping of any host.
//!
//! Uses the system `ping` and `route` (no root needed) and plain sockets.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket, lookup_host};
use tokio::process::Command;
use tokio::time::timeout;

use crate::{CoreError, Result};

/// Public resolvers: well known, always up, answer ICMP.
pub const PUBLIC_HOSTS: [&str; 2] = ["1.1.1.1", "8.8.8.8"];
/// Resolved to test DNS.
pub const DNS_TEST_HOST: &str = "www.apple.com";
/// Answers plain HTTP with "Success" unless a sign-in page intercepts it.
const CAPTIVE_HOST: &str = "captive.apple.com";

const STEP_TIMEOUT: Duration = Duration::from_secs(6);

/// Result of pinging one host.
#[derive(Debug, Clone, PartialEq)]
pub struct PingStats {
    pub host: String,
    pub sent: u32,
    pub received: u32,
    /// Round-trip times in milliseconds; `None` when nothing came back.
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

impl PingStats {
    pub fn loss_percent(&self) -> f64 {
        if self.sent == 0 {
            return 100.;
        }
        (self.sent - self.received.min(self.sent)) as f64 * 100. / self.sent as f64
    }

    pub fn reachable(&self) -> bool {
        self.received > 0
    }
}

/// Parses the summary of macOS/BSD (and Linux) `ping`.
pub fn parse_ping(host: &str, output: &str) -> Option<PingStats> {
    let counts = output.lines().find(|l| l.contains("packets transmitted"))?;
    let number_before = |word: &str| -> Option<u32> {
        let ix = counts.find(word)?;
        counts[..ix].split_whitespace().last()?.parse().ok()
    };
    let sent = number_before("packets transmitted")?;
    let received = number_before("received").or_else(|| number_before("packets received"))?;
    // "round-trip min/avg/max/stddev = 9.1/10.2/11.3/0.8 ms" (Linux: "rtt").
    let times = output
        .lines()
        .find(|l| l.contains("min/avg/max"))
        .and_then(|l| l.split('=').nth(1))
        .map(|v| {
            v.trim()
                .trim_end_matches("ms")
                .split('/')
                .filter_map(|n| n.trim().parse::<f64>().ok())
                .collect::<Vec<_>>()
        })
        .filter(|t| t.len() >= 3);
    Some(PingStats {
        host: host.to_string(),
        sent,
        received,
        min_ms: times.as_ref().map(|t| t[0]),
        avg_ms: times.as_ref().map(|t| t[1]),
        max_ms: times.as_ref().map(|t| t[2]),
    })
}

/// Host names and addresses only: letters, digits, `.`, `-`, `:` (IPv6).
/// Keeps anything that looks like a flag away from `ping`.
pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

/// Pings `host` `count` times, a fifth of a second apart.
pub async fn ping(host: &str, count: u32) -> Result<PingStats> {
    if !valid_host(host) {
        return Err(CoreError::Network(format!(
            "“{host}” isn't a host name or IP address. Try something like example.com or 1.1.1.1."
        )));
    }
    let program = if host.contains(':') { "ping6" } else { "ping" };
    let mut cmd = Command::new(program);
    cmd.args(["-c", &count.to_string(), "-i", "0.2"]);
    if program == "ping" {
        // Overall deadline in seconds (macOS -t; Linux calls it -w).
        let deadline = (count / 5 + 3).to_string();
        cmd.args([if cfg!(target_os = "macos") { "-t" } else { "-w" }, &deadline]);
    }
    let out = cmd.arg(host).kill_on_drop(true).output().await?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    match parse_ping(host, &stdout) {
        Some(stats) => Ok(stats),
        None => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(CoreError::Network(if stderr.contains("cannot resolve")
                || stderr.contains("Unknown host")
                || stderr.contains("Name or service not known")
            {
                "Unable to find this host. Check the spelling, or run the diagnosis to test DNS.".to_string()
            } else if stderr.trim().is_empty() {
                format!("No reply from {host}.")
            } else {
                stderr.trim().trim_start_matches("ping: ").to_string()
            }))
        }
    }
}

/// Router address from `route -n get default` ("gateway: 192.168.1.1").
pub fn parse_gateway(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|l| l.trim().strip_prefix("gateway:"))
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
}

pub async fn default_gateway() -> Option<String> {
    let out = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .await
        .ok()?;
    parse_gateway(&String::from_utf8_lossy(&out.stdout))
}

/// This machine's address on the route to the internet. Connecting a UDP
/// socket sends nothing; it only picks the outgoing interface.
pub async fn local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    socket.connect("1.1.1.1:53").await.ok()?;
    Some(socket.local_addr().ok()?.ip()).filter(|ip| !ip.is_unspecified())
}

/// Resolves `host`, returning its addresses and how long it took.
pub async fn resolve(host: &str) -> Result<(Vec<IpAddr>, Duration)> {
    let start = Instant::now();
    let addrs: Vec<SocketAddr> = timeout(STEP_TIMEOUT, lookup_host((host, 443)))
        .await
        .map_err(|_| CoreError::Network(format!("Looking up {host} timed out.")))??
        .collect();
    Ok((addrs.into_iter().map(|a| a.ip()).collect(), start.elapsed()))
}

/// What the captive-portal check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Portal {
    /// The real page came back: open internet.
    Open,
    /// Something else answered: a sign-in page is intercepting traffic.
    SignInRequired,
}

/// Fetches Apple's captive-portal check page over plain HTTP.
pub async fn captive_portal() -> Result<(Portal, Duration)> {
    let start = Instant::now();
    let fetch = async {
        let mut stream = TcpStream::connect((CAPTIVE_HOST, 80)).await?;
        let request = format!(
            "GET /hotspot-detect.html HTTP/1.0\r\nHost: {CAPTIVE_HOST}\r\nUser-Agent: CaptiveNetworkSupport\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await?;
        let mut body = Vec::new();
        stream.take(64 * 1024).read_to_end(&mut body).await?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&body).into_owned())
    };
    let body = timeout(STEP_TIMEOUT, fetch)
        .await
        .map_err(|_| CoreError::Network(format!("{CAPTIVE_HOST} didn't answer in time.")))??;
    let portal = if body.contains("<BODY>Success</BODY>") || body.contains("Success</") {
        Portal::Open
    } else {
        Portal::SignInRequired
    };
    Ok((portal, start.elapsed()))
}

/// Outcome of each step, as far as the diagnosis got.
#[derive(Debug, Clone, Default)]
pub struct Findings {
    pub local_ip: Option<IpAddr>,
    pub gateway: Option<String>,
    /// `None`: not run (no gateway known).
    pub router: Option<std::result::Result<PingStats, String>>,
    pub dns: Option<std::result::Result<(Vec<IpAddr>, Duration), String>>,
    pub internet: Vec<std::result::Result<PingStats, String>>,
    pub portal: Option<std::result::Result<(Portal, Duration), String>>,
}

/// One-line answer to "is my internet working, and if not, where does it break?"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Online,
    /// Works, but slow or dropping packets.
    Degraded,
    NoNetwork,
    RouterUnreachable,
    NoInternet,
    DnsFailing,
    SignInRequired,
}

impl Verdict {
    pub fn title(self) -> &'static str {
        match self {
            Verdict::Online => "You're online",
            Verdict::Degraded => "Online, but the connection is unstable",
            Verdict::NoNetwork => "Not connected to a network",
            Verdict::RouterUnreachable => "Your router isn't responding",
            Verdict::NoInternet => "Connected to the network, but not the internet",
            Verdict::DnsFailing => "The internet works, but DNS doesn't",
            Verdict::SignInRequired => "This network needs you to sign in",
        }
    }

    pub fn advice(self) -> &'static str {
        match self {
            Verdict::Online => "Every check passed.",
            Verdict::Degraded => {
                "Some packets were lost or replies were slow. Move closer to the router, or try a wired connection."
            }
            Verdict::NoNetwork => "Turn on Wi-Fi or plug in a cable, then run the check again.",
            Verdict::RouterUnreachable => {
                "Check the Wi-Fi or cable connection, or restart the router."
            }
            Verdict::NoInternet => {
                "Your router answers but can't reach the internet. Restart the router, or check with your provider."
            }
            Verdict::DnsFailing => {
                "Websites can't be found by name. Try another DNS server (like 1.1.1.1) in System Settings → Network."
            }
            Verdict::SignInRequired => {
                "Open a browser to finish signing in to this Wi-Fi network (hotels, cafés, airports)."
            }
        }
    }
}

/// Slower average than this (ms) to public hosts counts as degraded.
const SLOW_MS: f64 = 250.;

pub fn verdict(f: &Findings) -> Verdict {
    if f.local_ip.is_none() && f.gateway.is_none() {
        return Verdict::NoNetwork;
    }
    let internet: Vec<&PingStats> = f.internet.iter().filter_map(|r| r.as_ref().ok()).collect();
    let online = internet.iter().any(|s| s.reachable());
    let router_down = matches!(&f.router, Some(Err(_)))
        || matches!(&f.router, Some(Ok(s)) if !s.reachable());
    if !online {
        // Some routers ignore ping; only blame the router when nothing else works.
        return if router_down {
            Verdict::RouterUnreachable
        } else {
            Verdict::NoInternet
        };
    }
    if matches!(f.portal, Some(Ok((Portal::SignInRequired, _)))) {
        return Verdict::SignInRequired;
    }
    if matches!(f.dns, Some(Err(_))) {
        return Verdict::DnsFailing;
    }
    let lossy = internet.iter().any(|s| s.loss_percent() > 10.);
    let slow = internet
        .iter()
        .filter_map(|s| s.avg_ms)
        .any(|ms| ms > SLOW_MS);
    if lossy || slow {
        Verdict::Degraded
    } else {
        Verdict::Online
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC_OK: &str = "\
PING 1.1.1.1 (1.1.1.1): 56 data bytes
64 bytes from 1.1.1.1: icmp_seq=0 ttl=57 time=12.204 ms

--- 1.1.1.1 ping statistics ---
4 packets transmitted, 4 packets received, 0.0% packet loss
round-trip min/avg/max/stddev = 11.020/12.345/14.100/1.104 ms
";

    const MAC_DOWN: &str = "\
PING 10.0.0.9 (10.0.0.9): 56 data bytes

--- 10.0.0.9 ping statistics ---
4 packets transmitted, 0 packets received, 100.0% packet loss
";

    const LINUX_PARTIAL: &str = "\
--- 8.8.8.8 ping statistics ---
4 packets transmitted, 3 received, 25% packet loss, time 3004ms
rtt min/avg/max/mdev = 9.1/10.2/11.3/0.8 ms
";

    fn stats(received: u32, avg: Option<f64>) -> PingStats {
        PingStats {
            host: "1.1.1.1".into(),
            sent: 4,
            received,
            min_ms: avg,
            avg_ms: avg,
            max_ms: avg,
        }
    }

    #[test]
    fn parses_macos_ping() {
        let s = parse_ping("1.1.1.1", MAC_OK).unwrap();
        assert_eq!((s.sent, s.received), (4, 4));
        assert_eq!(s.avg_ms, Some(12.345));
        assert_eq!(s.max_ms, Some(14.1));
        assert_eq!(s.loss_percent(), 0.);
    }

    #[test]
    fn parses_total_loss_without_times() {
        let s = parse_ping("10.0.0.9", MAC_DOWN).unwrap();
        assert!(!s.reachable());
        assert_eq!(s.avg_ms, None);
        assert_eq!(s.loss_percent(), 100.);
    }

    #[test]
    fn parses_linux_ping() {
        let s = parse_ping("8.8.8.8", LINUX_PARTIAL).unwrap();
        assert_eq!((s.sent, s.received), (4, 3));
        assert_eq!(s.min_ms, Some(9.1));
        assert_eq!(s.loss_percent(), 25.);
    }

    #[test]
    fn garbage_is_not_a_ping() {
        assert_eq!(parse_ping("x", "ping: cannot resolve x: Unknown host"), None);
    }

    #[test]
    fn parses_gateway() {
        let out = "   route to: default\ndestination: default\n    gateway: 192.168.1.1\n  interface: en0\n";
        assert_eq!(parse_gateway(out).as_deref(), Some("192.168.1.1"));
        assert_eq!(parse_gateway("route: writing to routing socket: not in table"), None);
    }

    #[test]
    fn rejects_flag_like_hosts() {
        assert!(valid_host("example.com"));
        assert!(valid_host("2606:4700:4700::1111"));
        assert!(!valid_host("-f"));
        assert!(!valid_host("a b"));
        assert!(!valid_host("host;rm"));
        assert!(!valid_host(""));
    }

    /// Talks to the real network: `cargo test -p sawit-core network -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_probes() {
        println!("local ip: {:?}", local_ip().await);
        println!("gateway: {:?}", default_gateway().await);
        println!("ping: {:?}", ping("1.1.1.1", 3).await);
        println!("dns: {:?}", resolve(DNS_TEST_HOST).await);
        println!("portal: {:?}", captive_portal().await);
        println!("bad host: {:?}", ping("no-such-host.invalid", 1).await);
    }

    #[test]
    fn verdicts() {
        let base = Findings {
            local_ip: Some("192.168.1.20".parse().unwrap()),
            gateway: Some("192.168.1.1".into()),
            router: Some(Ok(stats(4, Some(2.)))),
            dns: Some(Ok((vec![], Duration::from_millis(20)))),
            internet: vec![Ok(stats(4, Some(12.)))],
            portal: Some(Ok((Portal::Open, Duration::from_millis(50)))),
        };
        assert_eq!(verdict(&base), Verdict::Online);

        assert_eq!(verdict(&Findings::default()), Verdict::NoNetwork);

        let mut f = base.clone();
        f.internet = vec![Ok(stats(0, None))];
        assert_eq!(verdict(&f), Verdict::NoInternet);
        f.router = Some(Ok(stats(0, None)));
        assert_eq!(verdict(&f), Verdict::RouterUnreachable);

        let mut f = base.clone();
        f.dns = Some(Err("timed out".into()));
        assert_eq!(verdict(&f), Verdict::DnsFailing);

        let mut f = base.clone();
        f.portal = Some(Ok((Portal::SignInRequired, Duration::ZERO)));
        assert_eq!(verdict(&f), Verdict::SignInRequired);

        let mut f = base.clone();
        f.internet = vec![Ok(stats(2, Some(12.)))];
        assert_eq!(verdict(&f), Verdict::Degraded);
        f.internet = vec![Ok(stats(4, Some(400.)))];
        assert_eq!(verdict(&f), Verdict::Degraded);

        // A router that ignores ping doesn't matter when the internet works.
        let mut f = base;
        f.router = Some(Ok(stats(0, None)));
        assert_eq!(verdict(&f), Verdict::Online);
    }
}
