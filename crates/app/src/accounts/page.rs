//! The Accounts page (design: `docs/design/accounts.sawit`): a header with the page's summary,
//! a vertical tab list grouped by what the tabs are about (it folds to icons in a narrow
//! window), the notices, then the tab on show. The Accounts tab lists the accounts on the left
//! ([`super::list`]) and shows the chosen one on the right ([`super::detail`]).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::Account;

use super::rows;
use super::{AccountsModel, global};
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

/// The tab list's groups, in order: a label and its tabs.
pub const GROUPS: &[(&str, &[Tab])] = &[
    ("Accounts", &[Tab::Accounts, Tab::Usage]),
    ("Claude Code", &[Tab::StatusLine]),
    ("Setup", &[Tab::Diagnostics, Tab::Settings]),
];

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

    pub fn icon(self) -> Lucide {
        match self {
            Tab::Accounts => Lucide::Users,
            Tab::Usage => Lucide::ChartColumn,
            Tab::StatusLine => Lucide::SquareTerminal,
            Tab::Diagnostics => Lucide::Stethoscope,
            Tab::Settings => Lucide::Settings,
        }
    }

    /// The tab `delta` places on in the list's order, clamped to the ends (↑ and ↓).
    pub fn step(self, delta: isize) -> Tab {
        let ix = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0) as isize;
        Tab::ALL[(ix + delta).clamp(0, Tab::ALL.len() as isize - 1) as usize]
    }
}

/// Below this window width the tab list shows icons only (their names in tooltips and to
/// VoiceOver), and the account list drops its meters.
pub const NARROW: Pixels = px(1000.);

