//! The AWS Region: typed, it reads no config again; installed in another, the page takes it.

use super::*;

/// Typing a Region works out the statuses from the configs the last detection read: it
/// doesn't read every client's config again, on the UI thread, for each character.
#[gpui_kit::test]
fn typing_a_region_reads_no_config(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".codex/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        "[mcp_servers.aws]\nurl = \"https://aws-mcp.eu-west-1.api.aws/mcp\"\n",
    )
    .unwrap();
    let codex = target(Client::Codex, &config);
    let read = sawit_core::mcp_clients::read_entries(&codex);
    let (page, cx) = open(cx, vec![codex], |page| page.entries = vec![read]);
    // Since the detection, the file has changed (here: it no longer reads).
    std::fs::write(&config, "[mcp_servers\n").unwrap();
    page.update(cx, |page, cx| page.set_region("eu-west-1".into(), cx));
    let aws = super::SERVERS.iter().position(|s| s.id == "aws").unwrap();
    assert_eq!(
        page.read_with(cx, |page, _| page.status.get(&(aws, 0)).cloned()),
        Some(Status::Installed)
    );
}

/// The AWS server installed in another Region than the page's default is installed there:
/// the page takes that Region, and the sheet doesn't offer to move it.
#[gpui_kit::test]
fn aws_in_another_region_is_installed_there(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let codex = home.path().join(".codex/config.toml");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    std::fs::write(
        &codex,
        "[mcp_servers.aws]\nurl = \"https://aws-mcp.eu-west-1.api.aws/mcp\"\n",
    )
    .unwrap();
    let cursor = home.path().join(".cursor/mcp.json");
    std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
    let targets = vec![
        target(Client::Codex, &codex),
        target(Client::Cursor, &cursor),
    ];
    let (page, cx) = open(cx, Vec::new(), |_| {});
    page.update(cx, |page, cx| {
        let detected = super::Detected {
            entries: targets
                .iter()
                .map(sawit_core::mcp_clients::read_entries)
                .collect(),
            targets,
            clis: Default::default(),
            programs: Default::default(),
            server_exists: true,
        };
        page.apply(detected, cx)
    });
    let aws = super::SERVERS.iter().position(|s| s.id == "aws").unwrap();
    page.read_with(cx, |page, _| {
        assert_eq!(page.region, "eu-west-1");
        assert_eq!(page.status.get(&(aws, 0)), Some(&Status::Installed));
    });
    // Typed back to us-east-1, it's in another Region: not ticked for Install.
    page.update(cx, |page, cx| page.set_region("us-east-1".into(), cx));
    page.update_in(cx, |page, window, cx| page.open_install(aws, window, cx));
    page.read_with(cx, |page, _| {
        let Some(super::Sheet::Install(sheet)) = &page.sheet else {
            panic!("no sheet")
        };
        assert!(sheet.selected.is_empty(), "{:?}", sheet.selected);
    });
}
