//! Installing and upgrading: the package manager an npm tool installs with, PureMac behind
//! Homebrew, an app installed outside what upgrades it, and PureMac's logo.

use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::dev_tools::{Found, find};

use super::queued;
use crate::tools_page::{Job, State, ToolsPage};

/// A tool npm installs globally installs with the package manager picked; over a copy found
/// (Reinstall, Upgrade) it runs with the one that installed that copy, so no second copy
/// lands beside it. Other tools keep their own installers.
#[gpui_kit::test]
fn npm_tools_install_with_the_package_manager_picked(cx: &mut TestAppContext) {
    use sawit_core::package_manager::PackageManager;
    let page = cx.new(|cx| {
        let mut page = ToolsPage::blank(cx);
        page.pm = Some(PackageManager::Pnpm);
        page
    });
    let cf = find("cf").unwrap();
    page.update(cx, |page, _| {
        assert_eq!(page.command(cf), "pnpm add -g cf");
        assert_eq!(
            page.command(find("bun").unwrap()),
            find("bun").unwrap().install
        );
        page.found.insert(
            "cf",
            Found {
                path: "/nonexistent-sawit-test/.bun/install/global/node_modules/cf/bin/cf".into(),
                version: Some("1.0.0".into()),
                outside: None,
            },
        );
        assert_eq!(page.command(cf), "bun add -g cf");
        page.upgrades.insert("cf");
        assert_eq!(page.command(cf), "bun add -g cf");
    });
}

/// PureMac installs from its Homebrew cask: picked while Homebrew is missing, Homebrew's
/// installer goes to the terminal (it asks for a password) and PureMac waits behind it. Its
/// install and upgrade are brew's whatever package manager is picked.
#[gpui_kit::test]
fn puremac_waits_for_homebrew_and_installs_with_brew(cx: &mut TestAppContext) {
    use std::cell::RefCell;
    use std::rc::Rc;

    use sawit_core::package_manager::PackageManager;

    use crate::tools_page::ToolsEvent;
    let page = cx.new(|cx| {
        let mut page = ToolsPage::blank(cx);
        page.loading = false;
        page.pm = Some(PackageManager::Bun);
        // Another installer "runs", so nothing queued starts here.
        page.running = Some(("deno", Job::Install));
        page
    });
    let terminal = Rc::new(RefCell::new(Vec::new()));
    let seen = terminal.clone();
    cx.update(|cx| {
        cx.subscribe(&page, move |_, event: &ToolsEvent, _| {
            let ToolsEvent::RunInTerminal(command) = event;
            seen.borrow_mut().push(command.clone());
        })
        .detach()
    });
    page.update(cx, |page, cx| page.enqueue(&["puremac"], cx));
    cx.run_until_parked();
    assert_eq!(
        *terminal.borrow(),
        [find("homebrew").unwrap().install.to_string()]
    );
    assert_eq!(
        page.read_with(cx, |page, _| page.states.get("homebrew").cloned()),
        Some(State::InTerminal)
    );
    assert_eq!(queued(cx, &page), [("puremac", Job::Install)]);
    let puremac = find("puremac").unwrap();
    page.update(cx, |page, _| {
        assert_eq!(page.command(puremac), "brew install --cask puremac");
        page.upgrades.insert("puremac");
        assert_eq!(page.command(puremac), "brew upgrade --cask puremac");
    });
}

/// An app installed outside what upgrades it (PureMac from its disk image, Zed without its
/// script's command) shows its version and why, but no Update badge, Upgrade, Upgrade All,
/// Reinstall or ↩; Homebrew's copy of PureMac gets them all.
#[gpui_kit::test]
fn an_app_installed_outside_homebrew_offers_no_upgrade(cx: &mut TestAppContext) {
    use gpui_kit::VisualTestContext;
    use sawit_core::dev_tools::Outside;

    use crate::tools_page::Filter;
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::catalog::init(cx);
    });
    let at = |path: &str, v: &str, outside| Found {
        path: path.into(),
        version: Some(v.into()),
        outside,
    };
    let (window, page) = cx
        .update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |_, cx| {
                cx.new(|cx| {
                    let mut page = ToolsPage::blank(cx);
                    page.loading = false;
                    page.filter = Filter::Installed;
                    page.found = [
                        (
                            "puremac",
                            at(
                                "/Applications/PureMac.app",
                                "3.1.0",
                                Some(Outside::Homebrew),
                            ),
                        ),
                        (
                            "zed",
                            at(
                                "/Applications/Zed.app",
                                "0.200.0",
                                Some(Outside::InstallScript),
                            ),
                        ),
                        ("bun", at("/Users/me/.bun/bin/bun", "1.0.0", None)),
                    ]
                    .into();
                    page.latest = [
                        ("puremac", "3.2.0".to_string()),
                        ("zed", "0.210.0".to_string()),
                        ("bun", "1.1.0".to_string()),
                    ]
                    .into();
                    page
                })
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(window, cx).into_mut();
    cx.run_until_parked();
    page.read_with(cx, |page, _| {
        assert_eq!(page.newer("puremac"), None);
        assert_eq!(page.newer("zed"), None);
        assert_eq!(page.upgradable(), ["bun"]);
        assert!(!page.offers_reinstall("puremac") && !page.offers_reinstall("zed"));
        assert!(page.offers_reinstall("bun"));
        let tool = |id| find(id).unwrap();
        assert!(!page.passes(tool("puremac"), Filter::Updates));
        assert!(page.passes(tool("bun"), Filter::Updates));
    });
    // The row says why, under PureMac and Zed alike.
    assert!(cx.debug_bounds("tool-outside-puremac").is_some());
    assert!(cx.debug_bounds("tool-outside-zed").is_some());
    assert!(cx.debug_bounds("tool-outside-bun").is_none());
    // ↩ on PureMac's row runs nothing.
    page.update(cx, |page, cx| {
        page.cursor = page.shown("").iter().position(|(_, t)| t.id == "puremac");
        assert!(page.cursor.is_some());
        page.confirm(cx);
        assert!(page.queue.is_empty() && page.running.is_none());
        assert_eq!(page.states.get("puremac"), None);
    });
    // Homebrew's copy: Update, Upgrade and Reinstall are back.
    page.update(cx, |page, _| {
        page.found.get_mut("puremac").unwrap().outside = None;
        assert_eq!(page.newer("puremac").map(String::as_str), Some("3.2.0"));
        assert_eq!(page.upgradable(), ["bun", "puremac"]);
        assert!(page.offers_reinstall("puremac"));
    });
}

/// PureMac's logo is its app icon, a PNG in the app's assets; the SVG logos load as before.
#[test]
fn puremacs_logo_is_its_app_icon() {
    use gpui_kit::AssetSource as _;
    let load = |path: &str| {
        crate::assets::AppAssets
            .load(path)
            .ok()
            .flatten()
            .map(|bytes| bytes.to_vec())
    };
    let png = load("tool_logos/puremac.png").expect("PureMac's icon");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(load("tool_logos/bun.svg").is_some());
    assert!(load("tool_logos/bun.png").is_none());
    assert!(load("tool_logos/puremac.svg").is_none());
}
