//! Lane 3 (mesurer-ui): mesurer's keys (⌘K, ⇧⌘K, ⌘A, an agent's Esc), its links, `ui.state`, and
//! the Annotate panel's status and "Page tools" lines.

use gpui_kit::prelude::*;
use gpui_kit::{
    Action, Entity, FocusHandle, KeyContext, Keystroke, OperatingSystem, VisualTestContext, Window,
    div,
};
use sawit_core::browser::BrowserOp;
use sawit_core::browser::mesurer::Summary;
use serde_json::{Value, json};

use super::{engine_event, test_browser};
use crate::browser::annotate::Picking;
use crate::browser::engine::{EngineEvent, WorldReply};
use crate::browser::{
    BrowserView, CopyCommentsForAgent, Panel, PickMode, ResolveAllComments, SelectAllAnnotations,
    ShowMesurerTools,
};

/// The actions the keymap runs for `keys` with the focus at the end of `contexts`, and whether
/// they are a chord's start.
fn bound(keys: &str, contexts: &[&str], cx: &mut gpui_kit::TestAppContext) -> (Vec<String>, bool) {
    cx.update(|cx| {
        let typed: Vec<Keystroke> = keys
            .split(' ')
            .map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).unwrap())
            .collect();
        let stack: Vec<KeyContext> = contexts
            .iter()
            .map(|c| KeyContext::parse(c).unwrap())
            .collect();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, pending) = keymap.bindings_for_input(&typed, &stack);
        let names = bindings.iter().map(|b| b.action().name().to_string());
        (names.collect(), pending)
    })
}

/// While mesurer is mounted and the page has the keyboard (`Browser mesurer`), ⌘K and ⇧⌘K are
/// mesurer's at once, past the workspace's ⌘K chords; elsewhere ⌘K starts those chords as before.
/// ⌘A is mesurer's only in its Select tool (`mesurer_select`); otherwise no binding takes it,
/// and the Edit menu's Select All does.
#[gpui_kit::test]
fn the_keymap_gives_cmd_k_and_cmd_a_to_mesurer_only_where_they_are_its(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
    });
    let page = ["Root", "Workspace", "Browser mesurer"];
    let select = ["Root", "Workspace", "Browser mesurer mesurer_select"];
    let plain = ["Root", "Workspace", "Browser"];
    let one = |name: &str| (vec![name.to_string()], false);
    assert_eq!(
        bound("cmd-k", &page, cx),
        one("browser::CopyCommentsForAgent")
    );
    assert_eq!(
        bound("cmd-shift-k", &page, cx),
        one("browser::ResolveAllComments")
    );
    assert_eq!(
        bound("cmd-k", &plain, cx),
        (vec![], true),
        "a chord's start"
    );
    assert_eq!(bound("cmd-k w", &plain, cx), one("workspace::CloseAllTabs"));
    assert_eq!(bound("cmd-a", &page, cx), (vec![], false));
    assert_eq!(
        bound("cmd-a", &select, cx),
        one("browser::SelectAllAnnotations")
    );
    assert_eq!(bound("cmd-a", &plain, cx), (vec![], false));
}

/// The workspace around the browser's key context as the browser draws it
/// ([`BrowserView::mesurer_key_context`]), with its address field, the browser's handlers of
/// mesurer's shortcuts, and the workspace's ⌘K W counted.
struct StandIn {
    focus: FocusHandle,
    browser: Entity<BrowserView>,
    closes: usize,
}

