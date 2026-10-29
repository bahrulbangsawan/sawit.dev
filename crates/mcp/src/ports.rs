//! Listening processes: dev servers on ports, and stopping them (the same
//! rules as `sawit bakar ports`).

use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::kebun;
use sawit_core::ports::PortJob;
use sawit_core::probe;

use crate::{Sawit, fail, ok, tilde, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListPortsArgs {
    #[schemars(
        description = "Every listening process (databases, Docker, system daemons too). Default false: dev servers only"
    )]
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StopArgs {
    #[schemars(
        description = "Stop whatever listens on these ports (Docker, protected tools and Sawit instances are skipped)"
    )]
    #[serde(default)]
    pub ports: Vec<u16>,
    #[schemars(description = "Stop these process IDs (taken as asked, without the skip rules)")]
    #[serde(default)]
    pub pids: Vec<u32>,
    #[schemars(
        description = "Send SIGKILL to processes still alive a second after SIGTERM. Default false"
    )]
    #[serde(default)]
    pub force: bool,
}

/// What stopping would do: the jobs to stop and the reasons others were left.
#[derive(Debug, Default)]
pub(crate) struct Plan<'a> {
    pub jobs: Vec<&'a PortJob>,
    pub skipped: Vec<String>,
}

/// Picks the jobs to stop. With no ports named, every dev server
/// (`kebun::burnable`); named ports skip Docker, protected tools and
/// Sawit instances.
pub(crate) fn plan<'a>(jobs: &'a [PortJob], ports: &[u16], instance_ports: &[u16]) -> Plan<'a> {
    let mut plan = Plan::default();
    if ports.is_empty() {
        plan.jobs
            .extend(jobs.iter().filter(|j| kebun::burnable(j, instance_ports)));
    }
    for &port in ports {
        let Some(job) = jobs.iter().find(|j| j.ports.iter().any(|(p, _)| *p == port)) else {
            plan.skipped.push(format!(":{port}: nothing listens there."));
            continue;
        };
        if job.is_docker() {
            plan.skipped
                .push(format!(":{port}: a Docker container; stop it with Docker."));
        } else if kebun::protected(job) {
            plan.skipped
                .push(format!(":{port}: {} is protected.", job.label));
        } else if instance_ports.contains(&port) {
            plan.skipped
                .push(format!(":{port}: a Sawit instance; use stop_instance."));
        } else if !plan.jobs.iter().any(|v| v.root == job.root) {
            plan.jobs.push(job);
        }
    }
    plan
}

