//! Tests for the catalog: its ids and specs, GitHub's sign-in, the AWS Region, the package
//! manager picked, and the report of which clients have each server.

use super::*;

/// The list reads each client's config once and says, per server, which
/// clients have it and how; a server no client has lists none.
#[test]
fn the_report_names_the_clients_that_have_each_server() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    std::fs::create_dir_all(h.join(".codex")).unwrap();
    std::fs::write(
        h.join(".codex/config.toml"),
        "[mcp_servers.aws]\nurl = \"https://aws-mcp.eu-west-1.api.aws/mcp\"\n",
    )
    .unwrap();
    std::fs::write(
        h.join(".claude.json"),
        r#"{"mcpServers": {"sawit": {"command": "/Apps/Sawit.app/sawit-mcp"}}}"#,
    )
    .unwrap();
    let targets = mcp_clients::discover(h, None);
    let none = |_: &str| None;
    let report = report_for(targets, Path::new("/Apps/Sawit.app/sawit-mcp"), none);
    let clients = |id: &str| -> Vec<(String, Status)> {
        let listed = report.servers.iter().find(|l| l.server.id == id).unwrap();
        listed
            .installed
            .iter()
            .map(|(ix, status)| {
                (
                    report.targets[*ix].client.name().to_string(),
                    status.clone(),
                )
            })
            .collect()
    };
    assert_eq!(report.servers.len(), SERVERS.len());
    assert_eq!(
        clients("sawit"),
        [("Claude Code".to_string(), Status::Installed)]
    );
    let aws = clients("aws");
    assert_eq!(aws.len(), 1);
    assert_eq!(aws[0].0, "Codex");
    assert!(
        matches!(&aws[0].1, Status::Outdated(sig) if get("aws").unwrap().region_in(sig).as_deref() == Some("eu-west-1")),
        "{aws:?}"
    );
    assert!(clients("neon").is_empty());
    // A local server's program that isn't installed is named.
    let cap = report
        .servers
        .iter()
        .find(|l| l.server.id == "cap")
        .unwrap();
    assert_eq!(cap.missing_program, Some("cap"));
}

/// GitHub's server is GitHub's own (its documented name and URL), takes Sawit's sign-in, and
/// is handed it by the `sawit` beside the `sawit-mcp` the page installs: the spec says which
/// clients reach it through the bridge as the row does.
#[test]
fn github_takes_sawits_sign_in_from_the_sawit_beside_the_server() {
    let github = get(GITHUB_ID).unwrap();
    assert_eq!(github.name, "GitHub");
    assert_eq!(github.auth, Auth::GitHub);
    assert_eq!(
        github.address(DEFAULT_AWS_REGION),
        "https://api.githubcopilot.com/mcp/"
    );
    assert_eq!(github.logo, Some(("github", false)));
    let spec = github.spec(
        Path::new("/Applications/Sawit.app/Contents/MacOS/sawit-mcp"),
        DEFAULT_AWS_REGION,
        |_| None,
    );
    assert_eq!(
        spec.token_helper.as_deref(),
        Some("/Applications/Sawit.app/Contents/MacOS/sawit")
    );
    for client in mcp_clients::Client::ALL {
        assert_eq!(
            github.bridged_for(client),
            mcp_clients::bridged(client, &spec),
            "{client:?}"
        );
    }
    // Only GitHub's: no other server is handed Sawit's sign-in.
    for s in SERVERS.iter().filter(|s| s.id != GITHUB_ID) {
        let spec = s.spec(Path::new("/x/sawit-mcp"), DEFAULT_AWS_REGION, |_| None);
        assert_eq!(spec.token_helper, None, "{}", s.id);
        for client in mcp_clients::Client::ALL {
            assert_eq!(s.bridged_for(client), mcp_clients::bridged(client, &spec));
        }
    }
    // The scopes it needs are ones the one sign-in asks for, so Sign In Again grants them.
    let asked: Vec<&str> = crate::github::SCOPES.iter().map(|(s, _)| *s).collect();
    assert!(GITHUB_SCOPES.iter().all(|s| asked.contains(s)));
}

#[test]
fn ids_are_unique_and_specs_build() {
    let mut ids: Vec<_> = SERVERS.iter().map(|s| s.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), SERVERS.len());
    let none = |_: &str| None;
    for s in SERVERS {
        let spec = s.spec(Path::new("/bin/sawit-mcp"), "eu-west-1", none);
        assert_eq!(spec.name, s.id);
        assert_eq!(spec.is_remote(), s.is_remote());
    }
}

/// SMOKE-6: the built-in server's row is what a user reads before installing it into every
/// client at once, so it says what an agent can do with it: write to databases, run terminals,
/// commit, drive the canvas and the browser. "Read-only queries" was never true (`execute_sql`).
#[test]
fn the_built_in_server_says_what_agents_can_do_with_it() {
    let about = get(crate::mcp_clients::SERVER_NAME).unwrap().about;
    assert!(!about.contains("read-only"), "{about}");
    for what in [
        "databases (read and write)",
        "terminals",
        "git",
        "canvas",
        "browser",
    ] {
        assert!(about.contains(what), "{what}: {about}");
    }
}

