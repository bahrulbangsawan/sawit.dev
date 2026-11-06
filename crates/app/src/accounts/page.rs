//! The Accounts page: Claude Code's accounts on the left (colour, name, who it is, 5h/7d), the
//! chosen one on the right: Set Active, Open in Terminal, Sign In, usage tracking with Check Usage
//! Now, Rename and Remove. Add Account and Adopt Existing sit under the list.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::{self as aa, Account, tracking};

use super::rows::{self, swatch, window_bar};
use super::{AccountsModel, SignIn, global};
use crate::states::Banner;

/// What the page asks the window to do.
pub enum AccountsEvent {
    /// A new terminal tab running `claude` as the account.
    OpenTerminal {
        title: String,
        command: String,
        env: Vec<(String, String)>,
    },
}

impl EventEmitter<AccountsEvent> for AccountsPage {}

/// The page's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Accounts,
    Usage,
    StatusLine,
    Diagnostics,
    Settings,
}

impl Tab {
    pub const ALL: &[Tab] = &[
        Tab::Accounts,
        Tab::Usage,
        Tab::StatusLine,
        Tab::Diagnostics,
        Tab::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Accounts => "Accounts",
            Tab::Usage => "Usage",
            Tab::StatusLine => "Status Line",
            Tab::Diagnostics => "Diagnostics",
            Tab::Settings => "Settings",
        }
    }
}

pub struct AccountsPage {
    pub(super) model: Entity<AccountsModel>,
    pub(super) selected: Option<String>,
    pub(super) new_name: Entity<InputState>,
    rename: Entity<InputState>,
    pub(super) renaming: bool,
    pub(super) code: Entity<InputState>,
    tab: Tab,
    /// The Usage tab's period, in seconds.
    pub(super) period: i64,
    /// The status line layout being edited (the one in use until changed).
    pub(super) layout: Option<sawit_core::agent_accounts::statusline::Config>,
    focus: FocusHandle,
    _observe: Subscription,
}

impl Focusable for AccountsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The command a terminal runs for `account`: the default one with `CLAUDE_CONFIG_DIR` unset
/// (so the shell block can't send it elsewhere), the others with it set in the tab's environment.
pub fn terminal_command(account: &Account) -> (String, Vec<(String, String)>) {
    match account.env_value() {
        Some(dir) => (
            "command claude".into(),
            vec![(
                account.kind.profile_var().into(),
                dir.to_string_lossy().to_string(),
            )],
        ),
        None => ("unset CLAUDE_CONFIG_DIR; command claude".into(), Vec::new()),
    }
}

