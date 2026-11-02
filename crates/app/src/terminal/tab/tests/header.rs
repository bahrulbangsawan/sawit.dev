//! Pane headers (folder, agent logo, names) and how the focused and other panes look.

use std::time::{Duration, Instant};

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext};
use sawit_core::terminal::{AgentKind, shell_quote};

use super::test_remote;
use crate::terminal::tab::look::{INACTIVE_DIM, PaneLook, pane_style};
use crate::terminal::testing::{
    SHELLS, focus_terminal, init, open_panel, ready, scratch_dir, start, wait_until,
};
use crate::terminal::view::{AGENT_CHECK, FOLDER_TTL, TerminalView};
use crate::terminal::{JobTag, Origin};

#[gpui_kit::test]
fn a_pane_header_follows_cd_without_more_output(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    wait_until(cx, "the header shows a folder", |cx| {
        view.read(cx).cached_folder().is_some()
    });
    let dir = scratch_dir("header-cd");
    cx.update(|cx| view.read(cx).cd(&dir));
    // No more input: only the passing of time may bring the header up
    // to date. Time is the test clock's, moved one folder check at a
    // time, with a moment between for the shell to run the `cd`, for as
    // long as a busy machine takes to run it.
    let deadline = Instant::now() + SHELLS;
    while cx.update(|cx| view.read(cx).cached_folder()).as_ref() != Some(&dir) {
        assert!(
            Instant::now() < deadline,
            "the header still shows the old folder"
        );
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(FOLDER_TTL);
        cx.run_until_parked();
    }
}

/// Waits until `view` has looked for its agent and found `agent`, moving the test clock on one
/// look at a time, with a moment between for the shell to start or end the program.
fn wait_for_agent(cx: &mut TestAppContext, view: &Entity<TerminalView>, agent: Option<AgentKind>) {
    let deadline = Instant::now() + SHELLS;
    while cx.update(|cx| view.read(cx).agent()) != agent {
        assert!(
            Instant::now() < deadline,
            "the pane never found {agent:?}, only {:?}",
            cx.update(|cx| view.read(cx).agent())
        );
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(AGENT_CHECK);
        cx.run_until_parked();
    }
}

/// Whether the element tests know as `selector` is drawn in window `w`.
fn drawn(cx: &mut TestAppContext, w: AnyWindowHandle, selector: String) -> bool {
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    vcx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .is_some()
}

/// While a coding agent runs in a pane of a split tab, the pane's header shows its logo in
/// place of the terminal glyph, and once it exits the glyph is back. `cat`, run under the
/// name `claude`, plays Claude Code: it runs until its input ends, and what it is comes only
/// from its process, as the real agent's would. Like the real agent, it prints once it runs
/// (the empty line it echoes): a pane looks for its agent only after output, and on a busy
/// machine the shell may start the program after the look its last output asked for.
#[gpui_kit::test]
fn a_pane_header_shows_the_agents_logo_while_it_runs(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let (pane, view) = cx.update(|cx| {
        let t = panel.read(cx).tab(0).read(cx);
        (t.state.focused, t.focused_view().clone())
    });
    ready(cx, &view);
    let logo = format!("pane-agent-{pane}-claude-code");
    assert!(!drawn(cx, w, logo.clone()), "no agent runs yet");

    let claude = scratch_dir("agent-stand-in").join("claude");
    std::os::unix::fs::symlink("/bin/cat", &claude).unwrap();
    // macOS names a symlinked program after its target.
    start(cx, &view, &shell_quote(&claude), "cat");
    cx.update(|cx| view.read(cx).write(b"\r".to_vec()));
    wait_for_agent(cx, &view, Some(AgentKind::ClaudeCode));
    assert!(
        drawn(cx, w, logo.clone()),
        "the header shows Claude Code's logo"
    );
    cx.update(|cx| {
        let t = panel.read(cx).tab(0).read(cx);
        assert_eq!(t.agents(cx), vec![(pane, AgentKind::ClaudeCode)]);
    });

    // ^D: cat's input ends, and so does the stand-in.
    cx.update(|cx| view.read(cx).write(vec![4]));
    wait_for_agent(cx, &view, None);
    assert!(!drawn(cx, w, logo), "the glyph is back once it exits");
    cx.update(|cx| assert!(panel.read(cx).tab(0).read(cx).agents(cx).is_empty()));
}

#[gpui_kit::test]
fn an_idle_split_tab_does_not_redraw(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let renders = |cx: &mut TestAppContext| cx.update(|cx| tab.read(cx).renders);
    // Let the shells finish starting: half a second with no redraw.
    let deadline = Instant::now() + SHELLS;
    let mut last = renders(cx);
    let mut quiet_since = Instant::now();
    while quiet_since.elapsed() < Duration::from_millis(500) {
        assert!(Instant::now() < deadline, "the shells never went quiet");
        std::thread::sleep(Duration::from_millis(20));
        cx.run_until_parked();
        let now = renders(cx);
        if now != last {
            last = now;
            quiet_since = Instant::now();
        }
    }
    // A last frame or two of output, as a shell's prompt would draw.
    for _ in 0..2 {
        cx.update(|cx| tab.update(cx, |_, cx| cx.notify()));
        cx.run_until_parked();
    }

    let before = renders(cx);
    for _ in 0..10 {
        cx.executor().advance_clock(FOLDER_TTL);
        cx.run_until_parked();
    }
    let redraws = renders(cx) - before;
    assert!(
        redraws <= 1,
        "an idle split tab redrew {redraws} times in 10 folder checks"
    );
}

