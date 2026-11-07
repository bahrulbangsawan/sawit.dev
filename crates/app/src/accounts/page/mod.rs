//! The Accounts page (design: `docs/design/accounts.sawit`): a header with the page's summary,
//! a vertical tab list grouped by what the tabs are about (it folds to icons in a narrow
//! window), the notices, then the tab on show. The Accounts tab lists the accounts on the left
//! ([`super::list`]) and shows the chosen one on the right ([`super::detail`]).

mod render;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::agent_accounts::Account;

use super::rows;
use super::{AccountsModel, global};

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
    Overview,
    Detailed,
    Compare,
    Timeline,
    Analytics,
    Wallet,
    Accounts,
    Usage,
    StatusLine,
    Diagnostics,
    Settings,
}

/// The tab list's groups, in order: a label and its tabs.
pub const GROUPS: &[(&str, &[Tab])] = &[
    (
        "Pool",
        &[
            Tab::Overview,
            Tab::Detailed,
            Tab::Compare,
            Tab::Timeline,
            Tab::Analytics,
            Tab::Wallet,
        ],
    ),
    ("Accounts", &[Tab::Accounts, Tab::Usage]),
    ("Claude Code", &[Tab::StatusLine]),
    ("Setup", &[Tab::Diagnostics, Tab::Settings]),
];

impl Tab {
    pub const ALL: &[Tab] = &[
        Tab::Overview,
        Tab::Detailed,
        Tab::Compare,
        Tab::Timeline,
        Tab::Analytics,
        Tab::Wallet,
        Tab::Accounts,
        Tab::Usage,
        Tab::StatusLine,
        Tab::Diagnostics,
        Tab::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Detailed => "Detailed",
            Tab::Compare => "Compare",
            Tab::Timeline => "Timeline",
            Tab::Analytics => "Analytics",
            Tab::Wallet => "Wallet",
            Tab::Accounts => "Accounts",
            Tab::Usage => "Usage",
            Tab::StatusLine => "Status Line",
            Tab::Diagnostics => "Diagnostics",
            Tab::Settings => "Settings",
        }
    }

    pub fn icon(self) -> Lucide {
        match self {
            Tab::Overview => Lucide::LayoutDashboard,
            Tab::Detailed => Lucide::List,
            Tab::Compare => Lucide::GitCompare,
            Tab::Timeline => Lucide::CalendarClock,
            Tab::Analytics => Lucide::TrendingUp,
            Tab::Wallet => Lucide::Wallet,
            Tab::Accounts => Lucide::Users,
            Tab::Usage => Lucide::ChartColumn,
            Tab::StatusLine => Lucide::SquareTerminal,
            Tab::Diagnostics => Lucide::Stethoscope,
            Tab::Settings => Lucide::Settings,
        }
    }

    /// One of the pool's views, which read the pool when shown.
    pub fn is_pool(self) -> bool {
        matches!(
            self,
            Tab::Overview
                | Tab::Detailed
                | Tab::Compare
                | Tab::Timeline
                | Tab::Analytics
                | Tab::Wallet
        )
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
    /// The pool views' choices, and the wallet's fields.
    pub(super) pool_view: super::pool_tabs::PoolView,
    pub(super) price_input: Entity<InputState>,
    pub(super) day_input: Entity<InputState>,
    pub(super) topup_amount: Entity<InputState>,
    pub(super) topup_credits: Entity<InputState>,
    pub(super) topup_days: Entity<InputState>,
    pub(super) topup_note: Entity<InputState>,
    pub(super) topup_account: Option<String>,
    /// The agents Sawit sees in this page's window (its terminals, MCP clients).
    pub(super) agents_source: Option<AgentsSource>,
    _observe: Subscription,
    _inputs: Vec<Subscription>,
}

/// Where the Overview gets the window's agents from.
pub type AgentsSource = std::rc::Rc<dyn Fn(&App) -> Vec<super::SeenAgent>>;

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
            pool_view: Default::default(),
            price_input: input("200", window, cx),
            day_input: input("3", window, cx),
            topup_amount: input("10", window, cx),
            topup_credits: input("1000", window, cx),
            topup_days: input("30", window, cx),
            topup_note: input("Promo credits", window, cx),
            topup_account: None,
            agents_source: None,
        }
    }

    /// Gives the Overview the agents of the window this page is in.
    pub fn set_agents_source(&mut self, source: AgentsSource) {
        self.agents_source = Some(source);
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
        if t.is_pool() {
            self.model.update(cx, |m, cx| m.refresh_pool(cx));
        }
        cx.notify();
    }
}