impl AccountsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = global(cx);
        let input = |p: &str, window: &mut Window, cx: &mut Context<Self>| {
            let p = p.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(p))
        };
        Self {
            _observe: cx.observe(&model, |_, _, cx| cx.notify()),
            selected: model.read(cx).list().first().map(|a| a.id.clone()),
            model,
            new_name: input("New account name, like work", window, cx),
            rename: input("Name", window, cx),
            renaming: false,
            tab: Tab::Accounts,
            period: 7 * 86_400,
            layout: None,
            code: input("Paste the code from the sign-in page", window, cx),
            focus: cx.focus_handle(),
        }
    }

    pub(super) fn selected<'a>(&self, m: &'a AccountsModel) -> Option<&'a Account> {
        self.selected
            .as_deref()
            .and_then(|id| m.accounts.get(id))
            .or_else(|| m.list().first().copied())
    }

    pub(super) fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.new_name.read(cx).value().to_string();
        if aa::kebab(&name).is_empty() {
            return;
        }
        let id = self.model.update(cx, |m, cx| m.add(&name, None, cx));
        if let Some(id) = id {
            self.selected = Some(id.clone());
            self.new_name
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.model.update(cx, |m, cx| m.start_login(&id, cx));
        }
    }

    fn confirm_remove(&mut self, a: Account, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Remove “{}” from Sawit?", a.name),
            Some(&format!(
                "Sawit forgets the account{}. {} and its sign-in stay; Adopt Existing brings it back.",
                if a.tracking { " and gives its status line back" } else { "" },
                a.dir.display()
            )),
            &["Remove", "Cancel"],
            cx,
        );
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                model.update(cx, |m, cx| m.remove(&a.id, cx));
            }
        })
        .detach();
    }

    fn render_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let Some(a) = self.selected(m).cloned() else {
            return div()
                .p_6()
                .text_color(cx.theme().muted_foreground)
                .child("No Claude Code accounts yet. Add one, or adopt a ~/.claude-* folder.")
                .into_any_element();
        };
        let now = sawit_core::github::now() as f64;
        let is_active = m.active().map(|x| x.id.clone()) == Some(a.id.clone());
        let sign_in = m.sign_in.get(&a.id).cloned().unwrap_or(SignIn::Unknown);
        let plan = m.plan.get(&a.id).cloned();
        let snapshot = m.usage.get(&a.id).cloned();
        let login = m.login.as_ref().filter(|l| l.account == a.id).map(|l| {
            (
                l.phase.clone(),
                l.manual_url.clone(),
                l.callback_url.clone(),
                l.left(),
            )
        });
        let checking = m.checking_usage.as_deref() == Some(a.id.as_str());
        let muted = cx.theme().muted_foreground;
        let who = match &sign_in {
            SignIn::Known(s) if s.signed_in => s.identity().unwrap_or_else(|| "Signed in".into()),
            SignIn::Known(_) => "Signed out".into(),
            SignIn::Checking => "Checking the sign-in…".into(),
            SignIn::Failed(e) => e.clone(),
            SignIn::Unknown => String::new(),
        };
        let signed_out = matches!(&sign_in, SignIn::Known(s) if !s.signed_in);
        let (id1, id2, id3) = (a.id.clone(), a.id.clone(), a.id.clone());
        let header = h_flex()
            .gap_3()
            .items_center()
            .child(swatch(&a.color, 28.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(if self.renaming {
                        h_flex()
                            .gap_1()
                            .child(div().w(px(220.)).child(Input::new(&self.rename).small()))
                            .child(
                                Button::new("account-rename-save")
                                    .small()
                                    .label("Rename")
                                    .on_click(cx.listener({
                                        let id = a.id.clone();
                                        move |this, _, _, cx| {
                                            let name = this.rename.read(cx).value().to_string();
                                            this.model.update(cx, |m, cx| m.rename(&id, &name, cx));
                                            this.renaming = false;
                                        }
                                    })),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .text_xl()
                            .font_semibold()
                            .truncate()
                            .child(a.name.clone())
                            .into_any_element()
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .truncate()
                            .child(match &plan {
                                Some(p) if !who.is_empty() => format!("{who} · {p}"),
                                _ => who.clone(),
                            }),
                    ),
            )
            .child(if is_active {
                Button::new("account-active")
                    .small()
                    .ghost()
                    .icon(Lucide::Check)
                    .label("Active")
                    .disabled(true)
            } else {
                Button::new("account-set-active")
                    .small()
                    .primary()
                    .label("Set Active")
                    .tooltip("New Claude Code Sessions Use This Account")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.update(cx, |m, cx| m.set_active(&id1, cx))
                    }))
            })
            .child(
                Button::new("account-open-terminal")
                    .small()
                    .outline()
                    .icon(Lucide::SquareTerminal)
                    .label("Open in Terminal")
                    .on_click(cx.listener({
                        let a = a.clone();
                        move |_, _, _, cx| {
                            let (command, env) = terminal_command(&a);
                            cx.emit(AccountsEvent::OpenTerminal {
                                title: format!("claude · {}", a.name),
                                command,
                                env,
                            });
                        }
                    })),
            )
            .child(
                Button::new("account-more")
                    .small()
                    .ghost()
                    .icon(Lucide::Ellipsis)
                    .tooltip("More")
                    .dropdown_menu({
                        let a = a.clone();
                        let page = cx.entity();
                        move |menu, _, _| {
                            let (p1, p2, p3, a1, a2) = (
                                page.clone(),
                                page.clone(),
                                page.clone(),
                                a.clone(),
                                a.clone(),
                            );
                            let id = a.id.clone();
                            menu.item(PopupMenuItem::new("Rename…").on_click(
                                move |_, window, cx| {
                                    p1.update(cx, |this, cx| {
                                        let name = a1.name.clone();
                                        this.rename
                                            .update(cx, |s, cx| s.set_value(name, window, cx));
                                        this.renaming = true;
                                        cx.notify();
                                    })
                                },
                            ))
                            .item(PopupMenuItem::new("Check Sign-In Again").on_click(
                                move |_, _, cx| {
                                    p3.update(cx, |this, cx| {
                                        this.model.update(cx, |m, cx| m.check(&id, cx))
                                    })
                                },
                            ))
                            .separator()
                            .item(
                                PopupMenuItem::new("Remove from Sawit…").on_click(
                                    move |_, window, cx| {
                                        p2.update(cx, |this, cx| {
                                            this.confirm_remove(a2.clone(), window, cx)
                                        })
                                    },
                                ),
                            )
                        }
                    }),
            );
        let state = tracking::state(&a);
        let usage = v_flex()
            .gap_2()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(div().text_sm().font_semibold().child("Usage"))
            .map(|el| match state {
                tracking::Tracking::Off => el
                    .child(div().text_sm().text_color(muted).child("Usage tracking is off. Turned on, the account's status line reports its 5-hour and 7-day windows to Sawit."))
                    .child(Button::new("account-track").small().outline().label("Turn On Tracking…").on_click(cx.listener({
                        let a = a.clone();
                        move |this, _, window, cx| this.confirm_tracking(a.clone(), window, cx)
                    }))),
                tracking::Tracking::Overridden => el
                    .child(div().text_sm().text_color(cx.theme().warning).child("Overridden: this account's statusLine no longer runs Sawit, so no usage arrives."))
                    .child(Button::new("account-retrack").small().outline().label("Re-enable…").on_click(cx.listener({
                        let a = a.clone();
                        move |this, _, window, cx| this.confirm_tracking(a.clone(), window, cx)
                    }))),
                tracking::Tracking::On => {
                    let (when, stale) = snapshot
                        .as_ref()
                        .and_then(|s| rows::observed(s, now))
                        .unwrap_or_else(|| ("No data yet: start a claude session with this account and send one message.".into(), false));
                    el.child(window_bar("5-Hour", snapshot.as_ref().and_then(|s| s.five_hour), stale, now, cx))
                        .child(window_bar("7-Day", snapshot.as_ref().and_then(|s| s.seven_day), stale, now, cx))
                        .child(div().text_xs().text_color(muted).child(if stale {
                            format!("{when}. Usage from claude.ai or other devices after this time isn't included.")
                        } else {
                            when
                        }))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(Button::new("account-check-usage").small().outline().icon(Lucide::RefreshCw).label(if checking { "Checking…" } else { "Check Usage Now" }).loading(checking).disabled(checking || signed_out).tooltip("Claude Code Sends One Tiny Request as This Account (a Few Hundred Tokens)").on_click(
                                    cx.listener(move |this, _, _, cx| this.model.update(cx, |m, cx| m.check_usage(&id2, cx))),
                                ))
                                .child(Button::new("account-untrack").small().ghost().label("Turn Off Tracking").on_click(
                                    cx.listener(move |this, _, _, cx| this.model.update(cx, |m, cx| m.set_tracking(&id3, false, cx))),
                                )),
                        )
                }
            });
        v_flex()
            .id("account-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p_6()
            .gap_4()
            .child(header)
            .when(signed_out || login.is_some(), |el| {
                el.child(self.render_login(&a, login, window, cx))
            })
            .child(usage)
            .child(div().text_xs().text_color(muted).child(format!(
                "Folder: {}{}",
                a.dir.display(),
                if a.is_default {
                    " (runs with CLAUDE_CONFIG_DIR unset)"
                } else {
                    ""
                }
            )))
            .into_any_element()
    }
}