#[test]
fn regions_are_checked_and_read_back() {
    for ok in [
        "us-east-1",
        "eu-west-1",
        "ap-southeast-2",
        "us-gov-west-1",
        "cn-north-1",
    ] {
        assert!(valid_region(ok), "{ok}");
    }
    for bad in [
        "",
        "eu west 1",
        "x.example.com/#",
        "us-east",
        "useast-1",
        "US-EAST-1",
        "u-east-1",
        "us--1",
    ] {
        assert!(!valid_region(bad), "{bad}");
    }
    let aws = get("aws").unwrap();
    assert_eq!(
        aws.region_in("https://aws-mcp.eu-west-1.api.aws/mcp")
            .as_deref(),
        Some("eu-west-1")
    );
    // Through the mcp-remote bridge.
    assert_eq!(
        aws.region_in("/opt/homebrew/bin/npx -y mcp-remote https://aws-mcp.ap-south-1.api.aws/mcp")
            .as_deref(),
        Some("ap-south-1")
    );
    assert_eq!(
        aws.region_in("https://aws-mcp.x.example.com/#.api.aws/mcp"),
        None
    );
    assert_eq!(
        get("neon").unwrap().region_in("https://mcp.neon.tech/mcp"),
        None
    );
}

#[test]
fn region_and_paths_are_filled_in() {
    let aws = get("aws").unwrap();
    assert!(aws.has_region());
    assert_eq!(
        aws.address("eu-west-1"),
        "https://aws-mcp.eu-west-1.api.aws/mcp"
    );
    let find = |c: &str| (c == "npx").then(|| PathBuf::from("/opt/homebrew/bin/npx"));
    let rea = get("rea")
        .unwrap()
        .spec(Path::new("/x"), DEFAULT_AWS_REGION, find);
    assert!(matches!(
        rea.transport,
        crate::mcp_clients::Transport::Stdio { ref command, .. } if command == "/opt/homebrew/bin/npx"
    ));
    assert_eq!(rea.npx, "/opt/homebrew/bin/npx");
    assert_eq!(get("cap").unwrap().missing_program(find), Some("cap"));
    assert_eq!(get("rea").unwrap().missing_program(find), None);
}

/// A server the catalog runs with `npx -y` runs with the package manager picked: `bunx pkg`,
/// `pnpm dlx pkg`, `yarn dlx pkg`, its runner found where it is, nvm's node named for it; and
/// the remote bridge with it too. What it needs is that runner, not npx.
#[test]
fn local_servers_run_with_the_package_manager_picked() {
    let find = |c: &str| match c {
        "bunx" => Some(PathBuf::from("/Users/me/.bun/bin/bunx")),
        "pnpm" => Some(PathBuf::from("/opt/homebrew/bin/pnpm")),
        "node" => Some(PathBuf::from(
            "/Users/me/.nvm/versions/node/v22.11.0/bin/node",
        )),
        _ => None,
    };
    let rea = get("rea").unwrap();
    let program = |pm| {
        let spec = rea.spec_with(Path::new("/x"), DEFAULT_AWS_REGION, find, pm);
        match spec.transport {
            crate::mcp_clients::Transport::Stdio { command, args, .. } => {
                (command, args, spec.node_dir)
            }
            _ => unreachable!(),
        }
    };
    assert_eq!(
        program(PackageManager::Bun),
        (
            "/Users/me/.bun/bin/bunx".to_string(),
            vec!["rea-agents@6.3.0".to_string(), "mcp".into()],
            Some("/Users/me/.nvm/versions/node/v22.11.0/bin".to_string())
        )
    );
    assert_eq!(
        program(PackageManager::Pnpm).1,
        ["dlx", "rea-agents@6.3.0", "mcp"]
    );
    // Not found: written bare, as npx was.
    assert_eq!(program(PackageManager::Yarn).0, "yarn");
    assert_eq!(
        program(PackageManager::Yarn).1,
        ["dlx", "rea-agents@6.3.0", "mcp"]
    );
    assert_eq!(
        program(PackageManager::Npm).1,
        ["-y", "rea-agents@6.3.0", "mcp"]
    );
    assert_eq!(program(PackageManager::Npm).2, None);
    assert_eq!(
        rea.address_with(DEFAULT_AWS_REGION, PackageManager::Bun),
        "bunx rea-agents@6.3.0 mcp"
    );
    assert_eq!(
        rea.address(DEFAULT_AWS_REGION),
        "npx -y rea-agents@6.3.0 mcp"
    );
    assert_eq!(rea.missing_program_with(find, PackageManager::Bun), None);
    assert_eq!(
        rea.missing_program_with(find, PackageManager::Npm),
        Some("npx")
    );
    // Cap runs its own program, whatever is picked.
    let cap = get("cap").unwrap();
    assert_eq!(cap.package(), None);
    assert_eq!(
        cap.missing_program_with(find, PackageManager::Bun),
        Some("cap")
    );
    // The bridge runs with it too.
    let mobbin = get("mobbin").unwrap().spec_with(
        Path::new("/x"),
        DEFAULT_AWS_REGION,
        find,
        PackageManager::Pnpm,
    );
    assert_eq!(mobbin.npx, "/opt/homebrew/bin/pnpm");
    assert_eq!(mobbin.npx_args, ["dlx"]);
}
