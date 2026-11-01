//! The Skills page's tests: the list fills the window, a pack's sheet shows the commands before
//! they run and ticks where the pack is installed, and closing the page stops its script.

use std::path::PathBuf;

use gpui_kit::{
    AppContext as _, Bounds, Entity, TestAppContext, VisualTestContext, WindowBounds,
    WindowOptions, point, px, size,
};
use sawit_core::package_manager::{Available, PackageManager};
use sawit_core::skills::{self, ClaudeHome, Installed, Report, Target};

use super::{Action, SkillsPage, State};

fn targets() -> Report {
    let claude = Target {
        label: "Claude Code".into(),
        clients: vec![],
        dir: "/Users/me/.claude/skills".into(),
        claude: Some(ClaudeHome {
            dir: "/Users/me/.claude".into(),
            profile_dir: None,
        }),
        cli_agent: Some("claude-code"),
    };
    let shared = Target {
        label: "Codex, Zed".into(),
        clients: vec![],
        dir: "/Users/me/.agents/skills".into(),
        claude: None,
        cli_agent: Some("codex"),
    };
    let has_cf = Installed {
        folder: ["cf-cli".to_string()].into(),
        ..Default::default()
    };
    Report {
        targets: vec![claude, shared],
        installed: vec![Installed::default(), has_cf],
    }
}

/// The page in a 1200 × 900 window with `report` detected, bun and npm installed.
fn open(
    cx: &mut TestAppContext,
    setup: impl FnOnce(&mut SkillsPage),
) -> (Entity<SkillsPage>, &mut VisualTestContext) {
    open_at(cx, 1200., setup)
}

