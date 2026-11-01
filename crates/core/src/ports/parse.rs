//! Reading what `lsof` and `ps` print: listening sockets, processes and working folders.

use super::{BindAddr, Listener};

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

/// One process from `ps -axo pid=,ppid=,command=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    /// The command line (argv joined by spaces).
    pub args: String,
}

/// Parses `ps -axo pid=,ppid=,command=` output.
pub fn parse_ps(output: &str) -> Vec<Proc> {
    output
        .lines()
        .filter_map(|line| {
            // Columns are padded with runs of spaces.
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, args) = rest.trim_start().split_once(char::is_whitespace)?;
            Some(Proc {
                pid: pid.parse().ok()?,
                ppid: ppid.parse().ok()?,
                args: args.trim().to_string(),
            })
        })
        .collect()
}

/// Parses `lsof -a -d cwd -p … -F pn` output into (pid, directory).
pub fn parse_lsof_cwd(output: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut pid = None;
    for line in output.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let (Some(n), Some(p)) = (line.strip_prefix('n'), pid) {
            out.push((p, n.to_string()));
        }
    }
    out
}