impl Render for AccountsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (error, notice) = {
            let m = self.model.read(cx);
            (m.error.clone(), m.notice.clone())
        };
        let body = match self.tab {
            Tab::Accounts => {
                let list = self.render_list(cx);
                let detail = self.render_detail(window, cx);
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(list)
                    .child(detail)
                    .into_any_element()
            }
            Tab::Usage => self.render_usage(cx),
            Tab::StatusLine => self.render_statusline(cx),
            Tab::Diagnostics => self.render_diagnostics(cx),
            Tab::Settings => self.render_settings(cx),
        };
        let current = self.tab;
        let tabs = h_flex()
            .px_4()
            .pt_2()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(Tab::ALL.iter().map(|&t| {
                Button::new(SharedString::from(format!("accounts-tab-{}", t.label())))
                    .small()
                    .ghost()
                    .label(t.label())
                    .selected(t == current)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = t;
                        if t == Tab::Usage {
                            let period = this.period;
                            this.model
                                .update(cx, |m, cx| m.refresh_analytics(period, cx));
                        }
                        cx.notify();
                    }))
            }));
        v_flex()
            .id("accounts-page")
            .track_focus(&self.focus)
            .size_full()
            .bg(cx.theme().background)
            .when_some(error, |el, e| {
                el.child(Banner::error("accounts-error", e).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.model.update(cx, |m, cx| {
                            m.error = None;
                            cx.notify()
                        })
                    },
                )))
            })
            .when_some(notice, |el, n| {
                el.child(Banner::info("accounts-notice", n).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.model.update(cx, |m, cx| {
                            m.notice = None;
                            cx.notify()
                        })
                    },
                )))
            })
            .child(tabs)
            .child(div().flex_1().min_h_0().flex().child(body))
    }
}
