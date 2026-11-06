//! Ending everything a terminal started, when its project closes (backlog item 12). Dropping a
//! session hangs its PTY up, which ends the shell and the jobs it still owns; a dev server that
//! started its own process group (`bun dev` and its workers, a server put in the background) can
//! outlive that, holding its port. So the whole tree under the shell is read first, while the
//! shell is still its parent, then each process gets SIGHUP, then SIGTERM, then SIGKILL once
//! [`Grace`] is over: nothing is left behind.

use std::time::{Duration, Instant};

/// How long each step waits for the processes to exit before the next, stronger signal.
#[derive(Clone, Copy, Debug)]
pub struct Grace {
    /// After SIGHUP, before SIGTERM.
    pub hangup: Duration,
    /// After SIGTERM, before SIGKILL.
    pub terminate: Duration,
}

impl Default for Grace {
    fn default() -> Self {
        Self {
            hangup: Duration::from_millis(800),
            terminate: Duration::from_millis(2500),
        }
    }
}

/// `roots` and every process under them, parents first, each once, as `children` lists a
/// process's children. Sawit itself (and pid 0 or 1) is never in it.
pub fn tree(roots: &[u32], children: impl Fn(u32) -> Vec<u32>) -> Vec<u32> {
    let me = std::process::id();
    let mut out: Vec<u32> = Vec::new();
    let mut queue: Vec<u32> = roots.to_vec();
    while let Some(pid) = queue.pop() {
        if pid <= 1 || pid == me || out.contains(&pid) {
            continue;
        }
        out.push(pid);
        queue.extend(children(pid));
    }
    out
}

/// The children of `pid`, as the OS lists them now.
#[cfg(target_os = "macos")]
pub fn children_of(pid: u32) -> Vec<u32> {
    let mut buf = vec![0 as libc::pid_t; 256];
    loop {
        let bytes = (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        // SAFETY: the buffer is `bytes` long; the call writes at most that many.
        let got =
            unsafe { libc::proc_listchildpids(pid as libc::pid_t, buf.as_mut_ptr().cast(), bytes) };
        if got <= 0 {
            return Vec::new();
        }
        // proc_listchildpids answers in pids on macOS (not bytes, as its sibling calls do).
        let n = got as usize;
        if n < buf.len() {
            return buf[..n]
                .iter()
                .filter(|p| **p > 0)
                .map(|p| *p as u32)
                .collect();
        }
        buf.resize(buf.len() * 2, 0);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn children_of(_pid: u32) -> Vec<u32> {
    Vec::new()
}

/// `roots` and everything under them now ([`tree`] over the OS's process list).
pub fn process_tree(roots: &[u32]) -> Vec<u32> {
    tree(roots, children_of)
}

#[cfg(unix)]
fn signal(pid: u32, sig: libc::c_int) {
    // SAFETY: kill(2) takes plain integers and has no memory effects.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

#[cfg(unix)]
fn alive(pids: &[u32]) -> Vec<u32> {
    pids.iter()
        .copied()
        .filter(|&pid| crate::probe::is_alive(pid) && !zombie(pid))
        .collect()
}

/// A process that has exited but whose parent hasn't reaped it yet: it holds nothing.
#[cfg(target_os = "macos")]
fn zombie(pid: u32) -> bool {
    // SAFETY: an all-zero proc_bsdinfo is valid plain data.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    got == size && info.pbi_status == libc::SZOMB
}

#[cfg(all(unix, not(target_os = "macos")))]
fn zombie(_pid: u32) -> bool {
    false
}

/// Waits up to `wait` for `pids` to exit; answers the ones still running.
#[cfg(unix)]
fn wait_for(pids: &[u32], wait: Duration) -> Vec<u32> {
    let until = Instant::now() + wait;
    loop {
        let left = alive(pids);
        if left.is_empty() || Instant::now() >= until {
            return left;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What [`stop`] did: how many processes there were, and how many each step ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stopped {
    pub processes: usize,
    /// Still running after SIGHUP, so sent SIGTERM.
    pub terminated: usize,
    /// Still running after SIGTERM, so sent SIGKILL.
    pub killed: usize,
}

/// Ends `pids` (read with [`process_tree`] before their terminal closed): SIGHUP, then SIGTERM
/// to those still running after `grace.hangup`, then SIGKILL after `grace.terminate`. Blocks
/// until done: run it off the UI thread.
#[cfg(unix)]
pub fn stop(pids: &[u32], grace: Grace) -> Stopped {
    let me = std::process::id();
    let pids: Vec<u32> = pids.iter().copied().filter(|&p| p > 1 && p != me).collect();
    for &pid in &pids {
        signal(pid, libc::SIGHUP);
    }
    let left = wait_for(&pids, grace.hangup);
    for &pid in &left {
        signal(pid, libc::SIGTERM);
    }
    let terminated = left.len();
    let left = wait_for(&left, grace.terminate);
    for &pid in &left {
        signal(pid, libc::SIGKILL);
    }
    Stopped {
        processes: pids.len(),
        terminated,
        killed: left.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_takes_every_descendant_once_and_never_sawit() {
        let me = std::process::id();
        let children = |pid: u32| match pid {
            10 => vec![11, 12],
            11 => vec![13],
            12 => vec![13, me, 1],
            _ => vec![],
        };
        let mut got = tree(&[10, 20], children);
        got.sort();
        assert_eq!(got, vec![10, 11, 12, 13, 20]);
    }

    /// A process that ignores SIGHUP and SIGTERM still ends, with SIGKILL, and its child too.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_stubborn_tree_ends() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "trap '' HUP TERM; sleep 30 & wait"])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let pids = process_tree(&[child.id()]);
        assert!(
            pids.len() >= 2,
            "the tree has the shell and its sleep: {pids:?}"
        );
        let grace = Grace {
            hangup: Duration::from_millis(200),
            terminate: Duration::from_millis(200),
        };
        // Reap the shell meanwhile, as its parent would.
        let waiter = std::thread::spawn(move || child.wait());
        let stopped = stop(&pids, grace);
        assert_eq!(stopped.processes, pids.len());
        assert!(stopped.killed >= 1, "{stopped:?}");
        waiter.join().unwrap().unwrap();
        // SIGKILL lands a moment after it is sent.
        let left = wait_for(&pids, Duration::from_secs(2));
        assert!(left.is_empty(), "left running: {left:?}");
    }
}
