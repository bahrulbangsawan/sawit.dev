//! One row of the account list: the avatar, the name with its `cpN` badge, who it is, the
//! 5-hour and 7-day meters, and the status pill; it drags to reorder and has the account's menu.

use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::Account;

use super::{DragLabel, DraggedAccount, RowFacts, account_menu, mini_meter, status_of};
use crate::accounts::page::AccountsPage;
use crate::accounts::ui;

impl AccountsPage {
    pub(super) fn render_row(
        &self,
        a: &Account,
        row: RowFacts,
        home: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let m = self.model.read(cx);
        let id = a.id.clone();
        let who = m
            .status(&a.id)
            .and_then(|s| s.email.clone())
            .unwrap_or_else(|| a.dir.display().to_string().replacen(home, "~", 1));
        let status = status_of(m, a, row.active, row.now);
        let usage = m.usage.get(&a.id).cloned();
        let cp = format!("cp{}", row.ix + 1);
        let spoken = match status {
            Some((_, _, s)) => format!("{}, {cp}, {s}", a.name),
            None => format!("{}, {cp}", a.name),
        };
        let page = cx.entity();
        let menu_account = a.clone();
        let tip = who.clone();
        let RowFacts {
            ix,
            count,
            selected,
            active,
            focused,
            narrow,
            now,
        } = row;
        h_flex()
            .id(SharedString::from(format!("account-row-{}", a.id)))
            .debug_selector({
                let id = a.id.clone();
                move || format!("account-row-{id}")
            })
            .role(Role::ListItem)
            .aria_label(spoken)
            .aria_selected(selected)
            .items_start()
            .gap(px(10.))
            .px(px(10.))
            .py_2()
            .rounded(cx.theme().radius)
            .border_2()
            .border_color(transparent_black())
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(selected && focused, |el| el.border_color(cx.theme().ring))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().secondary)))
            .child(ui::avatar(&a.name, &a.color, 28., cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .min_h(px(20.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .min_w_0()
                                    .truncate()
                                    .child(a.name.clone()),
                            )
                            .child(ui::badge(cp.clone(), cx))
                            .child(div().flex_1())
                            .when_some(status, |el, (tone, icon, label)| {
                                el.child(ui::pill(
                                    SharedString::from(format!("account-status-{}", a.id)),
                                    tone,
                                    Some(icon),
                                    label,
                                    cx,
                                ))
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("account-who-{}", a.id)))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(who)
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
                    )
                    .when(!narrow, |el| {
                        el.child(
                            h_flex()
                                .mt_1p5()
                                .gap_3()
                                .child(mini_meter(
                                    "5h",
                                    usage.as_ref().and_then(|u| u.five_hour),
                                    now,
                                    cx,
                                ))
                                .child(mini_meter(
                                    "7d",
                                    usage.as_ref().and_then(|u| u.seven_day),
                                    now,
                                    cx,
                                )),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.list_focus.focus(window, cx);
                this.selected = Some(id.clone());
                this.renaming = false;
                cx.notify();
            }))
            .on_drag(
                DraggedAccount {
                    id: a.id.clone(),
                    name: a.name.clone().into(),
                },
                |d: &DraggedAccount, _, _, cx| cx.new(|_| DragLabel(d.name.clone())),
            )
            .drag_over::<DraggedAccount>(|el, _, _, cx| el.bg(cx.theme().accent))
            .on_drop(cx.listener(move |this, d: &DraggedAccount, _, cx| {
                this.model
                    .update(cx, |m, cx| m.move_account_to(&d.id, ix, cx));
            }))
            .context_menu(move |menu, _, _| {
                account_menu(menu, &page, &menu_account, ix, count, active)
            })
            .into_any_element()
    }
}