fn ports_of(job: &PortJob) -> String {
    job.ports
        .iter()
        .map(|(p, _)| format!(":{p}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One process line for `list_ports`.
pub(crate) fn job_line(job: &PortJob, instance_ports: &[u16]) -> String {
    let mut tags = Vec::new();
    if job.ports.iter().any(|(p, _)| instance_ports.contains(p)) {
        tags.push("Sawit instance");
    }
    if job.is_docker() {
        tags.push("Docker");
    }
    if job.is_daemon() {
        tags.push("daemon");
    }
    if kebun::protected(job) {
        tags.push("protected");
    }
    if job.dev {
        tags.push("dev server");
    }
    let kids = job.pids.len().saturating_sub(1);
    let mut line = format!(
        "- {} · PID {}{} · {}{}",
        ports_of(job),
        job.root,
        if kids > 0 { format!(" (+{kids} children)") } else { String::new() },
        job.label,
        if tags.is_empty() { String::new() } else { format!(" [{}]", tags.join(", ")) }
    );
    if job.cmdline != job.label && !job.cmdline.is_empty() {
        line.push_str(&format!("\n    cmd: {}", clip_line(&job.cmdline, 300)));
    }
    if let Some(cwd) = &job.cwd {
        line.push_str(&format!("\n    cwd: {}", tilde(std::path::Path::new(cwd))));
    }
    line
}

fn clip_line(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

impl Sawit {
    fn instance_ports(&self) -> Vec<u16> {
        self.registry()
            .map(|r| r.instances.iter().map(|i| i.port).collect())
            .unwrap_or_default()
    }

    async fn list_ports_tool(&self, all: bool) -> CallToolResult {
        let jobs = tri!(probe::listening_jobs().await.map_err(|e| e.to_string()));
        let instance_ports = self.instance_ports();
        let shown: Vec<String> = jobs
            .iter()
            .filter(|j| all || kebun::burnable(j, &instance_ports))
            .map(|j| job_line(j, &instance_ports))
            .collect();
        if shown.is_empty() {
            return ok(if all {
                "Nothing listens on a TCP port."
            } else {
                "No dev servers are running. Pass all=true to see every listening process."
            });
        }
        ok(shown.join("\n"))
    }

    async fn stop_processes_tool(&self, a: StopArgs) -> CallToolResult {
        let jobs = tri!(probe::listening_jobs().await.map_err(|e| e.to_string()));
        let instance_ports = self.instance_ports();
        let only_pids = a.ports.is_empty() && !a.pids.is_empty();
        let plan = if only_pids {
            Plan::default()
        } else {
            plan(&jobs, &a.ports, &instance_ports)
        };
        let mut skipped = plan.skipped;
        let mut targets: Vec<(u32, String)> = Vec::new();
        for job in &plan.jobs {
            for &pid in &job.pids {
                targets.push((pid, format!("{} {}", ports_of(job), job.label)));
            }
        }
        let me = std::process::id();
        for &pid in &a.pids {
            if pid <= 1 || pid == me {
                skipped.push(format!("PID {pid}: refused."));
            } else if !probe::is_alive(pid) {
                skipped.push(format!("PID {pid}: no such process."));
            } else if !targets.iter().any(|(p, _)| *p == pid) {
                let label = jobs
                    .iter()
                    .find(|j| j.pids.contains(&pid))
                    .map(|j| format!("{} {}", ports_of(j), j.label))
                    .unwrap_or_else(|| format!("PID {pid}"));
                targets.push((pid, label));
            }
        }
        if targets.is_empty() {
            let mut out = vec!["Nothing to stop.".to_string()];
            out.extend(skipped);
            return ok(out.join("\n"));
        }

        let mut refused = Vec::new();
        for (pid, _) in &targets {
            if let Err(e) = probe::terminate(*pid, false) {
                refused.push(format!("PID {pid}: {e}"));
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut survivors: Vec<u32> = targets
            .iter()
            .map(|(p, _)| *p)
            .filter(|&p| probe::is_alive(p))
            .collect();
        let mut killed = false;
        if a.force && !survivors.is_empty() {
            for &pid in &survivors {
                let _ = probe::terminate(pid, true);
            }
            killed = true;
            tokio::time::sleep(Duration::from_millis(500)).await;
            survivors.retain(|&p| probe::is_alive(p));
        }

        let mut out = Vec::new();
        for (pid, label) in &targets {
            let state = if survivors.contains(pid) {
                "still running"
            } else {
                "stopped"
            };
            out.push(format!("- PID {pid} · {label}: {state}"));
        }
        if killed {
            out.push("SIGKILL was sent to processes that ignored SIGTERM.".into());
        } else if !survivors.is_empty() {
            out.push("Some processes ignored SIGTERM; call again with force=true to SIGKILL them.".into());
        }
        out.extend(refused);
        out.extend(skipped);
        if survivors.is_empty() {
            ok(out.join("\n"))
        } else {
            fail(out.join("\n"))
        }
    }
}

#[tool_router(router = ports_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List processes listening on TCP ports with PID, ports, command and working folder. Default: dev servers only (not databases, Docker, Sawit instances or system daemons); all=true lists everything"
    )]
    async fn list_ports(&self, Parameters(a): Parameters<ListPortsArgs>) -> CallToolResult {
        self.list_ports_tool(a.all).await
    }

    #[tool(
        description = "Stop processes: by port, by PID, or (with neither) every dev server. Sends SIGTERM, then SIGKILL to survivors when force=true. Ports held by Docker, protected tools (editors, agents, language servers) or Sawit instances are skipped; PIDs are stopped as given"
    )]
    async fn stop_processes(&self, Parameters(a): Parameters<StopArgs>) -> CallToolResult {
        self.stop_processes_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawit_core::ports::BindAddr;

    fn job(root: u32, port: u16, cmd: &str, dev: bool) -> PortJob {
        PortJob {
            root,
            pids: vec![root],
            label: cmd.into(),
            tool: cmd.split_whitespace().next().unwrap_or_default().into(),
            cmdline: cmd.into(),
            listeners: vec![(root, cmd.into())],
            ports: vec![(port, BindAddr::LoopbackV4)],
            cwd: None,
            dev,
        }
    }

    #[test]
    fn plan_follows_bakar_rules() {
        let jobs = vec![
            job(10, 3000, "node vite", true),
            job(11, 5432, "postgres -D x", false),
            job(12, 4000, "claude mcp-server", true),
            job(13, 5433, "node other", true),
        ];
        // Default: dev servers, not protected ones nor instance ports.
        let p = plan(&jobs, &[], &[5433]);
        let roots: Vec<u32> = p.jobs.iter().map(|j| j.root).collect();
        assert_eq!(roots, [10]);
        // Named ports.
        let p = plan(&jobs, &[4000, 5433, 9999, 5432], &[5433]);
        let roots: Vec<u32> = p.jobs.iter().map(|j| j.root).collect();
        assert_eq!(roots, [11]);
        assert_eq!(p.skipped.len(), 3);
        assert!(p.skipped[0].contains("protected"));
        assert!(p.skipped[1].contains("Sawit instance"));
        assert!(p.skipped[2].contains("nothing listens"));
    }
}
