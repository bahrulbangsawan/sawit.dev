//! Who has the keyboard: ⇧⌘. and its tooltip, the address field, a press in the page, and the
//! workspace's keys once the tab is hidden.

use crate::browser;

/// The Annotate button's tooltip says the key the keymap gives Annotate: ⇧⌘. by default, the
/// user's own once they change it, and none once they take that out too.
#[gpui_kit::test]
fn the_annotate_tooltip_follows_the_keymap(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::{Action, KeyBinding, KeyBindingContextPredicate, Unbind, VisualTestContext};
    cx.update(browser::init);
    let cx = cx.add_empty_window();
    let tooltip = |cx: &mut VisualTestContext| {
        cx.update(|window, _| browser::toolbar::annotate_tooltip(window))
    };
    // The user's binding of `keys` to `action` in the browser, as the keymap file binds it.
    let bind = |keys: &str, action: Box<dyn Action>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let browser = KeyBindingContextPredicate::parse("Browser").ok();
            let context = browser.map(std::rc::Rc::new);
            let binding = KeyBinding::load(keys, action, context, false, None, cx).unwrap();
            cx.bind_keys([binding.with_meta(crate::keymap::USER)]);
        })
    };
    let off = || Box::new(Unbind("browser::ToggleAnnotate".into()));
    assert_eq!(tooltip(cx).as_ref(), "Annotate (⇧⌘.)");

    bind("cmd->", off(), cx);
    bind("ctrl-alt-a", Box::new(browser::ToggleAnnotate), cx);
    assert_eq!(tooltip(cx).as_ref(), "Annotate (⌃⌥A)");

    bind("ctrl-alt-a", off(), cx);
    assert_eq!(tooltip(cx).as_ref(), "Annotate");
}

/// ⇧⌘. toggles Annotate while Sawit's side of the tab has the keyboard: the Annotate and
/// Design buttons and a click in the side panel give it to the view, and every pick to the
/// new note's text box. A Mac reports ⇧⌘. as `cmd->` (gpui's macOS `parse_keystroke` turns
/// Shift off for a shifted symbol), so that is the key pressed here, in the workspace with
/// every key the workspace and the text boxes bind.
#[gpui_kit::test]
fn shift_cmd_period_toggles_annotate_from_sawits_side(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::input::{Input, InputState};
    use gpui_kit::prelude::*;
    use gpui_kit::{Entity, FocusHandle, Focusable as _, VisualTestContext, Window, div};

    /// The browser tab's key context in the workspace's, with a note's text box, counting
    /// the toggles it gets.
    struct BrowserSide {
        focus: FocusHandle,
        note: Entity<InputState>,
        toggles: usize,
    }
    impl Render for BrowserSide {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let browser = div()
                .key_context("Browser")
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, _: &browser::ToggleAnnotate, _, _| {
                    this.toggles += 1;
                }))
                .size_full()
                .child(Input::new(&self.note));
            div().key_context("Workspace").size_full().child(browser)
        }
    }

    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
    });
    let (view, cx) = cx.add_window_view(|window, cx| BrowserSide {
        focus: cx.focus_handle(),
        note: cx.new(|cx| InputState::new(window, cx)),
        toggles: 0,
    });
    let toggles = |cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).toggles);

    // The view has the keyboard (the toolbar's buttons, a click in the panel).
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("cmd->");
    assert_eq!(toggles(cx), 1, "⇧⌘. as a Mac sends it toggles Annotate");
    cx.simulate_keystrokes("cmd->");
    assert_eq!(toggles(cx), 2, "and toggles it back");

    // A note's text box has it (after a pick).
    let note = cx.update(|_, cx| view.read(cx).note.clone());
    cx.update(|window, cx| note.update(cx, |note, cx| note.focus(window, cx)));
    assert!(cx.update(|window, cx| note.read(cx).focus_handle(cx).is_focused(window)));
    cx.simulate_keystrokes("cmd->");
    assert_eq!(toggles(cx), 3, "from the note's text box too");
}

