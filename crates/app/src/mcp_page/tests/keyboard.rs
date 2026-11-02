//! The search, and working the page from the keyboard.

use super::*;

/// The search finds what the rows show, their tags too: "sign" finds the servers tagged
/// Sign-in, "api key" the one tagged API key.
#[test]
fn the_search_matches_the_tags() {
    let at = |id: &str| super::SERVERS.iter().position(|s| s.id == id).unwrap();
    assert!(McpPage::matches(at("aws"), "sign"));
    assert!(McpPage::matches(at("epidemic-sound"), "api key"));
    assert!(!McpPage::matches(at("exa"), "sign"));
}

/// The page works from the keyboard: ↩ in the search moves it to the first row, ↩ there
/// opens the server's sheet, ↩ in the sheet installs, Escape closes it and ↓ goes on down
/// the list.
#[gpui_kit::test]
fn the_page_works_from_the_keyboard(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".cursor/mcp.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "{}").unwrap();
    let cursor = target(Client::Cursor, &config);
    let read = sawit_core::mcp_clients::read_entries(&cursor);
    let (page, cx) = open(cx, vec![cursor], |page| {
        page.entries = vec![read];
        page.recompute();
        page.server_exists = true;
    });
    cx.update(|_, cx| crate::catalog::init(cx));
    page.update_in(cx, |page, window, cx| page.focus_search(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    assert_eq!(page.read_with(cx, |page, _| page.cursor), Some(0));
    cx.simulate_keystrokes("enter");
    let sheet = |cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| match &page.sheet {
            Some(super::Sheet::Install(sheet)) => Some(sheet.server),
            _ => None,
        })
    };
    assert_eq!(sheet(cx), Some(0), "↩ didn't open Sawit's sheet");
    page.update(cx, |page, _| {
        if let Some(super::Sheet::Install(sheet)) = &mut page.sheet {
            sheet.selected.insert(0);
        }
    });
    cx.simulate_keystrokes("enter");
    let installed = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        std::fs::read_to_string(&config).unwrap().contains("sawit")
    });
    assert!(installed, "↩ in the sheet didn't install");
    cx.simulate_keystrokes("escape");
    assert_eq!(sheet(cx), None);
    cx.simulate_keystrokes("down");
    assert_eq!(page.read_with(cx, |page, _| page.cursor), Some(1));
}
