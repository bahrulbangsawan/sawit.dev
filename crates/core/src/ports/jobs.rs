//! Listeners grouped into jobs: everything one command the user started listens on.

use super::commands::{
    DEV_DAEMONS, LAUNCHERS, is_launcher, named_command, program, short_command, tool,
};
use super::{BindAddr, Listener, Proc};

/// Everything listening for one command the user started (`bun run dev`
/// with its children), or one standalone process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortJob {
    /// The process the user started (the launcher), or the listener itself.
    pub root: u32,
    /// The root and all its descendants: what "stop" signals.
    pub pids: Vec<u32>,
    /// What to call it: `npm run dev`, `postgres -D …`.
    pub label: String,
    /// The tool that started it (`npm`, `bun`, `pnpm`, `python`, …).
    pub tool: String,
    /// The root's full command line.
    pub cmdline: String,
    /// The processes actually listening, as (pid, short command).
    pub listeners: Vec<(u32, String)>,
    /// Ports ascending, one bind each (the widest).
    pub ports: Vec<(u16, BindAddr)>,
    /// The root's working directory (the project folder), when known.
    pub cwd: Option<String>,
    /// Started from a dev tool or a typical local server (as opposed to
    /// apps such as AirPlay or Spotify that also listen).
    pub dev: bool,
}

impl PortJob {
    /// Docker Desktop / OrbStack publish container ports; stopping them
    /// stops Docker, so they are never picked by "select all".
    pub fn is_docker(&self) -> bool {
        self.listeners.iter().any(|(_, c)| {
            c.starts_with("com.docker") || c.starts_with("docker") || c.starts_with("OrbStack")
        })
    }

    /// A database or web server running on its own (`postgres`, `redis-server`),
    /// not a dev server someone started with a tool.
    pub fn is_daemon(&self) -> bool {
        self.listeners.iter().all(|(pid, _)| *pid == self.root)
            && DEV_DAEMONS.contains(&program(&self.cmdline))
    }
}

/// Groups `listeners` by the command that started them, climbing from each
/// listener through launchers (see [`LAUNCHERS`]) until the user's shell, a
/// terminal, launchd, `exclude` (Sawit itself and its ancestors) or
/// anything unknown (so an editor or terminal app is never part of a job).
pub fn group_into_jobs(listeners: &[Listener], procs: &[Proc], exclude: &[u32]) -> Vec<PortJob> {
    use std::collections::HashMap;
    let by_pid: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let args_of = |pid: u32, fallback: &str| {
        by_pid
            .get(&pid)
            .map(|p| p.args.clone())
            .unwrap_or_else(|| fallback.to_string())
    };
    let mut jobs: Vec<PortJob> = Vec::new();
    for l in listeners {
        let mut root = l.pid;
        while let Some(parent) = by_pid.get(&root).and_then(|p| by_pid.get(&p.ppid)) {
            if parent.pid <= 1 || exclude.contains(&parent.pid) || !is_launcher(&parent.args) {
                break;
            }
            root = parent.pid;
        }
        let job = match jobs.iter_mut().position(|j| j.root == root) {
            Some(ix) => &mut jobs[ix],
            None => {
                let cmdline = args_of(root, &l.command);
                let mut pids = vec![root];
                let mut ix = 0;
                while ix < pids.len() {
                    if let Some(kids) = children.get(&pids[ix]) {
                        pids.extend(kids.iter().filter(|k| !exclude.contains(k)));
                    }
                    ix += 1;
                }
                let launched = root != l.pid;
                let listener_args = args_of(l.pid, &l.command);
                // A standalone process is named the way lsof names it.
                let (label, tool_name) = match named_command(&cmdline, &l.command) {
                    Some(named) if !launched => (named, l.command.clone()),
                    _ => (short_command(&cmdline), tool(&cmdline).to_string()),
                };
                let docker = l.command.starts_with("com.docker")
                    || l.command.starts_with("docker")
                    || l.command.starts_with("OrbStack");
                let dev = launched
                    || docker
                    || LAUNCHERS.contains(&tool(&listener_args))
                    || DEV_DAEMONS.contains(&program(&listener_args));
                jobs.push(PortJob {
                    root,
                    pids,
                    label,
                    tool: tool_name,
                    cmdline,
                    listeners: vec![],
                    ports: vec![],
                    cwd: None,
                    dev,
                });
                jobs.last_mut().expect("pushed above")
            }
        };
        if !job.listeners.iter().any(|(pid, _)| *pid == l.pid) {
            let short = by_pid
                .get(&l.pid)
                .map(|p| {
                    named_command(&p.args, &l.command).unwrap_or_else(|| short_command(&p.args))
                })
                .unwrap_or_else(|| l.command.clone());
            job.listeners.push((l.pid, short));
        }
        match job.ports.iter_mut().find(|(port, _)| *port == l.port) {
            Some(entry) => {
                if l.addr.is_wildcard() && !entry.1.is_wildcard() {
                    entry.1 = l.addr;
                }
            }
            None => job.ports.push((l.port, l.addr)),
        }
    }
    for j in &mut jobs {
        j.ports.sort_by_key(|(port, _)| *port);
    }
    // Dev jobs first, then by lowest port.
    jobs.sort_by_key(|j| (!j.dev, j.ports.first().map(|(port, _)| *port), j.root));
    jobs
}