/// A browser tab's view in a window, not drawn (its WKWebView needs a real window), shown as
/// the workspace shows it.
fn browser_view(cx: &mut gpui_kit::VisualTestContext) -> gpui_kit::Entity<browser::BrowserView> {
    use gpui_kit::AppContext as _;
    cx.update(|window, cx| {
        let view = cx.new(|cx| browser::BrowserView::new(None, None, window, cx));
        view.update(cx, |view, _| view.visible = true);
        view
    })
}

/// Enter in the address field loads the page and gives it the keyboard: AppKit's, and
/// GPUI's, which goes to the browser's own view, so that the ⌘ keys GPUI sees first (⌘V, ⌘A,
/// ⌘Z) no longer act on the address field while the page has the keyboard.
#[gpui_kit::test]
fn enter_in_the_address_gives_the_page_the_keyboard(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::input::InputEvent;
    cx.update(|cx| {
        gpui_kit::init(cx);
        browser::init(cx);
    });
    let cx = cx.add_empty_window();
    let view = browser_view(cx);
    let address = cx.update(|_, cx| view.read(cx).address.clone());
    cx.update(|window, cx| {
        address.update(cx, |a, cx| {
            a.set_value("localhost:3000", window, cx);
            a.focus(window, cx);
        })
    });
    cx.update(|_, cx| {
        address.update(cx, |_, cx| {
            cx.emit(InputEvent::PressEnter {
                secondary: false,
                shift: false,
            })
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(view.read(cx).focus.is_focused(window));
        assert_eq!(view.read(cx).url, "http://localhost:3000");
    });
}

/// ⌘C goes to the page while the browser's view has GPUI's keyboard: no binding of Sawit's
/// takes it (gpui-kit's root copies its own text selection on ⌘C).
#[gpui_kit::test]
fn cmd_c_in_the_browser_is_left_to_the_page(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::{KeyContext, Keystroke, OperatingSystem};
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
    });
    cx.update(|cx| {
        let typed = [Keystroke::parse_with_os("cmd-c", OperatingSystem::Mac).unwrap()];
        let stack = ["Root", "Workspace", "Browser"].map(|c| KeyContext::parse(c).unwrap());
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, _) = keymap.bindings_for_input(&typed, &stack);
        let names: Vec<&str> = bindings.iter().map(|b| b.action().name()).collect();
        assert!(names.is_empty(), "{names:?}");
    });
}

/// The user's press in the page (the page agent's `pressed` message, sent for a real press
/// only) moves GPUI's keyboard to the browser's view, as AppKit gave the page its own: off
/// the address field, the terminal or Files, whose ⌘V, ⌘C and ⌘Z would otherwise act there.
#[gpui_kit::test]
fn a_press_in_the_page_moves_gpuis_keyboard_to_it(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        browser::init(cx);
    });
    let cx = cx.add_empty_window();
    let view = browser_view(cx);
    let address = cx.update(|_, cx| view.read(cx).address.clone());
    cx.update(|window, cx| address.update(cx, |a, cx| a.focus(window, cx)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.receive(
                browser::FromPage::Event(sawit_core::browser::PageEvent::Pressed),
                window,
                cx,
            )
        })
    });
    cx.update(|window, cx| assert!(view.read(cx).focus.is_focused(window)));
}

/// A press reported while the tab is hidden leaves GPUI's keyboard where the user has it (the
/// terminal, say): the user can't press in a hidden page, and a view that isn't drawn must
/// not take the keyboard: keys would go from the window's root, so that ⌘W, ⌘T and ⌘1 stop
/// working, until the workspace takes it back (`workspace::KeyboardHome`), and
/// then the terminal no longer has it.
#[gpui_kit::test]
fn a_press_in_a_hidden_page_leaves_the_keyboard(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        browser::init(cx);
    });
    let cx = cx.add_empty_window();
    let view = browser_view(cx);
    let elsewhere = cx.update(|_, cx| cx.focus_handle());
    cx.update(|window, cx| {
        view.update(cx, |view, _| view.visible = false);
        elsewhere.focus(window, cx);
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.receive(
                browser::FromPage::Event(sawit_core::browser::PageEvent::Pressed),
                window,
                cx,
            )
        })
    });
    cx.update(|window, cx| {
        assert!(
            elsewhere.is_focused(window),
            "a hidden page's press took the keyboard: the view has it = {}",
            view.read(cx).focus.is_focused(window)
        )
    });
}

