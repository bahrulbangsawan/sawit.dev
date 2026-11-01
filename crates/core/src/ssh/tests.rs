//! Tests for SSH: the command line for a server, config hosts, the saved list, and the
//! reachability check.

use std::path::Path;

use super::probe::probe_tcp;
use super::*;

fn host() -> SshHost {
    let mut h = SshHost::new("web", "203.0.113.7");
    h.id = "abc".into();
    h
}

#[test]
fn plain_host_is_just_the_destination_after_the_options() {
    let l = launch(&host(), None);
    assert_eq!(l.program, "ssh");
    assert_eq!(l.args.last().unwrap(), "203.0.113.7");
    assert!(l.args.contains(&"ControlMaster=auto".to_string()));
    assert!(!l.args.contains(&"-t".to_string()));
    assert!(l.env.is_empty());
}

#[test]
fn user_port_key_and_jump_become_flags() {
    let mut h = host();
    h.user = "deploy".into();
    h.port = Some(2222);
    h.auth = Auth::Key;
    h.identity_file = "/k/id_ed25519".into();
    h.jump = "me@bastion".into();
    let a = launch(&h, None).args;
    let pos = |s: &str| a.iter().position(|x| x == s).unwrap();
    assert_eq!(a[pos("-p") + 1], "2222");
    assert_eq!(a[pos("-l") + 1], "deploy");
    assert_eq!(a[pos("-i") + 1], "/k/id_ed25519");
    assert!(a.contains(&"IdentitiesOnly=yes".to_string()));
    assert_eq!(a[pos("-J") + 1], "me@bastion");
}

#[test]
fn key_file_is_ignored_unless_key_auth_is_chosen() {
    let mut h = host();
    h.identity_file = "/k/id".into();
    assert!(!launch(&h, None).args.contains(&"-i".to_string()));
}

#[test]
fn remote_dir_and_startup_run_before_a_login_shell() {
    let mut h = host();
    h.remote_dir = "~/apps/my app".into();
    h.startup = "tmux new -A -s main".into();
    let a = launch(&h, None).args;
    assert!(a.contains(&"-t".to_string()));
    assert_eq!(
        a.last().unwrap(),
        "cd ~/'apps/my app'; tmux new -A -s main; exec \"$SHELL\" -l"
    );
    assert_eq!(a[a.len() - 2], "203.0.113.7");
}

#[test]
fn forwards_accept_a_bare_port() {
    assert_eq!(
        parse_forward("5432").as_deref(),
        Some("5432:localhost:5432")
    );
    assert_eq!(
        parse_forward("8080:localhost:80").as_deref(),
        Some("8080:localhost:80")
    );
    assert_eq!(parse_forward("80:x"), None);
    assert_eq!(parse_forward("abc"), None);
    let mut h = host();
    h.forwards = vec!["5432".into()];
    let a = launch(&h, None).args;
    let i = a.iter().position(|x| x == "-L").unwrap();
    assert_eq!(a[i + 1], "5432:localhost:5432");
}

#[test]
fn saved_password_uses_the_askpass_helper() {
    let mut h = host();
    h.auth = Auth::Password;
    let l = launch(&h, Some(Path::new("/A/sawit")));
    assert!(l.env.contains(&("SSH_ASKPASS".into(), "/A/sawit".into())));
    assert!(
        l.env
            .contains(&("SSH_ASKPASS_REQUIRE".into(), "force".into()))
    );
    assert!(l.env.contains(&(ASKPASS_ENV.into(), "abc".into())));
    // Without a helper (no saved password) ssh asks in the terminal.
    assert!(launch(&h, None).env.is_empty());
}

#[test]
fn options_cannot_be_smuggled_through_the_address() {
    let mut h = host();
    h.host = "-oProxyCommand=evil".into();
    assert!(h.problem().is_some());
    h.host = "ok".into();
    h.user = "-x".into();
    assert!(h.problem().is_some());
    assert!(
        launch_alias("-oX")
            .args
            .ends_with(&["--".into(), "-oX".into()])
    );
}

#[test]
fn command_line_is_shell_quoted() {
    let mut h = host();
    h.startup = "echo hi".into();
    assert!(
        launch(&h, None)
            .command_line()
            .ends_with("'echo hi; exec \"$SHELL\" -l'")
    );
}

