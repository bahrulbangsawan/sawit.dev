//! Tests for probes: port reports, the login shell, and finding tools on `PATH`.

use std::path::Path;

use super::shell::login_shell_path_of;
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

#[test]
fn scripts_run_in_the_login_shell_only_when_it_runs_posix_scripts() {
    for shell in [
        "/bin/zsh",
        "/bin/bash",
        "/bin/sh",
        "/opt/homebrew/bin/bash",
        "dash",
    ] {
        assert_eq!(script_shell(Some(shell)), Path::new(shell), "{shell}");
    }
    for shell in [
        "/opt/homebrew/bin/fish",
        "/usr/local/bin/nu",
        "/usr/local/bin/pwsh",
        "/bin/tcsh",
        "/bin/csh",
        "xonsh",
    ] {
        assert_eq!(script_shell(Some(shell)), Path::new("/bin/zsh"), "{shell}");
    }
    assert_eq!(script_shell(None), Path::new("/bin/zsh"));
}

/// tcsh (macOS ships it) refuses `-l -c`, like nu and PowerShell: the
/// app still gets a login PATH, from zsh.
#[tokio::test]
async fn a_login_shell_that_cant_answer_still_gives_a_path() {
    let path = login_shell_path_of(Some("/bin/tcsh")).await;
    assert!(
        path.is_some_and(|p| p.split(':').any(|d| d == "/usr/bin")),
        "no PATH"
    );
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
