//! Saved SSH hosts are found by id or unique name and kept until the agent names them again;
//! commands never prompt, and the password helper answers password prompts only.

use rmcp::model::CallToolResult;
use sawit_core::ssh::Auth;

use super::exec::exec_options;
use super::hosts::{names, parse_auth};
use super::*;

#[test]
fn finds_hosts_by_id_or_unique_name() {
    let mut list = HostList::default();
    let a = list.add(SshHost::new("web", "1.2.3.4"), 1);
    list.add(SshHost::new("db", "1.2.3.5"), 1);
    list.add(SshHost::new("db", "1.2.3.6"), 1);
    assert_eq!(find_saved(&list, &a).unwrap().unwrap().name, "web");
    assert_eq!(find_saved(&list, "web").unwrap().unwrap().host, "1.2.3.4");
    assert!(find_saved(&list, "nope").unwrap().is_none());
    assert!(find_saved(&list, "db").is_err());
}

#[test]
fn exec_never_prompts_for_key_hosts() {
    let opts = exec_options(false);
    assert!(opts.contains(&"BatchMode=yes".to_string()));
    assert!(opts.contains(&"ControlMaster=no".to_string()));
    let opts = exec_options(true);
    assert!(!opts.contains(&"BatchMode=yes".to_string()));
    assert!(opts.contains(&"NumberOfPasswordPrompts=1".to_string()));
}

#[test]
fn auth_names() {
    assert_eq!(parse_auth("Password").unwrap(), Auth::Password);
    assert_eq!(parse_auth("key").unwrap(), Auth::Key);
    assert!(parse_auth("otp").is_err());
}

#[test]
fn a_host_is_named_again_by_its_id_or_its_name() {
    let mut host = SshHost::new("web", "1.2.3.4");
    host.id = "h1".into();
    assert!(names(&host, "h1"));
    assert!(names(&host, "web"));
    assert!(!names(&host, "Web"));
    assert!(!names(&host, "1.2.3.4"));
    assert!(!names(&host, ""));
}

/// The saved host and its password stay unless the agent names the host
/// again: removing it, or moving it off password sign-in, which deletes
/// the password. A refusal returns before the Keychain is touched.
#[tokio::test]
async fn the_keychain_password_is_kept_until_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let sawit = Sawit::new(sawit_core::instances::AppPaths {
        root: dir.path().to_path_buf(),
    });
    let mut host = SshHost::new("web", "1.2.3.4");
    host.auth = Auth::Password;
    let mut list = HostList::default();
    let id = list.add(host, 1);
    list.save(&sawit.ssh_file()).unwrap();
    let said = |r: &CallToolResult| {
        let texts = r.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<String>()
    };
    let update = |confirm: Option<&str>| UpdateHostArgs {
        id: "web".into(),
        name: None,
        host: None,
        user: None,
        port: None,
        auth: None,
        identity_file: Some("~/.ssh/id_ed25519".into()),
        password: None,
        forwards: None,
        jump: None,
        group: None,
        remote_dir: None,
        startup: None,
        confirm: confirm.map(String::from),
    };
    for confirm in [None, Some("other")] {
        let r = sawit.update_ssh_host_tool(update(confirm)).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("Keychain password"), "{}", said(&r));
    }
    let r = sawit.remove_ssh_host_tool("web", "").await;
    assert_eq!(r.is_error, Some(true));
    assert!(said(&r).contains("confirm=\"web\""), "{}", said(&r));
    let kept = sawit.ssh_hosts().unwrap();
    let kept = kept.get(&id).unwrap();
    assert_eq!(kept.auth, Auth::Password);
    assert!(kept.identity_file.is_empty());
}

#[test]
fn askpass_ignores_other_prompts() {
    assert_eq!(askpass("nope", "Enter passphrase for key:"), 1);
    assert_eq!(askpass("nope", "Are you sure (yes/no)?"), 1);
}
