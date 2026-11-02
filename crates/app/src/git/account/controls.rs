//! The GitHub controls every view shares: a control gated on the sign-in, the sign-in link beside
//! it, the account menu (the one place to sign out) and the title bar's account control, whose
//! menu also lists the window's pages.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::sync::Arc;

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

/// Adds the window's pages to a menu: the title bar's account menu lists them (the workspace
/// knows them), the GitHub page's own account menu doesn't.
pub type Pages = fn(PopupMenu) -> PopupMenu;

/// The account menu, the one place to sign out: who is signed in, a new sign-in when this one
/// lacks a scope Sawit asks for, the GitHub page and Sign Out.
pub fn account_menu(menu: PopupMenu, account: &GitHubAccount) -> PopupMenu {
    menu_with(menu, account, None)
}

/// [`account_menu`] with `pages` between who is signed in and the GitHub items.
fn menu_with(menu: PopupMenu, account: &GitHubAccount, pages: Option<Pages>) -> PopupMenu {
    let header = match (account.account(), account.session()) {
        (Some(a), _) => {
            let picture = Picture::Account {
                login: a.login.clone(),
                image: account.avatar(),
            };
            match &a.name {
                Some(name) => header(
                    picture,
                    name.clone(),
                    Some((format!("@{}", a.login), false)),
                    format!("Signed in as @{} ({name})", a.login),
                ),
                None => header(
                    picture,
                    format!("@{}", a.login),
                    None,
                    format!("Signed in as @{}", a.login),
                ),
            }
        }
        (None, Session::Unconfirmed { error: Some(e), .. }) => header(
            Picture::Icon(Lucide::Github),
            "GitHub".into(),
            Some((e.clone(), true)),
            e.clone(),
        ),
        (None, _) => header(
            Picture::Icon(Lucide::Github),
            "Checking your GitHub sign-in…".into(),
            None,
            "Checking your GitHub sign-in…".into(),
        ),
    };
    let mut menu = menu.min_w(px(240.)).max_w(px(320.)).item(header);
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
    if let Some(pages) = pages {
        menu = pages(menu.separator());
    }
    menu.separator()
        .item(
            PopupMenuItem::new("GitHub Page")
                .icon(Lucide::Github)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenGitHub), cx)),
        )
        .item(
            PopupMenuItem::new("Sign Out of GitHub…")
                .icon(Lucide::LogOut)
                .on_click(|_, window, cx| confirm_sign_out(window, cx)),
        )
}

/// The title bar's menu while nobody is signed in: why GitHub features are off, the one sign-in
/// (an item like the others, so the arrow keys reach it and ↩ signs in), then the pages.
fn signed_out_menu(menu: PopupMenu, account: &GitHubAccount, pages: Pages) -> PopupMenu {
    let (detail, warn) = match account.notice() {
        Some(notice) => (notice.to_string(), true),
        None => (
            "One sign-in for pull requests, issues and cloning".to_string(),
            false,
        ),
    };
    let sign_in = if account.flow_under_way() {
        "Finish Signing In…"
    } else {
        "Sign In with GitHub…"
    };
    let menu = menu
        .min_w(px(240.))
        .max_w(px(320.))
        .item(header(
            Picture::Icon(Lucide::CircleUser),
            "Not signed in to GitHub".into(),
            Some((detail.clone(), warn)),
            format!("Not signed in to GitHub. {detail}"),
        ))
        .item(
            PopupMenuItem::new(sign_in)
                .icon(Lucide::Github)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx)),
        );
    pages(menu.separator())
}

/// What the menu's first row shows for the account: its picture, else an icon.
#[derive(Clone)]
enum Picture {
    Account {
        login: String,
        image: Option<Arc<Image>>,
    },
    Icon(Lucide),
}

/// The menu's first row: a picture, a first line and a muted second one (in the warning colour
/// when it is a problem). A heading rather than an item, so no pointer or key picks it; GPUI
/// exposes no plain text to assistive technology, so `spoken` is what it reads there.
fn header(
    picture: Picture,
    title: String,
    detail: Option<(String, bool)>,
    spoken: String,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let avatar = match &picture {
            Picture::Account {
                login,
                image: Some(image),
            } => Avatar::new().name(login.clone()).src(image.clone()),
            Picture::Account { login, image: None } => Avatar::new().name(login.clone()),
            Picture::Icon(icon) => Avatar::new().placeholder(*icon),
        };
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id("github-account-header")
            .role(Role::Label)
            .aria_label(spoken.clone())
            // The menu keeps an icon's width (and its gap) before every row: the picture takes
            // that place, so it lines up with the items' icons.
            .ml(px(-16.))
            .py_1p5()
            .gap_2p5()
            .child(avatar.with_size(px(32.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(cx.theme().popover_foreground)
                            .child(title.clone()),
                    )
                    .when_some(detail.clone(), |el, (text, warn)| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(if warn { cx.theme().warning } else { muted })
                                .child(text),
                        )
                    }),
            )
    })
    .disabled(true)
}

/// The title bar's account control. Signed in: the avatar and login, opening the account menu
/// with the window's `pages`. Otherwise "Sign In" (marked when GitHub refused the last sign-in),
/// opening the same pages under the one sign-in.
pub fn account_control(account: &Entity<GitHubAccount>, pages: Pages, cx: &App) -> AnyElement {
    let a = account.read(cx);
    if a.session == Session::Loading {
        return div().into_any_element();
    }
    let menu_account = account.clone();
    if !a.is_signed_in() || a.flow_under_way() {
        let notice = a.notice.clone();
        let label = if a.flow_under_way() {
            "Finish Signing In"
        } else {
            "Sign In"
        };
        return Button::new("github-account")
            .ghost()
            .xsmall()
            .icon(match notice {
                Some(_) => Icon::new(Lucide::TriangleAlert).text_color(cx.theme().warning),
                None => Icon::new(Lucide::CircleUser),
            })
            .label(label)
            // An ellipsis, as on any Mac: a menu of choices opens, nothing signs in yet.
            .accessibility_label(format!("{label} to GitHub…"))
            .tooltip(notice.unwrap_or_else(|| {
                "One sign-in for every GitHub feature: pull requests, issues, cloning and more"
                    .into()
            }))
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
                signed_out_menu(menu, menu_account.read(cx), pages)
            })
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
            "GitHub Account and Pages"
        })
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
            menu_with(menu, menu_account.read(cx), Some(pages))
        })
        .into_any_element()
}
