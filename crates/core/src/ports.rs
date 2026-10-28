//! Listening-socket model, `lsof` parsing and port conflict analysis.

use std::net::IpAddr;

/// Address a socket is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindAddr {
    AnyV4,
    /// `*` on an IPv6 socket. Treated as dual-stack because `lsof` does not
    /// show `IPV6_V6ONLY`; when unsure we prefer to warn.
    AnyV6,
    LoopbackV4,
    LoopbackV6,
    Specific(IpAddr),
}

impl BindAddr {
    /// Which address families (v4, v6) this bind can receive traffic for.
    pub fn families(&self) -> (bool, bool) {
        match self {
            BindAddr::AnyV4 | BindAddr::LoopbackV4 => (true, false),
            BindAddr::AnyV6 => (true, true),
            BindAddr::LoopbackV6 => (false, true),
            BindAddr::Specific(ip) => (ip.is_ipv4(), ip.is_ipv6()),
        }
    }

    pub fn is_wildcard(&self) -> bool {
        matches!(self, BindAddr::AnyV4 | BindAddr::AnyV6)
    }

    fn overlaps(&self, other: &BindAddr) -> bool {
        let (a4, a6) = self.families();
        let (b4, b6) = other.families();
        (a4 && b4) || (a6 && b6)
    }

    fn from_host(host: &str, ipv6: bool) -> Option<BindAddr> {
        if host == "*" {
            return Some(if ipv6 {
                BindAddr::AnyV6
            } else {
                BindAddr::AnyV4
            });
        }
        let ip: IpAddr = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .ok()?;
        Some(match ip {
            ip if ip.is_unspecified() && ip.is_ipv4() => BindAddr::AnyV4,
            ip if ip.is_unspecified() => BindAddr::AnyV6,
            ip if ip.is_loopback() && ip.is_ipv4() => BindAddr::LoopbackV4,
            ip if ip.is_loopback() => BindAddr::LoopbackV6,
            ip => BindAddr::Specific(ip),
        })
    }
}

impl std::fmt::Display for BindAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindAddr::AnyV4 => f.write_str("0.0.0.0"),
            BindAddr::AnyV6 => f.write_str("[::]"),
            BindAddr::LoopbackV4 => f.write_str("127.0.0.1"),
            BindAddr::LoopbackV6 => f.write_str("[::1]"),
            BindAddr::Specific(IpAddr::V6(ip)) => write!(f, "[{ip}]"),
            BindAddr::Specific(ip) => write!(f, "{ip}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub pid: u32,
    pub command: String,
    pub addr: BindAddr,
    pub port: u16,
}

