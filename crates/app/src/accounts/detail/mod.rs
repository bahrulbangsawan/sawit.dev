//! The Accounts tab's detail (design: `docs/design/accounts.sawit`), capped at 720 px: the
//! header (avatar, name renamed in place, `cpN`, plan, who it is, Active or Set Active, Open in
//! Terminal, ⋯), the sign-in card while signed out, the usage limits as two meter cards with
//! when they were read and Check Usage, and the details (folder with Reveal and Copy Path, how
//! it runs, the sessions running as it).

mod header;
mod limits;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::usage::Window as UsageWindow;

use super::SignIn;
use super::list::adopt_button;
use super::page::AccountsPage;
use super::rows;
use super::ui;

/// One window's card: what it is, "% used", the bar, and when it resets, in words and on the
/// clock. Past warning a card also carries an icon beside its colour.
fn meter_card(
    id: &'static str,
    title: &'static str,
    w: Option<UsageWindow>,
    stale: bool,
    now: f64,
    cx: &App,
) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let card = v_flex()
        .id(id)
        .flex_1()
        .min_w(px(200.))
        .px_4()
        .py_3p5()
        .gap_2()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().sidebar);
    let head = |icon: Option<Lucide>, color: Hsla| {
        h_flex()
            .child(div().text_sm().text_color(muted).child(title))
            .child(div().flex_1())
            .when_some(icon, |el, i| {
                el.child(Icon::new(i).small().text_color(color))
            })
    };
    match w {
        Some(w) if w.resets_at > now => {
            let used = w.used_percentage;
            let color = ui::meter_color(used, cx);
            let spoken = format!(
                "{title}: {} used, {}",
                rows::pct(&w),
                rows::resets_in(&w, now)
            );
            card.role(Role::Label)
                .aria_label(spoken.clone())
                .child(head(ui::level_icon(used), color))
                .child(
                    h_flex()
                        .items_baseline()
                        .gap_1p5()
                        .child(
                            div()
                                .text_size(px(28.))
                                .line_height(px(32.))
                                .font_medium()
                                .when(stale, |el| el.text_color(muted))
                                .child(rows::pct(&w)),
                        )
                        .child(div().text_sm().text_color(muted).child("used")),
                )
                .child(ui::chart_tip(
                    div()
                        .py_1()
                        .when(stale, |el| el.opacity(0.5))
                        .child(ui::track((used / 100.0) as f32, color, 6., cx)),
                    format!("detail-meter-{title}"),
                    spoken,
                    cx,
                ))
                .child(
                    h_flex()
                        .text_xs()
                        .text_color(muted)
                        .child(capitalized(&rows::resets_in(&w, now)))
                        .child(div().flex_1())
                        .child(sawit_core::date::when_label(w.resets_at as i64, now as i64)),
                )
        }
        // The window ended after the last reading: no bar, which would read as 0% used.
        Some(w) => card
            .role(Role::Label)
            .aria_label(format!("{title}: no data since the reset"))
            .child(head(Some(Lucide::Clock), muted))
            .child(div().text_sm().child(format!(
                "No data since the reset at {}",
                sawit_core::date::when_label(w.resets_at as i64, now as i64)
            )))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("It fills in with the next session's first message."),
            ),
        None => card
            .role(Role::Label)
            .aria_label(format!("{title}: no data yet"))
            .child(head(None, muted))
            .child(div().text_sm().child("No data yet"))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Start a claude session as this account and send one message."),
            ),
    }
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

impl AccountsPage {
    pub(super) fn render_detail(
        &mut self,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let m = self.model.read(cx);
        let Some(a) = self.selected(m).cloned() else {
            return self.render_no_accounts(cx);
        };
        let now = sawit_core::github::now() as f64;
        let list = m.list();
        let ix = list.iter().position(|x| x.id == a.id).unwrap_or(0);
        let count = list.len();
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
        let sessions: Vec<_> = m
            .sessions()
            .into_iter()
            .filter(|s| s.account == a.id)
            .collect();
        let others: Vec<(String, String)> = list
            .iter()
            .filter(|x| x.id != a.id)
            .map(|x| (x.id.clone(), x.name.clone()))
            .collect();
        let home = m.home.to_string_lossy().to_string();
        let who = match &sign_in {
            SignIn::Known(s) if s.signed_in => s.identity().unwrap_or_else(|| "Signed in".into()),
            SignIn::Known(_) => "Signed out".into(),
            SignIn::Checking => "Checking the sign-in…".into(),
            SignIn::Failed(e) => e.clone(),
            SignIn::Unknown => String::new(),
        };
        let signed_out = matches!(&sign_in, SignIn::Known(s) if !s.signed_in);
        let header = self.render_detail_header(
            &a,
            DetailHead {
                ix,
                count,
                is_active,
                plan,
                who,
            },
            window,
            cx,
        );
        let usage =
            self.render_limits(&a, snapshot.as_ref(), checking, signed_out, narrow, now, cx);
        let details = self.render_details(&a, &home, sessions, others, cx);
        div()
            .id("account-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .p_6()
                    .gap_6()
                    .child(header)
                    .when(signed_out || login.is_some(), |el| {
                        el.child(self.render_login(&a, login, window, cx))
                    })
                    .child(usage)
                    .child(details),
            )
            .into_any_element()
    }

    /// No accounts: what an account is, and both ways to get one.
    fn render_no_accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let adoptable = sawit_core::agent_accounts::discover(
            sawit_core::agent_accounts::AgentKind::ClaudeCode,
            &m.home,
        );
        div()
            .id("account-detail")
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                ui::empty_state(
                    Lucide::Users,
                    "No Claude Code accounts yet",
                    "An account is a Claude Code folder with its own sign-in. Add one and sign in, or adopt a ~/.claude-* folder you already have.",
                    cx,
                )
                .child(
                    h_flex()
                        .mt_2()
                        .gap_2()
                        .child(
                            Button::new("account-add-empty")
                                .small()
                                .primary()
                                .icon(Lucide::Plus)
                                .label("Add Account")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.adding = true;
                                    this.new_name.read(cx).focus_handle(cx).focus(window, cx);
                                    cx.notify();
                                })),
                        )
                        .when(!adoptable.is_empty(), |el| el.child(adopt_button(adoptable))),
                ),
            )
            .into_any_element()
    }
}

/// What the detail's header shows besides the account itself.
struct DetailHead {
    ix: usize,
    count: usize,
    is_active: bool,
    plan: Option<String>,
    who: String,
}
