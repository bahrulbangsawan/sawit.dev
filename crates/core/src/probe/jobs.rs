//! Everything listening, grouped by the command that started it, and stopping processes.

use super::{ProbeEnv, TIMEOUT};
use crate::Result;
use crate::cmd;
use crate::ports::{PortJob, group_into_jobs, parse_lsof, parse_lsof_cwd, parse_ps};

/// Everything of this user listening on a TCP port (other users' sockets
/// aren't visible without root), grouped by the command that started it
/// (`npm run dev` with its children) and with its project folder.
pub async fn listening_jobs() -> Result<Vec<PortJob>> {
    let env = ProbeEnv::default();
    // lsof exits 1 with empty output when nothing matches; `+c 0` gives
    // full command names. `ps` runs alongside it: each takes tens of
    // milliseconds, and the Ports panel asks every few seconds.
    let (out, ps) = tokio::join!(
        cmd::run(
            &env.lsof,
            ["-nP", "+c", "0", "-iTCP", "-sTCP:LISTEN", "-F", "pctn"],
            TIMEOUT,
        ),
        cmd::run(&env.ps, ["-axo", "pid=,ppid=,command="], TIMEOUT),
    );
    let listeners = parse_lsof(&out?.stdout);
    if listeners.is_empty() {
        return Ok(Vec::new());
    }
    let procs = parse_ps(&ps?.stdout);
    // Never climb into Sawit itself or what started it.
    let mut exclude = vec![std::process::id()];
    while let Some(parent) = procs
        .iter()
        .find(|p| Some(&p.pid) == exclude.last())
        .map(|p| p.ppid)
        .filter(|pid| *pid > 1 && !exclude.contains(pid))
    {
        exclude.push(parent);
    }
    let mut jobs = group_into_jobs(&listeners, &procs, &exclude);
    let roots: Vec<String> = jobs.iter().map(|j| j.root.to_string()).collect();
    let roots = roots.join(",");
    if let Ok(out) = cmd::run(
        &env.lsof,
        ["-a", "-d", "cwd", "-p", roots.as_str(), "-F", "pn"],
        TIMEOUT,
    )
    .await
    {
        for (pid, dir) in parse_lsof_cwd(&out.stdout) {
            if let Some(j) = jobs.iter_mut().find(|j| j.root == pid) {
                j.cwd = Some(dir).filter(|d| d != "/");
            }
        }
    }
    Ok(jobs)
}

/// Sends SIGTERM (or SIGKILL when `force`) to `pid`. A process that is
/// already gone counts as success.
#[cfg(unix)]
pub fn terminate(pid: u32, force: bool) -> std::result::Result<(), String> {
    if pid <= 1 || pid == std::process::id() {
        return Err(format!(
            "Sawit won't stop process {pid} because it's a system process or Sawit itself."
        ));
    }
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: kill(2) takes plain integers and has no memory effects.
    if unsafe { libc::kill(pid as libc::pid_t, signal) } == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(()),
        Some(libc::EPERM) => Err(format!(
            "Unable to stop process {pid}. It belongs to another user."
        )),
        _ => Err(format!(
            "Unable to stop process {pid}. {}",
            std::io::Error::last_os_error()
        )),
    }
}

/// The process is still running.
#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    let exists = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    exists || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
