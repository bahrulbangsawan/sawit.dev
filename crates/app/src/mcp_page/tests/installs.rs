//! Installing and removing: clients that need the bridge, results when the list changes,
//! failures on the row, the package manager picked, and asking before a removal.

use super::*;

/// A client that only runs programs (Claude Desktop, …) reaches a remote server through
/// `npx mcp-remote`: without npx it isn't installed there, and Install leaves it out.
#[gpui_kit::test]
fn remote_servers_skip_bridge_only_clients_without_npx(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("claude_desktop_config.json");
    std::fs::write(&config, "{}").unwrap();
    let desktop = target(Client::ClaudeDesktop, &config);
    let read = sawit_core::mcp_clients::read_entries(&desktop);
    let neon = super::SERVERS.iter().position(|s| s.id == "neon").unwrap();
    let (page, cx) = open(cx, vec![desktop], |page| {
        page.entries = vec![read];
        page.recompute();
    });
    page.update_in(cx, |page, window, cx| {
        page.open_install(neon, window, cx);
        if let Some(super::Sheet::Install(sheet)) = &mut page.sheet {
            sheet.selected.insert(0);
        }
        page.install_selected(cx);
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "{}");
}

/// A detection can change which client is at which place in the list while an install
/// runs (a client appeared, or one went): the result lands on the client it was for, and
/// a place past the end of the list is no client at all.
#[gpui_kit::test]
fn results_land_on_their_client_when_the_list_changes(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = |name: &str| {
        let path = home.path().join(name).join("mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        path
    };
    let (a, b) = (config("a"), config("b"));
    let (page, cx) = open(cx, vec![target(Client::Cursor, &a)], |page| {
        page.entries = vec![sawit_core::mcp_clients::read_entries(&target(
            Client::Cursor,
            &a,
        ))];
        page.recompute();
        page.server_exists = true;
    });
    page.update(cx, |page, cx| {
        page.act((0, 0), false, cx);
        // Detect Again finds another client first.
        let targets = vec![target(Client::Cursor, &b), target(Client::Cursor, &a)];
        page.apply(
            super::Detected {
                entries: targets
                    .iter()
                    .map(sawit_core::mcp_clients::read_entries)
                    .collect(),
                targets,
                clis: Default::default(),
                programs: Default::default(),
                server_exists: true,
            },
            cx,
        );
        // And a place past the end of the list.
        page.check_sign_ins(vec![7], cx);
    });
    let done = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        page.read_with(cx, |page, _| page.busy.is_empty())
    });
    assert!(done, "the install didn't finish");
    page.read_with(cx, |page, _| {
        assert_eq!(page.status.get(&(0, 1)), Some(&Status::Installed), "a");
        assert_eq!(page.status.get(&(0, 0)), Some(&Status::NotInstalled), "b");
    });
}

/// A detection that finds other clients closes a sheet opened on the old list: its rows
/// are places in that list, which now hold other clients or none.
#[gpui_kit::test]
fn a_detection_closes_a_sheet_on_the_old_list(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = |name: &str| {
        let path = home.path().join(name).join(".claude.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        path
    };
    let (a, b) = (config("a"), config("b"));
    let aws = super::SERVERS.iter().position(|s| s.id == "aws").unwrap();
    let (page, cx) = open(
        cx,
        vec![
            target(Client::ClaudeCode, &a),
            target(Client::ClaudeCode, &b),
        ],
        |_| {},
    );
    page.update_in(cx, |page, window, cx| {
        page.start_sign_in(vec![(aws, 1)], window, cx)
    });
    cx.run_until_parked();
    page.update(cx, |page, cx| {
        let targets = vec![target(Client::ClaudeCode, &a)];
        page.apply(
            super::Detected {
                entries: targets
                    .iter()
                    .map(sawit_core::mcp_clients::read_entries)
                    .collect(),
                targets,
                clis: Default::default(),
                programs: Default::default(),
                server_exists: true,
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert!(page.read_with(cx, |page, _| page.sheet.is_none()));
}

/// A row's Uninstall (or Install) that fails for a client says so on the row, which is
/// where it was started: not only in the Manage sheet, which may never be opened.
#[gpui_kit::test]
fn a_failure_shows_on_the_servers_row(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".cursor/mcp.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    // Comments: Sawit won't rewrite it, so the change fails.
    std::fs::write(&config, "// mine\n{\"mcpServers\": {}}\n").unwrap();
    let (page, cx) = open(cx, vec![target(Client::Cursor, &config)], |page| {
        page.server_exists = true;
    });
    assert!(cx.debug_bounds("mcp-row-error").is_none());
    page.update(cx, |page, cx| page.act((0, 0), false, cx));
    let failed = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        page.read_with(cx, |page, _| !page.errors.is_empty())
    });
    assert!(failed, "the change didn't fail");
    assert!(
        cx.debug_bounds("mcp-row-error").is_some(),
        "the row doesn't say"
    );
}

/// The package manager picked runs a local server's package and the remote bridge: the
/// entry written for REA starts `bunx rea-agents@… mcp`, the row and sheet say so, and with
/// bunx found nothing blocks the install though npx isn't there.
#[gpui_kit::test]
fn the_package_manager_picked_runs_the_server(cx: &mut TestAppContext) {
    use sawit_core::mcp_catalog::SERVERS;
    use sawit_core::package_manager::PackageManager;
    let (page, cx) = open(cx, vec![], |page| {
        page.programs
            .insert("bunx", std::path::PathBuf::from("/Users/me/.bun/bin/bunx"));
        page.pm = Some(PackageManager::Bun);
    });
    let rea = SERVERS.iter().position(|s| s.id == "rea").unwrap();
    page.read_with(cx, |page, cx| {
        let spec = page.spec(rea);
        assert!(matches!(
            spec.transport,
            sawit_core::mcp_clients::Transport::Stdio { ref command, ref args, .. }
                if command == "/Users/me/.bun/bin/bunx" && args[0] == "rea-agents@6.3.0"
        ));
        assert!(!page.install_blocked(rea, cx));
        assert_eq!(
            SERVERS[rea].address_with("us-east-1", page.run_pm()),
            "bunx rea-agents@6.3.0 mcp"
        );
    });
}

/// The sheet opens with every installed client ticked, so its Remove asks first, as the
/// row's trash does: one click must not take a server out of every client.
#[gpui_kit::test]
fn the_sheets_remove_asks_first(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".cursor/mcp.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        r#"{"mcpServers": {"sawit": {"command": "/x/sawit-mcp"}}}"#,
    )
    .unwrap();
    let (page, cx) = open(cx, vec![target(Client::Cursor, &config)], |page| {
        page.status.insert((0, 0), Status::Installed);
    });
    page.update_in(cx, |page, window, cx| page.open_install(0, window, cx));
    cx.run_until_parked();

    // The removal runs off the UI thread: give it the time it takes.
    let has_sawit = |cx: &mut VisualTestContext| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        std::fs::read_to_string(&config).unwrap().contains("sawit")
    };
    let removed = |cx: &mut VisualTestContext| (0..50).any(|_| !has_sawit(cx));
    click(cx, "mcp-sheet-remove");
    assert!(has_sawit(cx), "removed without asking");
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Remove Sawit from 1 client?".to_string(),
            "Its entry is removed from: Cursor. Restart those clients to drop it.".to_string()
        ))
    );
    cx.simulate_prompt_answer("Cancel");
    assert!(has_sawit(cx), "Cancel removed it");

    click(cx, "mcp-sheet-remove");
    cx.simulate_prompt_answer("Remove");
    assert!(removed(cx), "Remove didn't remove it");
}