/// The shared connection holds a server's forwarded ports, so for a server with forwards it
/// goes seconds after the last session closes, not ten minutes later: until then another
/// program (a Sawit instance on 5432) can't take those ports.
#[test]
fn a_server_with_forwards_lets_go_of_its_ports_soon_after_its_last_session() {
    let persist = |launch: &Launch| {
        launch
            .args
            .iter()
            .find(|a| a.starts_with("ControlPersist="))
            .cloned()
    };
    let mut h = host();
    assert_eq!(
        persist(&launch(&h, None)).as_deref(),
        Some("ControlPersist=10m")
    );
    h.forwards = vec!["5432".into()];
    assert_eq!(
        persist(&launch(&h, None)).as_deref(),
        Some("ControlPersist=5s")
    );
    assert!(
        launch(&h, None)
            .args
            .contains(&"5432:localhost:5432".to_string())
    );
    // A forward that doesn't parse isn't passed, so it holds nothing.
    h.forwards = vec!["not a port".into()];
    assert_eq!(
        persist(&launch(&h, None)).as_deref(),
        Some("ControlPersist=10m")
    );
}

#[test]
fn plain_command_leaves_out_sawit_options() {
    let mut h = host();
    h.user = "root".into();
    h.port = Some(2222);
    assert_eq!(plain_command(&h), "ssh -p 2222 -l root 203.0.113.7");
}

#[test]
fn password_prompts_are_told_apart() {
    assert!(is_password_prompt("root@203.0.113.7's password: "));
    assert!(is_password_prompt("(deploy@web) Password:"));
    assert!(!is_password_prompt("Enter passphrase for key '/k/id': "));
    assert!(!is_password_prompt("Verification code: "));
    assert!(!is_password_prompt(
        "Are you sure you want to continue connecting (yes/no/[fingerprint])? "
    ));
}

fn parse(text: &str, files: &[(&str, &str)]) -> Vec<ConfigHost> {
    let mut out = Vec::new();
    let mut read = |p: &Path| {
        files
            .iter()
            .find(|(name, _)| p == Path::new("/home/.ssh").join(name))
            .map(|(_, t)| t.to_string())
    };
    parse_config(text, Path::new("/home/.ssh"), &mut read, 0, &mut out);
    out
}

#[test]
fn config_hosts_skip_patterns_and_keep_the_first_value() {
    let hosts = parse(
        "Host *\n  User everyone\n\nHost web prod-web\n  HostName 203.0.113.7\n  User deploy\n  Port 2222\n  User ignored\n\nHost db\n  Hostname=10.0.0.5\n  ProxyJump web\nHost *.internal !x\n",
        &[],
    );
    assert_eq!(hosts.len(), 3);
    assert_eq!(hosts[0].alias, "web");
    assert_eq!(hosts[0].address(), "deploy@203.0.113.7:2222");
    assert_eq!(hosts[1].alias, "prod-web");
    assert_eq!(hosts[1].user.as_deref(), Some("deploy"));
    assert_eq!(hosts[2].hostname.as_deref(), Some("10.0.0.5"));
    assert_eq!(hosts[2].jump.as_deref(), Some("web"));
}

#[test]
fn config_includes_are_followed() {
    let hosts = parse(
        "Include extra\nHost a\n",
        &[("extra", "Host orb\n  HostName 127.0.0.1\n  Port 32222\n")],
    );
    assert_eq!(
        hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(),
        ["orb", "a"]
    );
    assert_eq!(hosts[0].port, Some(32222));
}

#[test]
fn match_blocks_do_not_leak_into_the_previous_host() {
    let hosts = parse("Host a\nMatch host b\n  User nope\n", &[]);
    assert_eq!(hosts[0].user, None);
}

#[test]
fn search_groups_then_sorts_by_name() {
    let mut list = HostList::default();
    for (name, group) in [("zeta", ""), ("alpha", "prod"), ("beta", "")] {
        let mut h = SshHost::new(name, "h");
        h.group = group.into();
        list.add(h, 0);
    }
    let names: Vec<_> = list.search("").iter().map(|h| h.name.clone()).collect();
    assert_eq!(names, ["beta", "zeta", "alpha"]);
    assert_eq!(list.search("PROD").len(), 1);
}

#[test]
fn host_list_round_trips_and_tolerates_missing_fields() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ssh_hosts.json");
    assert_eq!(HostList::load(&file).unwrap(), HostList::default());
    let mut list = HostList::default();
    list.add(host(), 5);
    list.save(&file).unwrap();
    assert_eq!(HostList::load(&file).unwrap(), list);
    let old = r#"{"hosts":[{"id":"x","name":"n","host":"h"}]}"#;
    std::fs::write(&file, old).unwrap();
    let loaded = HostList::load(&file).unwrap();
    assert_eq!(loaded.hosts[0].auth, Auth::Default);
    std::fs::write(&file, "{oops").unwrap();
    assert!(HostList::load(&file).is_err());
}

#[tokio::test]
async fn probe_reads_the_ssh_banner() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let (mut s, _) = listener.accept().await.unwrap();
        s.write_all(b"SSH-2.0-OpenSSH_9.6\r\n").await.unwrap();
    });
    match probe_tcp("127.0.0.1", port).await {
        Probe::Up { banner, .. } => assert_eq!(banner, "SSH-2.0-OpenSSH_9.6"),
        other => panic!("{other:?}"),
    }
}
