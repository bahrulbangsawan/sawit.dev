//! The Accounts page's Diagnostics tab: what Sawit checks of the accounts, the shell and the
//! agents, each with how it went and the fix.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::advice::{self, Level};

use super::AccountsModel;
use super::page::AccountsPage;
use super::ui::{self, Tone};

impl AccountsPage {
    pub(super) fn render_diagnostics(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let status = |id: &str| m.status(id).cloned();
        let exe = std::env::current_exe().ok();
        let shell = AccountsModel::login_shell();
        let env = |v: &str| std::env::var_os(v).is_some();
        let probe = advice::Probe {
            claude: m.claude.as_deref(),
            login_shell: &shell,
            env: &env,
            status: &status,
            exe: exe.as_deref(),
        };
        let checks = advice::diagnostics(&m.root, &m.home, &m.accounts, &probe);
        let count = |l: Level| checks.iter().filter(|c| c.level == l).count();
        let (pass, warn, fail, info) = (
            count(Level::Pass),
            count(Level::Warn),
            count(Level::Fail),
            count(Level::Info),
        );
        let report: String = checks
            .iter()
            .map(|c| {
                let mark = match c.level {
                    Level::Pass => "pass",
                    Level::Warn => "warning",
                    Level::Fail => "fail",
                    Level::Info => "note",
                };
                match &c.fix {
                    Some(f) => format!("[{mark}] {}: {} {f}", c.title, c.detail),
                    None => format!("[{mark}] {}: {}", c.title, c.detail),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let plural = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let summary = h_flex()
            .flex_wrap()
            .gap_2()
            .child(ui::pill(
                "diag-pass",
                Tone::Good,
                Some(Lucide::CircleCheck),
                plural(pass, "passed", "passed"),
                cx,
            ))
            .when(warn > 0, |el| {
                el.child(ui::pill(
                    "diag-warn",
                    Tone::Warn,
                    Some(Lucide::TriangleAlert),
                    plural(warn, "warning", "warnings"),
                    cx,
                ))
            })
            .when(fail > 0, |el| {
                el.child(ui::pill(
                    "diag-fail",
                    Tone::Bad,
                    Some(Lucide::CircleX),
                    plural(fail, "failed", "failed"),
                    cx,
                ))
            })
            .when(info > 0, |el| {
                el.child(ui::pill(
                    "diag-info",
                    Tone::Neutral,
                    Some(Lucide::Info),
                    plural(info, "note", "notes"),
                    cx,
                ))
            })
            .child(div().flex_1())
            .child(
                Button::new("diag-again")
                    .xsmall()
                    .outline()
                    .icon(Lucide::RefreshCw)
                    .label("Run Again")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| m.check_all(cx));
                        cx.notify();
                    })),
            )
            .child(
                Button::new("diag-copy")
                    .xsmall()
                    .outline()
                    .icon(Lucide::Copy)
                    .label("Copy Report")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(report.clone()))
                    }),
            );
        let rows: Vec<AnyElement> = checks
            .into_iter()
            .enumerate()
            .map(|(ix, c)| {
                // GPUI shows assistive technology no plain text: each check says itself.
                let word = match c.level {
                    Level::Pass => "Passed",
                    Level::Warn => "Warning",
                    Level::Fail => "Failed",
                    Level::Info => "Note",
                };
                let spoken = match &c.fix {
                    Some(f) => format!("{word}: {}: {} {f}", c.title, c.detail),
                    None => format!("{word}: {}: {}", c.title, c.detail),
                };
                let (icon, color) = match c.level {
                    Level::Pass => (Lucide::CircleCheck, cx.theme().success),
                    Level::Warn => (Lucide::TriangleAlert, cx.theme().warning),
                    Level::Fail => (Lucide::CircleX, cx.theme().danger),
                    Level::Info => (Lucide::Info, cx.theme().muted_foreground),
                };
                h_flex()
                    .id(("accounts-check", ix))
                    .role(Role::Label)
                    .aria_label(spoken)
                    .gap_3()
                    .px_4()
                    .py_2p5()
                    .items_start()
                    .child(
                        div()
                            .pt_0p5()
                            .child(Icon::new(icon).small().text_color(color)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().font_medium().child(c.title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .whitespace_normal()
                                    .child(c.detail),
                            )
                            .when_some(c.fix, |el, f| {
                                el.child(
                                    h_flex()
                                        .mt_1()
                                        .gap_1()
                                        .text_xs()
                                        .child(
                                            Icon::new(Lucide::Wrench)
                                                .xsmall()
                                                .text_color(cx.theme().muted_foreground),
                                        )
                                        .child(div().whitespace_normal().child(f)),
                                )
                            }),
                    )
                    .into_any_element()
            })
            .collect();
        let (scroll, content) = ui::tab_body("accounts-diagnostics");
        scroll
            .child(
                content
                    .gap_4()
                    .child(summary)
                    .child(ui::group_box(cx).children(ui::rows(rows, cx))),
            )
            .into_any_element()
    }
}