/// Where the keyboard is when the browser's tab goes off screen.
#[derive(Clone, Copy, Debug)]
enum Keyboard {
    /// The workspace: a click in the page never moved GPUI's keyboard before KEYMAP-4.
    Workspace,
    /// The browser's view, after the user's press in the page.
    Page,
    /// The address field, while the user types an address.
    Address,
}

/// I3: the browser's tab often goes off screen without a click, after a press in the page
/// gave its view GPUI's keyboard (KEYMAP-4): ⇧⌘J hides the tab area, a page comes in front
/// without taking the keyboard (an agent's `app_open_page`), ⌃Tab, an agent's
/// `app_activate_tab`. A view that isn't drawn and still has the keyboard makes GPUI send
/// the keys from the window's root, gpui-kit's `Root`, past every workspace shortcut, until a
/// click: ⇧⌘J didn't bring the tab area back, ⌘W didn't close the page in front. The
/// workspace takes the keyboard back ([`crate::workspace::KeyboardHome`]), from the page and
/// the address field alike.
///
/// The workspace here is a stand-in, in a window as `cli::open_window` makes one (under
/// gpui-kit's `Root`): the workspace's keys and key context, its keyboard, `render`'s refocus
/// when nothing has the keyboard, its `KeyboardHome`, given back to as `render` does (never to
/// the browser's view), and the browser's tab, told whether it shows as `sync_browser` tells
/// it and drawn while it shows (its key context, its view's keyboard, the address field). The
/// webview itself needs a real window.
#[gpui_kit::test]
fn the_workspace_keys_work_once_the_browser_tab_is_hidden(cx: &mut gpui_kit::TestAppContext) {
    use crate::workspace::{CloseActiveTab, OpenMcp, ToggleCenter};
    use browser::{BrowserView, FromPage};
    use gpui_kit::component::input::Input;
    use gpui_kit::prelude::*;
    use gpui_kit::{
        AnyWindowHandle, Entity, FocusHandle, Focusable as _, TestAppContext, Window, div,
    };
    use sawit_core::browser::PageEvent;

    struct StandIn {
        focus: FocusHandle,
        browser: Entity<BrowserView>,
        /// The tab area shows: ⇧⌘J toggles it.
        area: bool,
        /// Another tab is in front of the browser's, and takes no keyboard, as a page an agent
        /// opens: ⌃⌘M opens it here, ⌘W closes it.
        other_tab: bool,
        toggles: usize,
        closes: usize,
        keyboard: crate::workspace::KeyboardHome<bool>,
    }
    impl Render for StandIn {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if window.focused(cx).is_none() {
                self.focus.focus(window, cx);
            }
            let shown = self.area && !self.other_tab;
            self.browser.update(cx, |b, _| b.set_visible(shown));
            let (view, address) = {
                let browser = self.browser.read(cx);
                (browser.focus.clone(), browser.address.clone())
            };
            self.keyboard
                .give_back(shown, |back, _| *back != view, window, cx);
            div()
                .key_context("Workspace")
                .track_focus(&self.focus)
                .size_full()
                .on_action(cx.listener(|this, _: &ToggleCenter, _, cx| {
                    this.area = !this.area;
                    this.toggles += 1;
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &OpenMcp, _, cx| {
                    this.other_tab = true;
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &CloseActiveTab, _, cx| {
                    this.other_tab = false;
                    this.closes += 1;
                    cx.notify();
                }))
                .when(shown, |el| {
                    el.child(
                        div()
                            .key_context("Browser")
                            .track_focus(&view)
                            .size_full()
                            .child(Input::new(&address)),
                    )
                })
        }
    }

    /// A window with the stand-in, and the keyboard put where `keyboard` says.
    fn open(keyboard: Keyboard, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<StandIn>) {
        let (window, workspace) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                let browser = cx.new(|cx| BrowserView::new(None, None, window, cx));
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    StandIn {
                        keyboard: crate::workspace::KeyboardHome::new(focus.clone(), window, cx),
                        focus,
                        browser,
                        area: true,
                        other_tab: false,
                        toggles: 0,
                        closes: 0,
                    }
                })
            })
            .unwrap()
        });
        cx.update_window(window, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        let browser = cx.update(|cx| workspace.read(cx).browser.clone());
        let address = cx.update(|cx| browser.read(cx).address.clone());
        cx.update_window(window, |_, window, cx| match keyboard {
            Keyboard::Workspace => {}
            Keyboard::Page => browser.update(cx, |b, cx| {
                b.receive(FromPage::Event(PageEvent::Pressed), window, cx)
            }),
            Keyboard::Address => address.update(cx, |a, cx| a.focus(window, cx)),
        })
        .unwrap();
        cx.run_until_parked();
        let placed = cx
            .update_window(window, |_, window, cx| {
                let has = |focus: &FocusHandle| focus.is_focused(window);
                match keyboard {
                    Keyboard::Workspace => has(&workspace.read(cx).focus),
                    Keyboard::Page => has(&browser.read(cx).focus),
                    Keyboard::Address => has(&address.read(cx).focus_handle(cx)),
                }
            })
            .unwrap();
        assert!(placed, "the {keyboard:?} doesn't have the keyboard");
        (window, workspace)
    }
    fn press(keys: &str, window: AnyWindowHandle, cx: &mut TestAppContext) {
        cx.simulate_keystrokes(window, keys);
        cx.run_until_parked();
    }

    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
    });
    let mut failed = Vec::new();
    for keyboard in [Keyboard::Workspace, Keyboard::Page, Keyboard::Address] {
        // ⇧⌘J three times: the tab area hides, shows and hides again. The keyboard is then
        // the workspace's, not the hidden browser's.
        let (window, workspace) = open(keyboard, cx);
        for _ in 0..3 {
            press("cmd-shift-j", window, cx);
        }
        let (toggles, area) = cx.update(|cx| (workspace.read(cx).toggles, workspace.read(cx).area));
        if (toggles, area) != (3, false) {
            failed.push(format!(
                "{keyboard:?}: ⇧⌘J pressed 3 times toggled the tab area {toggles} times"
            ));
        }
        let (workspace_has_it, browser_has_it) = cx
            .update_window(window, |_, window, cx| {
                let browser = workspace.read(cx).browser.read(cx);
                (
                    workspace.read(cx).focus.is_focused(window),
                    browser.focus.contains_focused(window, cx)
                        || browser.address.read(cx).focus_handle(cx).is_focused(window),
                )
            })
            .unwrap();
        if !workspace_has_it || browser_has_it {
            failed.push(format!(
                "{keyboard:?}: the workspace has the keyboard = {workspace_has_it}, \
                     the hidden browser = {browser_has_it}"
            ));
        }

        // A page that takes no keyboard comes in front of the browser (⌃⌘M here, as an agent's
        // `app_open_page`), and ⌘W closes it.
        let (window, workspace) = open(keyboard, cx);
        press("ctrl-cmd-m", window, cx);
        press("cmd-w", window, cx);
        let (closes, other_tab) =
            cx.update(|cx| (workspace.read(cx).closes, workspace.read(cx).other_tab));
        if (closes, other_tab) != (1, false) {
            failed.push(format!(
                "{keyboard:?}: ⌘W after ⌃⌘M closed {closes} pages, \
                     the MCP page still in front = {other_tab}"
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
