//! Listening-socket model, `lsof` parsing and port conflict analysis.

mod commands;
mod conflict;
mod jobs;
mod parse;

pub use commands::{short_command, tool};
pub use conflict::{PortVerdict, analyze};
pub use jobs::{PortJob, group_into_jobs};
pub use parse::{Proc, parse_lsof, parse_lsof_cwd, parse_ps};

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

    pub(crate) fn from_host(host: &str, ipv6: bool) -> Option<BindAddr> {
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

#[cfg(test)]
mod tests;