pub struct AccountsPage {
    pub(super) model: Entity<AccountsModel>,
    pub(super) selected: Option<String>,
    pub(super) new_name: Entity<InputState>,
    pub(super) rename: Entity<InputState>,
    pub(super) renaming: bool,
    /// The Add Account panel at the top of the list is open.
    pub(super) adding: bool,
    pub(super) code: Entity<InputState>,
    pub(super) tab: Tab,
    /// The Usage tab's period, in seconds.
    pub(super) period: i64,
    /// The status line layout being edited (the one in use until changed).
    pub(super) layout: Option<sawit_core::agent_accounts::statusline::Config>,
    focus: FocusHandle,
    /// The tab list and the account list each take the keyboard (↑ and ↓ move in them).
    pub(super) nav_focus: FocusHandle,
    pub(super) list_focus: FocusHandle,
    _observe: Subscription,
    _inputs: Vec<Subscription>,
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

/// The header's summary: "4 accounts · 1 active · next reset in 2h 14m" (the soonest reset of
/// a window with usage in it), or "No accounts".
pub fn summary(m: &AccountsModel, now: f64) -> String {
    let list = m.list();
    if list.is_empty() {
        return "No accounts".into();
    }
    let mut parts = vec![match list.len() {
        1 => "1 account".to_string(),
        n => format!("{n} accounts"),
    }];
    if m.active().is_some() {
        parts.push("1 active".into());
    }
    let next = list
        .iter()
        .filter_map(|a| m.usage.get(&a.id))
        .flat_map(|s| [s.five_hour, s.seven_day])
        .filter_map(|w| rows::current(w, now))
        .min_by(|a, b| a.resets_at.total_cmp(&b.resets_at));
    if let Some(w) = next {
        parts.push(format!("next {}", rows::resets_in(&w, now)));
    }
    parts.join(" · ")
}

impl AccountsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = global(cx);
        let input = |p: &str, window: &mut Window, cx: &mut Context<Self>| {
            let p = p.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(p))
        };
        let rename = input("work", window, cx);
        let new_name = input("work", window, cx);
        // Enter in the rename field saves the name; Enter in the new account's adds it.
        let subs = vec![
            cx.subscribe(&rename, |this: &mut Self, input, e: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = e
                    && let Some(id) = this.selected(this.model.read(cx)).map(|a| a.id.clone())
                {
                    let name = input.read(cx).value().to_string();
                    this.model.update(cx, |m, cx| m.rename(&id, &name, cx));
                    this.renaming = false;
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &new_name,
                window,
                |this: &mut Self, _, e: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = e {
                        this.add(window, cx);
                    }
                },
            ),
        ];
        Self {
            _observe: cx.observe(&model, |_, _, cx| cx.notify()),
            _inputs: subs,
            selected: model.read(cx).list().first().map(|a| a.id.clone()),
            model,
            new_name,
            rename,
            renaming: false,
            adding: false,
            tab: Tab::Accounts,
            period: 7 * 86_400,
            layout: None,
            code: input("Paste the code from the sign-in page", window, cx),
            focus: cx.focus_handle(),
            nav_focus: cx.focus_handle().tab_stop(true),
            list_focus: cx.focus_handle().tab_stop(true),
        }
    }

    pub(super) fn selected<'a>(&self, m: &'a AccountsModel) -> Option<&'a Account> {
        self.selected
            .as_deref()
            .and_then(|id| m.accounts.get(id))
            .or_else(|| m.list().first().copied())
    }

    /// Selects the account `delta` rows on from the selected one (↑ and ↓ in the list).
    pub(super) fn select_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let ids: Vec<String> = m.list().iter().map(|a| a.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let at = self
            .selected(m)
            .and_then(|a| ids.iter().position(|id| *id == a.id))
            .unwrap_or(0) as isize;
        let next = (at + delta).clamp(0, ids.len() as isize - 1) as usize;
        self.selected = Some(ids[next].clone());
        self.renaming = false;
        cx.notify();
    }

    pub(super) fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.new_name.read(cx).value().to_string();
        if sawit_core::agent_accounts::kebab(&name).is_empty() {
            return;
        }
        let id = self.model.update(cx, |m, cx| m.add(&name, None, cx));
        if let Some(id) = id {
            self.selected = Some(id.clone());
            self.adding = false;
            self.new_name
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.model.update(cx, |m, cx| m.start_login(&id, cx));
        }
    }

    /// Opens a terminal running `claude` as `a`.
    pub(super) fn open_terminal(&mut self, a: &Account, cx: &mut Context<Self>) {
        let (command, env) = terminal_command(a);
        cx.emit(AccountsEvent::OpenTerminal {
            title: format!("claude · {}", a.name),
            command,
            env,
        });
    }

    pub(super) fn start_rename(
        &mut self,
        a: &Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = a.name.clone();
        self.selected = Some(a.id.clone());
        self.rename
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.renaming = true;
        self.rename.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn confirm_remove(
        &mut self,
        a: Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = crate::confirm::Confirm::new(
            format!("Remove account {} from Sawit?", a.name),
            "Remove Account",
        )
        .thing(a.name.clone())
        .detail(format!(
            "Sawit forgets the account{}. {} and its sign-in stay; Adopt Existing brings it back.",
            if a.tracking {
                " and gives its status line back"
            } else {
                ""
            },
            a.dir.display()
        ))
        .ask(window, cx);
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await {
                model.update(cx, |m, cx| m.remove(&a.id, cx));
            }
        })
        .detach();
    }

    /// Shows tab `t`, reading what it needs.
    pub(super) fn show_tab(&mut self, t: Tab, cx: &mut Context<Self>) {
        self.tab = t;
        if t == Tab::Usage {
            let period = self.period;
            self.model
                .update(cx, |m, cx| m.refresh_analytics(period, cx));
        }
        cx.notify();
    }

    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let m = self.model.read(cx);
        let summary = summary(m, sawit_core::github::now() as f64);
        h_flex()
            .flex_none()
            .h(px(52.))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Icon::new(Lucide::Users)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().text_base().font_semibold().child("Accounts"))
            .child(
                div()
                    .id("accounts-summary")
                    .role(Role::Label)
                    .aria_label(summary.clone())
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
            )
    }

    /// The tab list: grouped, with icons; icons only when `narrow`. ↑ and ↓ move between tabs
    /// while it has the keyboard, and its selected tab shows a ring then.
    fn render_nav(
        &self,
        narrow: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.nav_focus.is_focused(window);
        let current = self.tab;
        let muted = cx.theme().muted_foreground;
        let mut items: Vec<AnyElement> = Vec::new();
        for (gi, (group, tabs)) in GROUPS.iter().enumerate() {
            if narrow {
                if gi > 0 {
                    items.push(
                        div()
                            .w(px(24.))
                            .h(px(1.))
                            .my_1p5()
                            .bg(cx.theme().border)
                            .into_any_element(),
                    );
                }
            } else {
                items.push(
                    div()
                        .px_2()
                        .pt(px(if gi == 0 { 0. } else { 12. }))
                        .pb_1()
                        .text_xs()
                        .font_semibold()
                        .text_color(muted)
                        .child(*group)
                        .into_any_element(),
                );
            }
            for &t in tabs.iter() {
                let selected = t == current;
                let label = t.label();
                items.push(
                    h_flex()
                        .id(SharedString::from(format!("accounts-tab-{label}")))
                        .debug_selector(move || format!("accounts-tab-{label}"))
                        .role(Role::Tab)
                        .aria_label(label)
                        .aria_selected(selected)
                        .flex_none()
                        .h(px(28.))
                        .gap_2()
                        .rounded(cx.theme().radius)
                        .cursor_pointer()
                        .text_sm()
                        .border_2()
                        .border_color(transparent_black())
                        .map(|el| {
                            if narrow {
                                el.w(px(32.)).justify_center()
                            } else {
                                el.w_full().px_1p5()
                            }
                        })
                        .when(selected, |el| el.bg(cx.theme().list_active).font_medium())
                        .when(selected && focused, |el| el.border_color(cx.theme().ring))
                        .when(!selected, |el| el.hover(|el| el.bg(cx.theme().secondary)))
                        .child(Icon::new(t.icon()).small().text_color(if selected {
                            cx.theme().primary
                        } else {
                            muted
                        }))
                        .when(!narrow, |el| el.child(label))
                        .when(narrow, |el| {
                            el.tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.nav_focus.focus(window, cx);
                            this.show_tab(t, cx);
                        }))
                        .into_any_element(),
                );
            }
        }
        v_flex()
            .id("accounts-nav")
            .role(Role::TabList)
            .aria_label("Account Sections")
            .track_focus(&self.nav_focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.modifiers.modified() {
                    return;
                }
                let delta = match e.keystroke.key.as_str() {
                    "up" => -1,
                    "down" => 1,
                    _ => return,
                };
                let next = this.tab.step(delta);
                this.show_tab(next, cx);
                cx.stop_propagation();
            }))
            .flex_none()
            .h_full()
            .gap_0p5()
            .py_3()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .map(|el| {
                if narrow {
                    el.w(px(48.)).items_center()
                } else {
                    el.w(px(188.)).px_2()
                }
            })
            .children(items)
    }
}

impl Render for AccountsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < NARROW;
        let (error, notice) = {
            let m = self.model.read(cx);
            (m.error.clone(), m.notice.clone())
        };
        let body = match self.tab {
            Tab::Accounts => {
                let offer = self.render_import_offer(cx);
                let list = self.render_list(narrow, window, cx);
                let detail = self.render_detail(narrow, window, cx);
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .children(offer)
                    .child(h_flex().flex_1().min_h_0().child(list).child(detail))
                    .into_any_element()
            }
            Tab::Usage => self.render_usage(cx),
            Tab::StatusLine => self.render_statusline(cx),
            Tab::Diagnostics => self.render_diagnostics(cx),
            Tab::Settings => self.render_settings(cx),
        };
        let nav = self.render_nav(narrow, window, cx);
        v_flex()
            .id("accounts-page")
            .track_focus(&self.focus)
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_header(cx))
            .child(
                h_flex().flex_1().min_h_0().child(nav).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
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
                        .child(div().flex_1().min_h_0().flex().child(body)),
                ),
            )
    }
}
