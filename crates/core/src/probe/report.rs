//! Who listens on a port a server is to bind, and what that means for the server.

use std::path::{Path, PathBuf};

use super::{ProbeEnv, TIMEOUT};
use crate::Result;
use crate::cmd;
use crate::owners::{Owner, OwnerContext, classify, parse_docker_ps};
use crate::ports::{BindAddr, Listener, PortVerdict, analyze, parse_lsof};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedListener {
    pub listener: Listener,
    pub owner: Owner,
    pub command_line: String,
}

impl OwnedListener {
    pub fn describe(&self) -> String {
        let l = &self.listener;
        match &self.owner {
            Owner::Docker { containers } if !containers.is_empty() => {
                format!("Docker (container {})", containers.join(", "))
            }
            Owner::Docker { .. } => format!("Docker ({}, PID {})", l.command, l.pid),
            Owner::Homebrew { formula: Some(f) } => format!("Homebrew {f} (PID {})", l.pid),
            Owner::Homebrew { formula: None } => format!("Homebrew {} (PID {})", l.command, l.pid),
            Owner::PostgresApp => format!("Postgres.app (PID {})", l.pid),
            Owner::Sawit { instance } => {
                format!("Sawit instance \"{instance}\" (PID {})", l.pid)
            }
            Owner::Other => format!("{} (PID {})", l.command, l.pid),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortReport {
    pub port: u16,
    pub intended: Vec<BindAddr>,
    pub verdict: PortVerdict,
    pub owners: Vec<OwnedListener>,
}

impl PortReport {
    fn describe(&self, l: &Listener) -> String {
        self.owners
            .iter()
            .find(|o| &o.listener == l)
            .map(OwnedListener::describe)
            .unwrap_or_else(|| format!("{} (PID {})", l.command, l.pid))
    }

    /// A human explanation of the verdict and who is involved.
    pub fn explain(&self) -> String {
        let port = self.port;
        let ours = join(self.intended.iter().filter(|a| !a.is_wildcard()));
        let shadow_line = |l: &Listener| {
            let who = self.describe(l);
            if l.addr.is_wildcard() && !ours.is_empty() {
                format!(
                    "{who} listens on {}:{port}. Connections to {ours}:{port} reach this instance, \
                     but connections through other addresses (LAN IP, container, host.docker.internal) reach {who}.",
                    l.addr
                )
            } else {
                format!(
                    "{who} listens on {}:{port}. Connections to {}:{port} reach {who}, not this instance.",
                    l.addr, l.addr
                )
            }
        };
        match &self.verdict {
            PortVerdict::Free => format!("Port {port} is free."),
            PortVerdict::Conflict {
                same_addr,
                shadowing,
            } => {
                let mut lines = vec![format!(
                    "Port {port} is already in use on the same address by {}. Start will fail; choose another port or stop that process.",
                    same_addr
                        .iter()
                        .map(|l| format!("{} on {}", self.describe(l), l.addr))
                        .collect::<Vec<_>>()
                        .join(", ")
                )];
                lines.extend(shadowing.iter().map(shadow_line));
                lines.join("\n")
            }
            PortVerdict::Shadowing { listeners } => {
                let mut lines = vec![format!(
                    "Another process also listens on port {port}. The server can start, but clients may reach the wrong database depending on the address they use:"
                )];
                lines.extend(listeners.iter().map(shadow_line));
                lines.join("\n")
            }
        }
    }
}

fn join<'a>(addrs: impl Iterator<Item = &'a BindAddr>) -> String {
    addrs.map(ToString::to_string).collect::<Vec<_>>().join("/")
}

/// Checks `port` against the addresses we intend to bind, using detected tools.
pub async fn check_port(
    port: u16,
    intended: &[BindAddr],
    managed: Vec<(String, PathBuf)>,
) -> Result<PortReport> {
    check_port_with(&ProbeEnv::detect().await, port, intended, managed).await
}

pub async fn check_port_with(
    env: &ProbeEnv,
    port: u16,
    intended: &[BindAddr],
    managed: Vec<(String, PathBuf)>,
) -> Result<PortReport> {
    let filter = format!("-iTCP:{port}");
    // lsof exits 1 with empty output when nothing matches.
    let out = cmd::run(
        &env.lsof,
        ["-nP", filter.as_str(), "-sTCP:LISTEN", "-F", "pctn"],
        TIMEOUT,
    )
    .await?;
    let listeners = parse_lsof(&out.stdout);
    let verdict = analyze(port, intended, &listeners);

    let mut ctx = OwnerContext {
        brew_prefix: env.brew_prefix.clone(),
        docker_ports: vec![],
        managed,
    };
    let looks_docker = |l: &Listener| classify(l, "", &OwnerContext::default()) != Owner::Other;
    if let Some(docker) = &env.docker
        && listeners.iter().any(looks_docker)
        && let Ok(out) = cmd::run(
            docker,
            ["ps", "--format", "{{.Names}}\t{{.Ports}}"],
            TIMEOUT,
        )
        .await
    {
        ctx.docker_ports = parse_docker_ps(&out.stdout);
    }

    let mut owners = Vec::with_capacity(listeners.len());
    for listener in listeners.into_iter().filter(|l| l.port == port) {
        let command_line = command_line(&env.ps, listener.pid).await;
        let owner = classify(&listener, &command_line, &ctx);
        owners.push(OwnedListener {
            listener,
            owner,
            command_line,
        });
    }
    Ok(PortReport {
        port,
        intended: intended.to_vec(),
        verdict,
        owners,
    })
}

async fn command_line(ps: &Path, pid: u32) -> String {
    let pid = pid.to_string();
    match cmd::run(ps, ["-o", "command=", "-p", pid.as_str()], TIMEOUT).await {
        Ok(out) => out.stdout.trim().to_string(),
        Err(_) => String::new(),
    }
}
