//! A quick reachability check: where `ssh` would connect, and whether an SSH server answers
//! there.

use std::time::{Duration, Instant};

use super::{SSH, SshHost, connection_args};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// The SSH server answered: its version banner and how long it took.
    Up {
        latency_ms: u64,
        banner: String,
    },
    /// Reached through a jump host or proxy command; not checked directly.
    Indirect,
    Down(String),
}

/// Asks `ssh -G` where `args` (connection arguments) really lead, then
/// opens a TCP connection there and reads the server's banner.
async fn probe_args(args: Vec<String>) -> Probe {
    let resolved = tokio::process::Command::new(SSH)
        .arg("-G")
        .args(&args)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    let output = match resolved {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => return Probe::Down(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => return Probe::Down(format!("Unable to run ssh. {e}")),
    };
    let (mut hostname, mut port) = (String::new(), 22u16);
    for line in output.lines() {
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match k {
            "hostname" => hostname = v.to_string(),
            "port" => port = v.parse().unwrap_or(22),
            "proxyjump" | "proxycommand" if v != "none" && !v.is_empty() => {
                return Probe::Indirect;
            }
            _ => {}
        }
    }
    probe_tcp(&hostname, port).await
}

pub(super) async fn probe_tcp(hostname: &str, port: u16) -> Probe {
    use tokio::io::AsyncReadExt;
    let started = Instant::now();
    let connect = tokio::net::TcpStream::connect((hostname, port));
    let mut stream = match tokio::time::timeout(Duration::from_secs(5), connect).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Probe::Down(friendly_io(&e, hostname, port)),
        Err(_) => return Probe::Down(format!("{hostname}:{port} didn't answer within 5 seconds.")),
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    let mut buf = [0u8; 256];
    let banner = match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf)).await {
        Ok(Ok(n)) => String::from_utf8_lossy(&buf[..n])
            .lines()
            .find(|l| l.starts_with("SSH-"))
            .map(|l| l.trim().to_string()),
        _ => None,
    };
    match banner {
        Some(banner) => Probe::Up { latency_ms, banner },
        None => Probe::Down(format!(
            "Port {port} is open on {hostname}, but no SSH server answered."
        )),
    }
}

fn friendly_io(e: &std::io::Error, hostname: &str, port: u16) -> String {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => {
            format!("{hostname} refused the connection on port {port}. Is sshd running?")
        }
        _ if e.to_string().contains("failed to lookup") || e.to_string().contains("nodename") => {
            format!("Unable to find {hostname}. Check the address.")
        }
        _ => format!("Unable to reach {hostname}:{port}. {e}"),
    }
}

pub async fn probe_host(host: SshHost) -> Probe {
    if let Some(problem) = host.problem() {
        return Probe::Down(problem);
    }
    probe_args(connection_args(&host)).await
}

pub async fn probe_alias(alias: String) -> Probe {
    probe_args(vec!["--".into(), alias]).await
}