/// Parses `lsof -nP -iTCP:<port> -sTCP:LISTEN -F pctn` output.
pub fn parse_lsof(output: &str) -> Vec<Listener> {
    let mut listeners = Vec::new();
    let (mut pid, mut command, mut ipv6) = (0u32, String::new(), false);
    for line in output.lines() {
        let Some(field) = line.chars().next() else {
            continue;
        };
        let value = &line[field.len_utf8()..];
        match field {
            'p' => {
                pid = value.parse().unwrap_or(0);
                command.clear();
            }
            'c' => command = value.to_string(),
            'f' => ipv6 = false,
            't' => ipv6 = value == "IPv6",
            'n' => {
                let Some((host, port)) = value.rsplit_once(':') else {
                    continue;
                };
                let (Some(addr), Ok(port)) = (BindAddr::from_host(host, ipv6), port.parse()) else {
                    continue;
                };
                listeners.push(Listener {
                    pid,
                    command: command.clone(),
                    addr,
                    port,
                });
            }
            _ => {}
        }
    }
    listeners
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortVerdict {
    Free,
    /// Someone holds the exact address we want: starting will fail.
    Conflict {
        same_addr: Vec<Listener>,
        shadowing: Vec<Listener>,
    },
    /// Overlapping binds of different specificity: starting may succeed but
    /// clients reach different servers depending on the address they use.
    Shadowing {
        listeners: Vec<Listener>,
    },
}

/// Judges whether binding `intended` addresses on `port` collides with `listeners`.
pub fn analyze(port: u16, intended: &[BindAddr], listeners: &[Listener]) -> PortVerdict {
    let mut same_addr = Vec::new();
    let mut shadowing = Vec::new();
    for l in listeners.iter().filter(|l| l.port == port) {
        let relevant: Vec<_> = intended.iter().filter(|a| a.overlaps(&l.addr)).collect();
        if relevant.iter().any(|a| **a == l.addr) {
            same_addr.push(l.clone());
        } else if relevant
            .iter()
            .any(|a| a.is_wildcard() || l.addr.is_wildcard())
        {
            shadowing.push(l.clone());
        }
    }
    match (same_addr.is_empty(), shadowing.is_empty()) {
        (true, true) => PortVerdict::Free,
        (true, false) => PortVerdict::Shadowing {
            listeners: shadowing,
        },
        (false, _) => PortVerdict::Conflict {
            same_addr,
            shadowing,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCKER: &str = "p14087\nccom.docker.backend\nf74\ntIPv6\nn*:5433\n";
    const MIXED: &str =
        "p58423\ncPython\nf3\ntIPv4\nn127.0.0.1:57360\nf4\ntIPv6\nn*:57360\nf5\ntIPv4\nn*:57360\n";
    const LOCAL: [BindAddr; 2] = [BindAddr::LoopbackV4, BindAddr::LoopbackV6];

    fn listener(addr: BindAddr, port: u16) -> Listener {
        Listener {
            pid: 1,
            command: "x".into(),
            addr,
            port,
        }
    }

    #[test]
    fn parses_docker_v6_wildcard() {
        assert_eq!(
            parse_lsof(DOCKER),
            vec![Listener {
                pid: 14087,
                command: "com.docker.backend".into(),
                addr: BindAddr::AnyV6,
                port: 5433
            }]
        );
    }

    #[test]
    fn parses_mixed_fds_of_one_process() {
        let addrs: Vec<_> = parse_lsof(MIXED)
            .into_iter()
            .map(|l| (l.pid, l.addr, l.port))
            .collect();
        assert_eq!(
            addrs,
            vec![
                (58423, BindAddr::LoopbackV4, 57360),
                (58423, BindAddr::AnyV6, 57360),
                (58423, BindAddr::AnyV4, 57360),
            ]
        );
    }

    #[test]
    fn parses_ipv6_loopback_brackets() {
        let l = parse_lsof("p9\ncpostgres\nf7\ntIPv6\nn[::1]:5432\n");
        assert_eq!(l[0].addr, BindAddr::LoopbackV6);
        assert_eq!(l[0].port, 5432);
    }

    #[test]
    fn parses_specific_address() {
        let l = parse_lsof("p9\ncpostgres\nf7\ntIPv4\nn192.168.1.5:5432\n");
        assert_eq!(
            l[0].addr,
            BindAddr::Specific("192.168.1.5".parse().unwrap())
        );
    }

    #[test]
    fn empty_lsof_output_is_free() {
        assert_eq!(
            analyze(5433, &[BindAddr::LoopbackV4], &parse_lsof("")),
            PortVerdict::Free
        );
    }

    #[test]
    fn docker_v6_wildcard_shadows_loopback_v4() {
        let verdict = analyze(5433, &LOCAL, &parse_lsof(DOCKER));
        assert!(
            matches!(verdict, PortVerdict::Shadowing { ref listeners } if listeners.len() == 1)
        );
    }

    #[test]
    fn same_loopback_is_conflict() {
        let verdict = analyze(
            5433,
            &[BindAddr::LoopbackV4],
            &[listener(BindAddr::LoopbackV4, 5433)],
        );
        assert!(
            matches!(verdict, PortVerdict::Conflict { ref same_addr, .. } if same_addr.len() == 1)
        );
    }

    #[test]
    fn wildcard_intent_vs_loopback_listener_shadows() {
        let verdict = analyze(
            5433,
            &[BindAddr::AnyV4],
            &[listener(BindAddr::LoopbackV4, 5433)],
        );
        assert!(matches!(verdict, PortVerdict::Shadowing { .. }));
    }

    #[test]
    fn v6_loopback_listener_ignored_for_v4_only_intent() {
        let verdict = analyze(
            5433,
            &[BindAddr::LoopbackV4],
            &[listener(BindAddr::LoopbackV6, 5433)],
        );
        assert_eq!(verdict, PortVerdict::Free);
    }

    #[test]
    fn other_port_ignored() {
        let verdict = analyze(5433, &LOCAL, &[listener(BindAddr::AnyV4, 5432)]);
        assert_eq!(verdict, PortVerdict::Free);
    }

    #[test]
    fn conflict_also_reports_shadowing_listeners() {
        let ls = [
            listener(BindAddr::LoopbackV4, 5433),
            listener(BindAddr::AnyV4, 5433),
        ];
        match analyze(5433, &[BindAddr::LoopbackV4], &ls) {
            PortVerdict::Conflict {
                same_addr,
                shadowing,
            } => {
                assert_eq!(same_addr.len(), 1);
                assert_eq!(shadowing.len(), 1);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }
}
