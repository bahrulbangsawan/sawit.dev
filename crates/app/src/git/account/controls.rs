//! The GitHub controls every view shares: a control gated on the sign-in, the sign-in link beside
//! it, the account menu (the one place to sign out) and the title bar's account control.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{GitHubAccount, OpenGitHub, Session, SignInToGitHub, global};

// ---------------------------------------------------------------- controls

/// A GitHub control as the sign-in allows it: as built while signed in; while signed out
/// disabled, its tooltip saying why. Its label stays, so does its accessible name, and its text
/// is muted rather than faded, so it stays legible. Put [`sign_in_link`] beside it.
pub fn gate(button: Button, account: &GitHubAccount, to: &str, cx: &App) -> Button {
    match account.blocked(to) {
        None => button,
        Some(why) => button
            .disabled(true)
            .tooltip(why)
            .text_color(cx.theme().muted_foreground),
    }
}

/// The way on beside a GitHub control while signed out: the one sign-in.
pub fn sign_in_link(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(Lucide::Github)
        .label("Sign In to GitHub")
        .tooltip("One Sign-In for Every GitHub Feature in Sawit")
        .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx))
}

/// Asks, then signs out of GitHub everywhere.
pub fn confirm_sign_out(window: &mut Window, cx: &mut App) {
    let answer = window.prompt(
        PromptLevel::Warning,
        "Sign out of GitHub?",
        Some(
            "Sawit forgets your sign-in, and its GitHub features turn off until you sign in again: pull requests and issues, cloning your repositories, saving shell files. Pushing and pulling keep working with your own Git credentials. Your repositories and local changes stay as they are.",
        ),
        &["Sign Out", "Cancel"],
        cx,
    );
    let account = global(cx);
    cx.spawn(async move |cx| {
        if answer.await == Ok(0) {
            account.update(cx, |a, cx| a.sign_out(cx));
        }
    })
    .detach();
}

/// The account menu, the one place to sign out: who is signed in, the page, a new sign-in when
/// this one lacks a scope Sawit asks for, and Sign Out.
pub fn account_menu(menu: PopupMenu, account: &GitHubAccount) -> PopupMenu {
    let mut menu = match (account.account(), account.session()) {
        (Some(a), _) => menu.label(match &a.name {
            Some(name) => format!("Signed in as @{} ({name})", a.login),
            None => format!("Signed in as @{}", a.login),
        }),
        (None, Session::Unconfirmed { error: Some(e), .. }) => menu.label(e.clone()),
        (None, _) => menu.label("Checking your GitHub sign-in…"),
    };
    let missing = account.missing_scopes();
    if !missing.is_empty() {
        menu = menu.item(
            PopupMenuItem::new(format!("Sign In Again to Allow {}…", missing.join(", ")))
                .icon(Lucide::KeyRound)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx)),
        );
    }
    if matches!(
        account.session(),
        Session::Unconfirmed { error: Some(_), .. }
    ) {
        menu = menu.item(
            PopupMenuItem::new("Check Again")
                .icon(Lucide::RefreshCw)
                .on_click(|_, _, cx| global(cx).update(cx, |a, cx| a.check_again(cx))),
        );
    }
    menu.item(
        PopupMenuItem::new("Open GitHub Page")
            .icon(Lucide::Github)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenGitHub), cx)),
    )
    .separator()
    .item(
        PopupMenuItem::new("Sign Out of GitHub…")
            .icon(Lucide::LogOut)
            .on_click(|_, window, cx| confirm_sign_out(window, cx)),
    )
}

/// The title bar's account control: "Sign In to GitHub" while signed out (marked when GitHub
/// refused the last sign-in), the avatar and login once signed in, with the account menu.
pub fn account_control(account: &Entity<GitHubAccount>, cx: &App) -> AnyElement {
    let a = account.read(cx);
    if a.session == Session::Loading {
        return div().into_any_element();
    }
    if !a.is_signed_in() || a.flow_under_way() {
        let notice = a.notice.clone();
        let label = if a.flow_under_way() {
            "Finish Signing In"
        } else {
            "Sign In to GitHub"
        };
        return Button::new("github-account")
            .ghost()
            .xsmall()
            .icon(match notice {
                Some(_) => Icon::new(Lucide::TriangleAlert).text_color(cx.theme().warning),
                None => Icon::new(Lucide::Github),
            })
            .label(label)
            .tooltip(notice.unwrap_or_else(|| {
                "One sign-in for every GitHub feature: pull requests, issues, cloning and more"
                    .into()
            }))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx))
            .into_any_element();
    }
    let login = a.login().map(String::from);
    let unconfirmed = matches!(a.session, Session::Unconfirmed { error: Some(_), .. });
    let lacking = !a.missing_scopes().is_empty();
    let name = login
        .as_ref()
        .map(|l| format!("@{l}"))
        .unwrap_or_else(|| "GitHub".into());
    let avatar = match (a.avatar(), &login) {
        (Some(image), Some(login)) => Avatar::new().name(login.clone()).src(image),
        (None, Some(login)) => Avatar::new().name(login.clone()),
        _ => Avatar::new().placeholder(Lucide::Github),
    };
    let menu_account = account.clone();
    Button::new("github-account")
        .ghost()
        .xsmall()
        .child(avatar.xsmall())
        .child(div().child(name.clone()))
        .when(unconfirmed || lacking, |b| {
            b.child(
                Icon::new(Lucide::TriangleAlert)
                    .xsmall()
                    .text_color(cx.theme().warning),
            )
        })
        .accessibility_label(format!("GitHub Account {name}"))
        .tooltip(if unconfirmed {
            "Sawit couldn't reach GitHub to check your sign-in"
        } else if lacking {
            "Your GitHub sign-in lacks a permission Sawit asks for"
        } else {
            "GitHub Account"
        })
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
            account_menu(menu, menu_account.read(cx))
        })
        .into_any_element()
}
