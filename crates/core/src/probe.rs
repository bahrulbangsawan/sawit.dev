//! Runs the system tools (`lsof`, `ps`, `docker`, `brew`) to find out who is
//! listening on a port and explains the result.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::Result;
use crate::cmd;
use crate::owners::Owner;
use crate::owners::{OwnerContext, classify, parse_docker_ps};
use crate::ports::{BindAddr, Listener, PortVerdict, analyze, parse_lsof};

const TIMEOUT: Duration = Duration::from_secs(3);

/// Where the tools live. `Default` uses bare names resolved via `PATH`.
#[derive(Debug, Clone)]
pub struct ProbeEnv {
    pub lsof: PathBuf,
    pub ps: PathBuf,
    pub docker: Option<PathBuf>,
    pub brew_prefix: Option<PathBuf>,
}

impl Default for ProbeEnv {
    fn default() -> Self {
        ProbeEnv {
            lsof: "lsof".into(),
            ps: "ps".into(),
            docker: None,
            brew_prefix: None,
        }
    }
}

impl ProbeEnv {
    /// Detects `docker` and the Homebrew prefix once per process.
    pub async fn detect() -> ProbeEnv {
        static ENV: OnceCell<ProbeEnv> = OnceCell::const_new();
        ENV.get_or_init(|| async {
            ProbeEnv {
                docker: find_in_path("docker"),
                brew_prefix: brew_prefix().await,
                ..ProbeEnv::default()
            }
        })
        .await
        .clone()
    }
}

/// Finds an executable named `name` in the current `PATH`.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    find_in(&std::env::var_os("PATH")?, name)
}

pub(crate) fn find_in(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// `brew --prefix`, if Homebrew is installed.
pub async fn brew_prefix() -> Option<PathBuf> {
    let brew = find_in_path("brew")?;
    let out = cmd::run(brew, ["--prefix"], TIMEOUT).await.ok()?;
    let prefix = out.stdout.trim();
    (out.code == Some(0) && !prefix.is_empty()).then(|| PathBuf::from(prefix))
}

/// The `PATH` a login shell would have. GUI apps launched from Finder get a
/// minimal `PATH`, so the app adopts this at startup (as Zed does).
pub async fn login_shell_path() -> Option<String> {
    const MARK: &str = "__PGDESK_PATH__";
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let script = format!("printf '{MARK}%s' \"$PATH\"");
    let out = cmd::run(shell, ["-l", "-c", script.as_str()], Duration::from_secs(5))
        .await
        .ok()?;
    let path = out.stdout.rsplit_once(MARK)?.1.trim().to_string();
    (!path.is_empty()).then_some(path)
}

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
            Owner::PgDesk { instance } => {
                format!("PG Desk instance \"{instance}\" (PID {})", l.pid)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owners::Owner;
    use crate::ports::{BindAddr, Listener, PortVerdict};

    fn bound_port() -> (std::net::TcpListener, u16) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    #[tokio::test]
    async fn own_listener_detected_as_conflict() {
        let (_guard, port) = bound_port();
        let report = check_port(port, &[BindAddr::LoopbackV4], vec![])
            .await
            .unwrap();
        assert!(
            matches!(report.verdict, PortVerdict::Conflict { .. }),
            "{:?}",
            report.verdict
        );
        let own = report
            .owners
            .iter()
            .find(|o| o.listener.pid == std::process::id())
            .unwrap();
        assert_eq!(own.owner, Owner::Other);
    }

    #[tokio::test]
    async fn free_port_reports_free() {
        let port = bound_port().1; // listener dropped immediately
        let report = check_port(port, &[BindAddr::LoopbackV4], vec![])
            .await
            .unwrap();
        assert_eq!(report.verdict, PortVerdict::Free);
        assert!(report.owners.is_empty());
    }

    #[tokio::test]
    async fn missing_docker_cli_still_reports() {
        let (_guard, port) = bound_port();
        let env = ProbeEnv {
            docker: None,
            brew_prefix: None,
            ..ProbeEnv::default()
        };
        let report = check_port_with(&env, port, &[BindAddr::LoopbackV4], vec![])
            .await
            .unwrap();
        assert!(matches!(report.verdict, PortVerdict::Conflict { .. }));
    }

    #[test]
    fn explain_mentions_both_sides_for_shadowing() {
        let listener = Listener {
            pid: 14087,
            command: "com.docker.backend".into(),
            addr: BindAddr::AnyV6,
            port: 5433,
        };
        let report = PortReport {
            port: 5433,
            intended: vec![BindAddr::LoopbackV4, BindAddr::LoopbackV6],
            verdict: PortVerdict::Shadowing {
                listeners: vec![listener.clone()],
            },
            owners: vec![OwnedListener {
                listener,
                owner: Owner::Docker {
                    containers: vec!["rekamedika-postgres".into()],
                },
                command_line: String::new(),
            }],
        };
        let text = report.explain();
        assert!(text.contains("rekamedika-postgres"), "{text}");
        assert!(text.contains("127.0.0.1"), "{text}");
    }

    #[test]
    fn explain_free() {
        let report = PortReport {
            port: 5440,
            intended: vec![],
            verdict: PortVerdict::Free,
            owners: vec![],
        };
        assert!(report.explain().contains("5440"));
    }

    #[tokio::test]
    async fn login_shell_path_contains_usr_bin() {
        assert!(
            login_shell_path()
                .await
                .unwrap()
                .split(':')
                .any(|p| p == "/usr/bin")
        );
    }

    #[test]
    fn find_in_path_uses_given_path() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("mytool");
        std::fs::write(&tool, "").unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(find_in(&path, "mytool"), Some(tool));
        assert_eq!(find_in(&path, "missing"), None);
    }
}