/// [`open`] in a window `width` wide.
fn open_at(
    cx: &mut TestAppContext,
    width: f32,
    setup: impl FnOnce(&mut SkillsPage),
) -> (Entity<SkillsPage>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::catalog::init(cx);
        cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
    });
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(width), px(900.)),
        })),
        ..Default::default()
    };
    let (window, page) = cx
        .update(|cx| {
            gpui_kit::open_window(options, cx, |_, cx| {
                cx.new(|cx| {
                    let mut page = SkillsPage::blank(cx);
                    page.report = targets();
                    page.loading = false;
                    page.claude = true;
                    page.git = true;
                    page.env = skills::Env {
                        home: "/Users/me".into(),
                        claude_config_dir_set: false,
                    };
                    page.pms = [PackageManager::Npm, PackageManager::Bun]
                        .into_iter()
                        .map(|pm| Available {
                            pm,
                            runner: PathBuf::from(format!("/bin/{}", pm.runner())),
                            version: None,
                        })
                        .collect();
                    page.pm = Some(PackageManager::Bun);
                    setup(&mut page);
                    page
                })
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(window, cx).into_mut();
    cx.run_until_parked();
    (page, cx)
}

/// The page fills the window as the other catalogs do: the list spans it, inside the 24 px
/// inset, at the display's width and at the window's narrowest; no column caps it.
#[gpui_kit::test]
fn the_list_fills_the_window(cx: &mut TestAppContext) {
    for width in [1470., 720.] {
        let (_page, vcx) = open_at(cx, width, |_| {});
        let list = vcx.debug_bounds("skills-list").expect("the list is drawn");
        assert_eq!(f32::from(list.origin.x).round(), 24., "{width}");
        assert_eq!(f32::from(list.size.width).round(), width - 48., "{width}");
    }
}

/// A pack's sheet ticks the agents that have it, shows the exact commands with the package
/// manager picked before anything runs, and changes them with the way it installs.
#[gpui_kit::test]
fn the_sheet_shows_the_commands_before_they_run(cx: &mut TestAppContext) {
    let (page, cx) = open(cx, |_| {});
    let mcp_apps = skills::get("mcp-apps").unwrap();
    cx.update(|window, cx| page.update(cx, |page, cx| page.open_sheet(mcp_apps, window, cx)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("skills-sheet-commands").is_some());
    let script = |page: &Entity<SkillsPage>, cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| {
            page.sheet_script(Action::Install, &page.picked(super::Pick::Install))
        })
    };
    // Its plugin first, into Claude Code (the one agent it reaches).
    assert_eq!(
        script(&page, cx).unwrap(),
        [
            "claude plugin marketplace add modelcontextprotocol/ext-apps",
            "claude plugin install mcp-apps@mcp-apps"
        ]
    );
    // The skills CLI, run with bunx, into both.
    page.update(cx, |page, _| {
        let sheet = page.sheet.as_mut().unwrap();
        sheet.method = mcp_apps.methods[1];
        sheet.selected = [0, 1].into();
    });
    assert_eq!(
        script(&page, cx).unwrap(),
        ["bunx skills add modelcontextprotocol/ext-apps --skill '*' -g -a claude-code -a codex -y"]
    );
    // Nothing ran.
    assert!(page.read_with(cx, |page, _| page.running.is_none()
        && page.queue.is_empty()));
}

/// shadcn's improve opens on the way its README documents, the skills CLI with the package
/// manager picked, and offers its Claude Code plugin second.
#[gpui_kit::test]
fn improve_opens_on_the_skills_cli_its_readme_documents(cx: &mut TestAppContext) {
    let (page, cx) = open(cx, |_| {});
    let improve = skills::get("improve").unwrap();
    cx.update(|window, cx| page.update(cx, |page, cx| page.open_sheet(improve, window, cx)));
    cx.run_until_parked();
    let script = |page: &Entity<SkillsPage>, cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| {
            page.sheet_script(Action::Install, &page.picked(super::Pick::Install))
        })
    };
    assert_eq!(
        page.read_with(cx, |page, _| page.sheet.as_ref().unwrap().method),
        improve.methods[0]
    );
    assert_eq!(
        script(&page, cx).unwrap(),
        ["bunx skills add shadcn/improve --skill '*' -g -a claude-code -y"]
    );
    page.update(cx, |page, _| {
        page.sheet.as_mut().unwrap().selected = [0, 1].into();
    });
    assert_eq!(
        script(&page, cx).unwrap(),
        ["bunx skills add shadcn/improve --skill '*' -g -a claude-code -a codex -y"]
    );
    page.update(cx, |page, _| {
        page.sheet.as_mut().unwrap().method = improve.methods[1];
    });
    // The plugin goes into Claude Code only.
    assert_eq!(
        script(&page, cx).unwrap(),
        [
            "claude plugin marketplace add shadcn/improve",
            "claude plugin install improve@improve"
        ]
    );
    assert!(page.read_with(cx, |page, _| page.running.is_none()
        && page.queue.is_empty()));
}

/// The sheet opens with the agents that have the pack ticked: Update and Remove act on them,
/// Install on none.
#[gpui_kit::test]
fn the_sheet_ticks_where_it_is_installed(cx: &mut TestAppContext) {
    let (page, cx) = open(cx, |_| {});
    let cf = skills::get("cf-cli").unwrap();
    cx.update(|window, cx| page.update(cx, |page, cx| page.open_sheet(cf, window, cx)));
    page.read_with(cx, |page, _| {
        assert_eq!(page.picked(super::Pick::Update), [1]);
        assert_eq!(page.picked(super::Pick::Remove), [1]);
        assert!(page.picked(super::Pick::Install).is_empty());
    });
}

/// Closing the Skills tab drops the page: the script it runs stops with it.
#[gpui_kit::test]
fn closing_the_page_stops_its_script(cx: &mut TestAppContext) {
    use futures::FutureExt as _;
    let (run, stop) = futures::future::abortable(futures::future::pending::<()>());
    let page = cx.new(|cx| {
        let mut page = SkillsPage::blank(cx);
        page.running = Some(super::Job {
            pack: skills::get("superpowers").unwrap(),
            action: Action::Install,
            lines: vec!["true".into()],
        });
        page.states
            .insert("superpowers", State::Running(Action::Install));
        page.stop = Some(stop);
        page
    });
    assert_eq!(
        page.read_with(cx, |page, _| page.work()).as_deref(),
        Some("Installing Superpowers")
    );
    drop(page);
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(run.now_or_never(), Some(Err(futures::future::Aborted)));
}
