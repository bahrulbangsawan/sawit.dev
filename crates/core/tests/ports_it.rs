//! The Ports panel's view of real dev servers: each package manager's
//! `dev` script is started for real, must show up as one job labelled with
//! the command that was run, and stopping that job must free the port
//! (launcher included, so nothing respawns). Tools that aren't installed
//! are skipped.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sawit_core::probe::{find_in_path, is_alive, listening_jobs, terminate};

const SERVER: &str = "require('http').createServer((q, r) => r.end('ok')).listen(Number(process.env.PORT), '127.0.0.1');\n";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ok() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A dev server's launcher. Dropping it, as the panic of a failed check does, stops it and
/// everything it started, server included, then reaps it: a [`Child`] doesn't stop on drop, and
/// a failed check used to leave its server running for ever, in a folder that was gone
/// (SMOKE-4).
///
/// The launcher stays in the test's process group, so that what stops the whole test run (⌃C in
/// a terminal, a tool's timeout) stops the server too.
struct Started(Child);

impl Started {
    fn id(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        // The launcher first, so that it can't start its server again. Until it is reaped below,
        // no other process can have its pid: what `ps` lists under it is its own.
        for pid in tree(self.0.id()) {
            let _ = terminate(pid, true);
        }
        let _ = self.0.wait();
    }
}

/// `root` and every process under it, parents before their children, as `ps` lists them now.
fn tree(root: u32) -> Vec<u32> {
    let listed = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    let parents: Vec<(u32, u32)> = listed
        .lines()
        .filter_map(|line| {
            let mut ids = line.split_whitespace().map(str::parse);
            Some((ids.next()?.ok()?, ids.next()?.ok()?))
        })
        .collect();
    let mut found = vec![root];
    let mut next = 0;
    while let Some(&parent) = found.get(next) {
        let children = parents.iter().filter(|(_, ppid)| *ppid == parent);
        found.extend(children.map(|(pid, _)| *pid));
        next += 1;
    }
    found
}

fn start(dir: &Path, program: &str, args: &[&str], port: u16) -> Started {
    let child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    Started(child)
}

/// A project whose `dev` script, for every package manager, starts a Node server on `$PORT`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("server.js"), SERVER).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{ "name": "sawit-ports-it", "private": true, "scripts": { "dev": "node server.js" } }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("deno.json"),
        r#"{ "tasks": { "dev": "node server.js" } }"#,
    )
    .unwrap();
    dir
}

/// Runs `program args` in a project whose `dev` script starts a Node
/// server, and checks the job the panel would show.
async fn check(program: &str, args: &[&str], label: &str, tool: &str) {
    if find_in_path(program).is_none() || find_in_path("node").is_none() {
        eprintln!("skip {program}: not installed");
        return;
    }
    let dir = project();
    let port = free_port();
    let server = start(dir.path(), program, args, port);
    wait_until(&format!("{program} to listen"), || listening(port));

    let jobs = listening_jobs().await.unwrap();
    let job = jobs
        .iter()
        .find(|j| j.ports.iter().any(|(p, _)| *p == port))
        .unwrap_or_else(|| panic!("{program}: no job on port {port}"));
    assert_eq!(job.label, label, "{program}");
    assert_eq!(job.tool, tool, "{program}");
    assert!(job.dev, "{program}");
    assert_eq!(job.root, server.id(), "{program}: the launcher is the job");
    assert!(
        job.pids.len() >= 2,
        "{program}: launcher and server: {:?}",
        job.pids
    );
    let cwd = std::fs::canonicalize(dir.path()).unwrap();
    assert_eq!(job.cwd.as_deref(), cwd.to_str(), "{program}");

    for pid in &job.pids {
        terminate(*pid, false).unwrap();
    }
    wait_until(&format!("{program} to stop"), || {
        !listening(port) && job.pids.iter().all(|p| !is_alive(*p) || *p == server.id())
    });
    // Dropping `server` reaps the launcher (our child).
}

#[tokio::test]
async fn npm_run_dev() {
    check("npm", &["run", "dev"], "npm run dev", "npm").await;
}

#[tokio::test]
async fn pnpm_dev() {
    check("pnpm", &["dev"], "pnpm dev", "pnpm").await;
}

#[tokio::test]
async fn bun_run_dev() {
    check("bun", &["run", "dev"], "bun run dev", "bun").await;
}

#[tokio::test]
async fn yarn_dev() {
    check("yarn", &["dev"], "yarn dev", "yarn").await;
}

#[tokio::test]
async fn deno_task_dev() {
    check("deno", &["task", "dev"], "deno task dev", "deno").await;
}

/// SMOKE-4: a check that fails part-way stops what it started, launcher and server alike. A
/// failed check used to leave its dev server running for ever, in a folder that was gone.
#[test]
fn a_failed_check_stops_its_dev_server() {
    if find_in_path("node").is_none() {
        eprintln!("skip: node is not installed");
        return;
    }
    let dir = project();
    let port = free_port();
    let failed = std::panic::catch_unwind(|| {
        // A launcher that runs as long as its server, as `npm run dev` does.
        let _server = start(dir.path(), "sh", &["-c", "node server.js & wait"], port);
        wait_until("the server to listen", || listening(port));
        panic!("the check fails");
    });
    assert!(failed.is_err());
    wait_until(
        &format!("the failed check's server on port {port} to stop"),
        || !listening(port),
    );
}