impl Render for StandIn {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (context, view, address) = {
            let b = self.browser.read(cx);
            (
                b.mesurer_key_context(window),
                b.focus.clone(),
                b.address.clone(),
            )
        };
        let to = |browser: &Entity<BrowserView>| {
            let browser = browser.clone();
            move |action: &dyn Action, cx: &mut gpui_kit::App| {
                browser.update(cx, |b, cx| b.mesurer_action(action, cx))
            }
        };
        let (copy, resolve, all) = (to(&self.browser), to(&self.browser), to(&self.browser));
        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .size_full()
            .on_action(
                cx.listener(|this, _: &crate::workspace::CloseAllTabs, _, cx| {
                    this.closes += 1;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .key_context(context)
                    .track_focus(&view)
                    .size_full()
                    .on_action(move |a: &CopyCommentsForAgent, _, cx| copy(a, cx))
                    .on_action(move |a: &ResolveAllComments, _, cx| resolve(a, cx))
                    .on_action(move |a: &SelectAllAnnotations, _, cx| all(a, cx))
                    .child(gpui_kit::component::input::Input::new(&address)),
            )
    }
}

/// Typed for real: with mesurer away the workspace's ⌘K W runs; mounted, with the page's
/// keyboard, ⌘K and ⇧⌘K go to mesurer (and ⌘K W is no chord there), while the address field
/// keeps the workspace's chords; ⌘A goes to mesurer in its Select tool only.
#[gpui_kit::test]
fn typed_cmd_k_and_cmd_a_reach_mesurer_from_the_page(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
    });
    let (stand_in, cx) = cx.add_window_view(|window, cx| StandIn {
        focus: cx.focus_handle(),
        browser: cx.new(|cx| BrowserView::new(None, None, window, cx)),
        closes: 0,
    });
    let browser = cx.update(|_, cx| stand_in.read(cx).browser.clone());
    let to_page = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let focus = browser.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    };
    let set = |mounted: bool, tool: &str, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            browser.update(cx, |b, _| {
                b.mesurer.mounted = mounted;
                b.mesurer.summary.tool = tool.into();
            });
            stand_in.update(cx, |_, cx| cx.notify());
        });
        cx.run_until_parked();
    };
    let press = |keys: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
    };
    let last = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| browser.read(cx).mesurer.keys.last_key.clone())
    };
    let closes = |cx: &mut VisualTestContext| cx.update(|_, cx| stand_in.read(cx).closes);

    to_page(cx);
    press("cmd-k w", cx);
    assert_eq!((closes(cx), last(cx)), (1, None), "mesurer away: ⌘K W");

    set(true, "none", cx);
    to_page(cx);
    press("cmd-k", cx);
    assert_eq!(
        last(cx),
        Some(json!({"key": "k", "code": "KeyK", "meta": true}))
    );
    press("cmd-shift-k", cx);
    assert_eq!(
        last(cx),
        Some(json!({"key": "k", "code": "KeyK", "meta": true, "shift": true}))
    );
    press("cmd-a", cx);
    assert_eq!(last(cx).unwrap()["key"], "k", "⌘A is Select All outside S");
    assert_eq!(closes(cx), 1);

    set(true, "selection", cx);
    press("cmd-a", cx);
    assert_eq!(
        last(cx),
        Some(json!({"key": "a", "code": "KeyA", "meta": true}))
    );

    // The address field has the keyboard: the workspace's chords.
    let address = cx.update(|_, cx| browser.read(cx).address.clone());
    cx.update(|window, cx| address.update(cx, |a, cx| a.focus(window, cx)));
    cx.update(|_, cx| stand_in.update(cx, |_, cx| cx.notify()));
    cx.run_until_parked();
    press("cmd-k w", cx);
    assert_eq!(closes(cx), 2, "⌘K W from the address field");
}

/// Posts `message` (`{kind, payload}`) from mesurer's world; answers what the app replied.
fn post(
    view: &Entity<BrowserView>,
    message: Value,
    cx: &mut VisualTestContext,
) -> Result<String, String> {
    let mut message = message;
    message["url"] = json!("about:blank");
    let (reply, mut answer) = WorldReply::channel();
    let body = message.to_string();
    engine_event(view, EngineEvent::World { body, reply }, cx);
    answer
        .try_recv()
        .ok()
        .flatten()
        .unwrap_or_else(|| Err("no answer".into()))
}

/// An agent's Esc while picking, with mesurer mounted, goes to the page first (here, with no
/// page, it waits in the queue and picking stays on). What the page says decides: a key mesurer
/// took leaves picking on; one no one took stops it, as the user's own Esc does.
#[gpui_kit::test]
fn an_agents_esc_goes_to_mesurer_first(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    cx.update(|_, cx| {
        view.update(cx, |b, cx| {
            b.toggle_panel(Panel::Annotate, cx);
            b.mesurer.mounted = true;
        })
    });
    let mut answer = cx.update(|_, cx| {
        view.update(cx, |b, cx| {
            b.apply(
                BrowserOp::Press {
                    key: "Escape".into(),
                },
                cx,
            )
        })
    });
    cx.run_until_parked();
    assert_eq!(answer.try_recv().ok().flatten(), None, "the page has it");
    let (pick, queued) = cx.update(|_, cx| {
        let b = view.read(cx);
        (
            b.pick,
            b.queued
                .iter()
                .map(|(op, _)| op.clone())
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(pick, PickMode::Annotate);
    assert_eq!(
        queued,
        vec![BrowserOp::Press {
            key: "Escape".into()
        }]
    );

    let answered = |prevented: bool, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            view.update(cx, |b, cx| {
                let pressed = Ok(json!({"pressed": "Escape", "prevented": prevented}));
                let answer = b.escape_answered("Escape", pressed, cx).unwrap();
                (answer, b.pick)
            })
        })
    };
    let (taken, pick) = answered(true, cx);
    assert_eq!(pick, PickMode::Annotate, "mesurer took it");
    assert_eq!(taken["taken_by_page"], true);
    assert_eq!(taken["picking"], "annotate");
    let (free, pick) = answered(false, cx);
    assert_eq!(pick, PickMode::Off, "no one took it");
    assert_eq!(
        (
            free["taken_by_page"].clone(),
            free["picking"].clone(),
            free["panel"].clone()
        ),
        (json!(false), json!("off"), json!("annotate"))
    );
}

