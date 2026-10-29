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

fn start(dir: &Path, program: &str, args: &[&str], port: u16) -> Child {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Runs `program args` in a project whose `dev` script starts a Node
/// server, and checks the job the panel would show.
async fn check(program: &str, args: &[&str], label: &str, tool: &str) {
    if find_in_path(program).is_none() || find_in_path("node").is_none() {
        eprintln!("skip {program}: not installed");
        return;
    }
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
    let port = free_port();
    let mut child = start(dir.path(), program, args, port);
    wait_until(&format!("{program} to listen"), || listening(port));

    let jobs = listening_jobs().await.unwrap();
    let job = jobs
        .iter()
        .find(|j| j.ports.iter().any(|(p, _)| *p == port))
        .unwrap_or_else(|| panic!("{program}: no job on port {port}"));
    assert_eq!(job.label, label, "{program}");
    assert_eq!(job.tool, tool, "{program}");
    assert!(job.dev, "{program}");
    assert_eq!(job.root, child.id(), "{program}: the launcher is the job");
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
        !listening(port) && job.pids.iter().all(|p| !is_alive(*p) || *p == child.id())
    });
    let _ = child.wait(); // reap the launcher (our child)
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
