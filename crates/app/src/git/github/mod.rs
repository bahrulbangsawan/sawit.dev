//! The GitHub tab: it leads with the sign-in card until you sign in (the
//! app's one sign-in, [`super::account`]), then shows the open repository's
//! pull requests and issues (conversation, comments, close, merge, new issue
//! or pull request) and your repositories to clone.

mod compose;
mod repos;
mod repository;
mod sign_in;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{Item, Repo};

use super::account::{self, GitHubAccount, SignIn};
use super::store::GitStore;
use super::thread::ThreadView;

const ITEM_HEIGHT: f32 = 48.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Repository,
    Repos,
}

#[derive(Clone)]
enum Load<T> {
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

pub struct GitHubView {
    store: Entity<GitStore>,
    account: Entity<GitHubAccount>,
    /// The sign-in what's loaded was loaded with: another one (a new account) loads afresh.
    loaded_with: Option<String>,
    page: Page,
    show_open: bool,
    items: Load<Vec<Item>>,
    /// What `items` was loaded for: owner, name, open.
    items_for: Option<(String, String, bool)>,
    thread: Option<Entity<ThreadView>>,
    /// Writing a new issue (`false`) or pull request (`true`).
    composing: Option<bool>,
    new_title: Entity<InputState>,
    new_body: Entity<TextareaState>,
    repos: Load<Vec<Repo>>,
    repo_search: Entity<InputState>,
    cloning: Option<String>,
    /// The sign-in code was just copied.
    code_copied: bool,
    working: bool,
    error: Option<String>,
    item_scroll: UniformListScrollHandle,
    repo_scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl GitHubView {
    pub fn new(store: Entity<GitStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let new_title = cx.new(|cx| InputState::new(window, cx).placeholder("Title"));
        let new_body =
            cx.new(|cx| TextareaState::new(window, cx).placeholder("Description (Markdown)"));
        let repo_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search your repositories"));
        let account = account::global(cx);
        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| {
                this.ensure_loaded(cx);
                cx.notify();
            }),
            // Signing in or out anywhere shows here at once.
            cx.observe(&account, |this, _, cx| {
                this.ensure_loaded(cx);
                cx.notify();
            }),
            cx.subscribe(&repo_search, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&new_title, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            store,
            account,
            loaded_with: None,
            page: Page::Repository,
            show_open: true,
            items: Load::Idle,
            items_for: None,
            thread: None,
            composing: None,
            new_title,
            new_body,
            repos: Load::Idle,
            repo_search,
            cloning: None,
            code_copied: false,
            working: false,
            error: None,
            item_scroll: UniformListScrollHandle::new(),
            repo_scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        if this.store.read(cx).slug.is_none() {
            this.page = Page::Repos;
        }
        this.ensure_loaded(cx);
        this
    }

    fn token(&self, cx: &App) -> Option<String> {
        self.account.read(cx).token().map(String::from)
    }

    fn slug(&self, cx: &App) -> Option<(String, String)> {
        self.store.read(cx).slug.clone()
    }

    /// When `error` is GitHub refusing `token` itself: the app signs out everywhere and this
    /// page goes back to its sign-in card, which says why. True then.
    fn refused(error: &sawit_core::CoreError, token: &str, cx: &mut App) -> bool {
        account::note_refusal(error, token, cx)
    }

    /// Loads what the current page shows if it isn't loaded yet. Signed out, or signed in as
    /// someone else, nothing loaded stays.
    fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        let token = self.token(cx);
        if token != self.loaded_with {
            self.loaded_with = token.clone();
            self.items = Load::Idle;
            self.items_for = None;
            self.repos = Load::Idle;
            self.thread = None;
            self.composing = None;
            self.error = None;
        }
        if token.is_none() {
            return;
        }
        match self.page {
            Page::Repository => {
                let want = self.slug(cx).map(|(o, n)| (o, n, self.show_open));
                if want.is_some() && want != self.items_for {
                    self.load_items(cx);
                }
            }
            Page::Repos => {
                if matches!(self.repos, Load::Idle) {
                    self.load_repos(cx);
                }
            }
        }
    }

    // ------------------------------------------------------------ render

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let store = self.store.read(cx);
        let login = self
            .account
            .read(cx)
            .login()
            .map(|l| format!("@{l}"))
            .unwrap_or_else(|| "GitHub".into());
        let slug = store.slug.clone();
        let account = self.account.clone();
        let page = self.page;
        let tab = |id: &'static str, label: String, which: Page| {
            Button::new(id)
                .ghost()
                .small()
                .label(label)
                .selected(page == which)
        };
        h_flex()
            .px_3()
            .py_1p5()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(Icon::new(Lucide::Github).small())
            .child(
                tab(
                    "gh-page-repo",
                    slug.map(|(o, n)| format!("{o}/{n}"))
                        .unwrap_or_else(|| "This Repository".into()),
                    Page::Repository,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Repository;
                    this.ensure_loaded(cx);
                    cx.notify();
                })),
            )
            .child(
                tab("gh-page-repos", "Your Repositories".into(), Page::Repos).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.page = Page::Repos;
                        this.ensure_loaded(cx);
                        cx.notify();
                    }),
                ),
            )
            .child(div().flex_1())
            // The account menu, as in the title bar: Sign Out is there.
            .child(
                Button::new("gh-account")
                    .ghost()
                    .small()
                    .label(login)
                    .tooltip("GitHub Account")
                    .dropdown_caret(true)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
                        account::account_menu(menu, account.read(cx))
                    }),
            )
    }
}

impl Render for GitHubView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The whole page needs GitHub: signed out it is the sign-in card, not a wall of
        // disabled buttons. A new sign-in under way (or failed) shows the card too.
        let account = self.account.read(cx);
        let card = !account.is_signed_in() || account.sign_in != SignIn::Idle;
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .when(card, |el| el.child(self.render_sign_in(cx)))
            .when(!card, |el| {
                el.child(self.render_header(cx))
                    .children(self.render_missing_scopes(cx))
                    .child(div().flex_1().min_h_0().child(match self.page {
                        Page::Repository => self.render_repository(cx),
                        Page::Repos => self.render_repos(cx),
                    }))
            })
    }
}