/// WCAG's contrast ratio of two colours.
fn contrast(a: sawit_core::terminal::Rgb, b: sawit_core::terminal::Rgb) -> f32 {
    let luminance = |c: sawit_core::terminal::Rgb| {
        let lin = |v: u8| {
            let v = v as f32 / 255.;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    };
    let (hi, lo) = {
        let (la, lb) = (luminance(a), luminance(b));
        if la > lb { (la, lb) } else { (lb, la) }
    };
    (hi + 0.05) / (lo + 0.05)
}

/// An inactive pane's terminal is dimmed, yet its text keeps the 4.5:1 text needs on the
/// default palette.
#[test]
fn dimmed_text_stays_readable() {
    let palette = sawit_core::terminal::Palette::default();
    let (fg, bg) = (palette.foreground, palette.background);
    let mix =
        |f: u8, b: u8| ((1. - INACTIVE_DIM) * f as f32 + INACTIVE_DIM * b as f32).round() as u8;
    let dimmed = sawit_core::terminal::Rgb {
        r: mix(fg.r, bg.r),
        g: mix(fg.g, bg.g),
        b: mix(fg.b, bg.b),
    };
    let ratio = contrast(dimmed, bg);
    assert!(ratio >= 4.5, "dimmed text is {ratio:.2}:1");
    assert!(contrast(fg, bg) > ratio);
}

/// The pane with the keyboard looks active: an accent border, the active tab's header
/// colours and a heavier title. The others sit back, muted and dimmed. Once the keyboard is
/// elsewhere in the window, the tab's focused pane keeps its header and heavier title with a
/// quieter border.
#[gpui_kit::test]
fn the_focused_pane_looks_active_and_the_others_sit_back(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init(cx);
        crate::theme::apply_zed_one_dark(cx);
    });
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let (left, right) = cx.update(|cx| {
        let panes = tab.read(cx).state.layout.panes();
        (panes[0], panes[1])
    });
    let looks = |cx: &mut TestAppContext| {
        cx.update_window(w, |_, window, cx| tab.read(cx).looks(window, cx))
            .unwrap()
    };
    assert_eq!(
        looks(cx),
        [(left, PaneLook::Inactive), (right, PaneLook::Active)]
    );
    // As drawn.
    let drawn = |cx: &mut TestAppContext, pane: u64, look: PaneLook| {
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        let selector: &'static str = Box::leak(format!("pane-{pane}-{look:?}").into_boxed_str());
        vcx.debug_bounds(selector).is_some()
    };
    assert!(drawn(cx, right, PaneLook::Active));
    assert!(drawn(cx, left, PaneLook::Inactive));

    cx.update(|cx| {
        let theme = gpui_kit::component::ActiveTheme::theme(cx);
        let bg = tab.read(cx).focused_view().read(cx).background();
        let active = pane_style(PaneLook::Active, theme, bg);
        let current = pane_style(PaneLook::Current, theme, bg);
        let inactive = pane_style(PaneLook::Inactive, theme, bg);
        assert_ne!(active.border, inactive.border);
        assert_ne!(active.header_bg, inactive.header_bg);
        assert_ne!(active.title, inactive.title);
        // Never by colour alone.
        assert!(active.title_weight > inactive.title_weight);
        assert!(current.title_weight > inactive.title_weight);
        assert_eq!((active.dim, current.dim), (None, None));
        assert!(inactive.dim.is_some());
        assert_ne!(
            current.border, active.border,
            "the current pane is as loud as the active one"
        );
        assert_eq!(current.header_bg, active.header_bg);
    });

    // The keyboard goes elsewhere in the window (an editor, say).
    let elsewhere = cx.update(|cx| cx.focus_handle());
    cx.update_window(w, |_, window, cx| window.focus(&elsewhere, cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        looks(cx),
        [(left, PaneLook::Inactive), (right, PaneLook::Current)]
    );
    assert!(
        drawn(cx, right, PaneLook::Current),
        "the tab wasn't drawn again"
    );
}

#[gpui_kit::test]
fn ssh_and_script_pane_headers_use_the_tab_strips_names(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.open_ssh(test_remote(), Origin::User, window, cx)
        })
    })
    .unwrap()
    .expect("the session starts");
    cx.run_until_parked();
    let tab = cx.update(|cx| {
        let p = panel.read(cx);
        p.tab(p.active_index()).clone()
    });
    let (ssh, ssh_view) = cx.update(|cx| {
        let t = tab.read(cx);
        (t.state.focused, t.focused_view().clone())
    });
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let (script, script_view) = cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.views().count(), 2);
        (t.state.focused, t.focused_view().clone())
    });
    // The programs set their own titles, which the headers pass over.
    cx.update(|cx| {
        ssh_view.update(cx, |v, _| v.title = "zsh".into());
        script_view.update(cx, |v, _| {
            v.title = "node".into();
            v.job = Some(JobTag {
                key: "dev".into(),
                name: "dev".into(),
            });
        });
    });
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.pane_label(ssh, ssh_view.read(cx)), "test");
        assert_eq!(t.pane_label(script, script_view.read(cx)), "dev");
    });
    cx.update(|cx| tab.update(cx, |t, _| t.state.rename(ssh, "prod")));
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.pane_label(ssh, ssh_view.read(cx)), "prod");
    });
}
