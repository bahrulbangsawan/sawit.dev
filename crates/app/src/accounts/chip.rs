//! The title bar's account chip: the active Claude Code account and its usage
//! (`work · 5h 23% · 7d 41%`). Its menu switches the active account (each with its usage) and
//! opens the Accounts page. Shown only once there are two accounts or more.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;

use super::rows;
use super::{AccountsModel, OpenAccounts, global};

/// The chip, or nothing while there is one account or none.
pub fn account_chip(cx: &mut App) -> AnyElement {
    let model = global(cx);
    let m = model.read(cx);
    if m.list().len() < 2 {
        return div().into_any_element();
    }
    let now = sawit_core::github::now() as f64;
    let active = m.active().cloned();
    let label = match &active {
        Some(a) => match rows::short(m.usage.get(&a.id), now) {
            Some(u) => format!("{} · {u}", a.name),
            None => a.name.clone(),
        },
        None => "No Active Account".into(),
    };
    let color = active
        .as_ref()
        .map(|a| a.color.clone())
        .unwrap_or_else(|| "#888888".into());
    Button::new("account-chip")
        .ghost()
        .xsmall()
        .child(rows::swatch(&color, 8.))
        .child(div().child(label.clone()))
        .accessibility_label(format!("Claude Code Account: {label}"))
        .tooltip("The Claude Code Account New Sessions Use")
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
            menu_of(menu, &model, cx)
        })
        .into_any_element()
}

fn menu_of(
    menu: gpui_kit::component::menu::PopupMenu,
    model: &Entity<AccountsModel>,
    cx: &App,
) -> gpui_kit::component::menu::PopupMenu {
    let m = model.read(cx);
    let now = sawit_core::github::now() as f64;
    let active = m.active().map(|a| a.id.clone());
    let mut menu = menu.min_w(px(240.)).max_w(px(360.));
    for a in m.list() {
        let usage = rows::short(m.usage.get(&a.id), now).unwrap_or_default();
        let text = if usage.is_empty() {
            a.name.clone()
        } else {
            format!("{}   {usage}", a.name)
        };
        let id = a.id.clone();
        menu = menu.item(
            PopupMenuItem::new(text)
                .checked(active.as_ref() == Some(&a.id))
                .on_click(move |_, _, cx| global(cx).update(cx, |m, cx| m.set_active(&id, cx))),
        );
    }
    menu.separator().item(
        PopupMenuItem::new("Accounts Page")
            .icon(Lucide::Users)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenAccounts), cx)),
    )
}