/// mesurer's links: an https one opens in the default browser (kept instead under
/// `SAWIT_E2E_RUN`), never in the tab; any other is refused.
#[gpui_kit::test]
fn mesurers_https_links_open_in_the_default_browser(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let link = |url: &str, cx: &mut VisualTestContext| {
        post(
            &view,
            json!({"kind": "link.open", "payload": {"url": url}}),
            cx,
        )
    };
    assert_eq!(
        link("https://mesurer.dev/", cx),
        Ok(json!({"opened": "https://mesurer.dev/"}).to_string())
    );
    if !crate::browser::mesurer::e2e_run() {
        assert_eq!(cx.opened_url().as_deref(), Some("https://mesurer.dev/"));
    }
    for refused in [
        "http://mesurer.dev",
        "javascript:alert(1)",
        "https://",
        "file:///etc",
    ] {
        assert!(link(refused, cx).is_err(), "{refused}");
    }
    let last = cx.update(|_, cx| view.read(cx).mesurer.keys.last_link.clone());
    assert_eq!(last.as_deref(), Some("https://mesurer.dev/"));
    assert_eq!(cx.update(|_, cx| view.read(cx).url.clone()), "about:blank");
}

/// `ui.state` moves the summary's tool and chrome at once, and the panel with them.
#[gpui_kit::test]
fn ui_state_moves_the_panels_tool(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    cx.update(|_, cx| view.update(cx, |b, _| b.mesurer.mounted = true));
    let state = json!({"tool": "guides", "minimized": false, "xray": true});
    let replied = post(&view, json!({"kind": "ui.state", "payload": state}), cx);
    assert_eq!(replied, Ok("null".into()));
    let summary = cx.update(|_, cx| view.read(cx).annotate.read(cx).mesurer_summary().cloned());
    let summary = summary.unwrap();
    assert_eq!((summary.tool.as_str(), summary.xray), ("guides", true));
}

/// The status line names mesurer's tool and how to leave it, and asks for a click otherwise;
/// the "Page tools" line counts the page's markup, and offers Show tools while mesurer is
/// minimized.
#[gpui_kit::test]
fn the_panel_names_mesurers_tool_and_counts_its_markup(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let panel = cx.update(|_, cx| view.read(cx).annotate.clone());
    let show = |summary: Option<Summary>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            panel.update(cx, |p, cx| {
                p.set_picking(Picking::On, cx);
                p.set_mesurer_summary(summary, cx);
                (p.status_text().to_string(), p.page_tools())
            })
        })
    };
    let summary = |tool: &str| Summary {
        tool: tool.into(),
        enabled: true,
        ..Summary::default()
    };
    let note = "Click an element to add a note".to_string();
    assert_eq!(show(None, cx), (note.clone(), None));
    assert_eq!(show(Some(summary("none")), cx), (note.clone(), None));
    assert_eq!(show(Some(summary("guides")), cx).0, "Guides · Esc to leave");
    assert_eq!(
        show(Some(summary("select")), cx).0,
        "Inspect · Esc to leave"
    );
    let marked = Summary {
        guides: 2,
        layout_guides: 1,
        comments_open: 1,
        xray: true,
        ..summary("xray")
    };
    assert_eq!(
        show(Some(marked.clone()), cx),
        (
            note.clone(),
            Some((
                "1 comment · 2 guides · 1 layout guide · X-ray on".into(),
                false
            ))
        )
    );
    let hidden = Summary {
        minimized: true,
        ..summary("guides")
    };
    assert_eq!(
        show(Some(hidden), cx),
        (note, Some(("Tools hidden".into(), true))),
        "a minimized mesurer's tool isn't on"
    );
}

/// Show tools (the panel's event) applies the page's workspace again with mesurer shown, and
/// keeps it so: the summary follows.
#[gpui_kit::test]
fn show_tools_brings_a_minimized_mesurer_back(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    cx.update(|_, cx| {
        view.update(cx, |b, cx| {
            b.mesurer.mounted = true;
            let snapshot =
                json!({"settings": {}, "workspace": {"minimized": true, "toolMode": "none"}});
            b.mesurer.store.loaded("about", &snapshot);
            b.mesurer_summary_changed(cx);
        })
    });
    assert!(cx.update(|_, cx| view.read(cx).mesurer.summary.minimized));
    let panel = cx.update(|_, cx| view.read(cx).annotate.clone());
    cx.update(|_, cx| panel.update(cx, |_, cx| cx.emit(ShowMesurerTools)));
    cx.run_until_parked();
    let (summary, held) = cx.update(|_, cx| {
        let b = view.read(cx);
        (
            b.mesurer.summary.clone(),
            b.mesurer.store.workspace("about").cloned(),
        )
    });
    assert!(!summary.minimized);
    assert_eq!(held.unwrap()["minimized"], false);
}
