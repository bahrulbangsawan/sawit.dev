//! The GitHub tab: sign in with your GitHub account, then the open
//! repository's pull requests and issues (conversation, comments, close,
//! merge, new issue or pull request) and your repositories to clone.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git;
use sawit_core::github::{self, Item, Repo, ago, now};

use super::store::{GitStore, SignIn};
use super::thread::{ThreadEvent, ThreadView};
use crate::rt;
use crate::states::{Banner, Placeholder};

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
        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| {
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
        let store = self.store.read(cx);
        store.account.as_ref().and(store.token.clone())
    }

    fn slug(&self, cx: &App) -> Option<(String, String)> {
        self.store.read(cx).slug.clone()
    }

    /// Loads what the current page shows if it isn't loaded yet.
    fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        if self.token(cx).is_none() {
            self.items = Load::Idle;
            self.items_for = None;
            self.repos = Load::Idle;
            self.thread = None;
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

    fn load_items(&mut self, cx: &mut Context<Self>) {
        let (Some(token), Some((owner, name))) = (self.token(cx), self.slug(cx)) else {
            return;
        };
        let open = self.show_open;
        self.items_for = Some((owner.clone(), name.clone(), open));
        self.items = Load::Loading;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            github::items(&token, &owner, &name, open).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.items = match result {
                    Ok(items) => Load::Ready(items),
                    Err(e) => Load::Failed(e.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn load_repos(&mut self, cx: &mut Context<Self>) {
        let Some(token) = self.token(cx) else {
            return;
        };
        self.repos = Load::Loading;
        cx.notify();
        let rx = rt::spawn(cx, async move { github::repos(&token).await });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.repos = match result {
                    Ok(repos) => Load::Ready(repos),
                    Err(e) => Load::Failed(e.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn items(&self) -> &[Item] {
        match &self.items {
            Load::Ready(items) => items,
            _ => &[],
        }
    }

    fn open_thread(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(token), Some(slug)) = (self.token(cx), self.slug(cx)) else {
            return;
        };
        let Some(item) = self.items().iter().find(|i| i.number == number).cloned() else {
            return;
        };
        self.composing = None;
        self.error = None;
        let thread = cx.new(|cx| ThreadView::new(item, slug, token, window, cx));
        cx.subscribe(&thread, |this, _, e: &ThreadEvent, cx| {
            this.on_thread_event(e, cx)
        })
        .detach();
        self.thread = Some(thread);
        cx.notify();
    }

    /// Keeps the list in step with what was done in the open thread.
    fn on_thread_event(&mut self, event: &ThreadEvent, cx: &mut Context<Self>) {
        let number = match event {
            ThreadEvent::StateChanged { number, .. }
            | ThreadEvent::Commented(number)
            | ThreadEvent::Merged(number) => *number,
        };
        if let Load::Ready(items) = &mut self.items
            && let Some(item) = items.iter_mut().find(|i| i.number == number)
        {
            match event {
                ThreadEvent::StateChanged { open, .. } => item.open = *open,
                ThreadEvent::Commented(_) => item.comments += 1,
                ThreadEvent::Merged(_) => item.open = false,
            }
        }
        if let ThreadEvent::Merged(number) = *event {
            // The merge commit is on the remote now.
            self.store.update(cx, |s, cx| {
                s.run("Fetching…", cx, move |repo, token| async move {
                    git::fetch(&repo, token.as_deref()).await?;
                    Ok(format!(
                        "Pull request #{number} was merged. Pull to bring it into your branch."
                    ))
                })
            });
        }
        cx.notify();
    }

    fn compose(&mut self, pull: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.composing = Some(pull);
        self.thread = None;
        self.error = None;
        let title = if pull {
            self.store
                .read(cx)
                .status
                .as_ref()
                .and_then(|s| s.branch.clone())
                .map(|b| b.replace(['-', '_', '/'], " "))
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.new_title.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.focus(window, cx);
        });
        self.new_body
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(token), Some((owner, name)), Some(pull)) =
            (self.token(cx), self.slug(cx), self.composing)
        else {
            return;
        };
        let title = self.new_title.read(cx).value().trim().to_string();
        let body = self.new_body.read(cx).value().to_string();
        let head = self
            .store
            .read(cx)
            .status
            .as_ref()
            .and_then(|s| s.branch.clone());
        if title.is_empty() {
            return;
        }
        if pull && head.is_none() {
            self.error = Some(
                "Switch to a branch first: a pull request is opened from your current branch."
                    .into(),
            );
            cx.notify();
            return;
        }
        self.working = true;
        self.error = None;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            if pull {
                let head = head.unwrap_or_default();
                let base = github::default_branch(&token, &owner, &name).await?;
                if head == base {
                    return Err(sawit_core::CoreError::GitHub(format!(
                        "You're on {base}, the default branch. Create a branch with your changes, push it, then open the pull request."
                    )));
                }
                github::create_pull(&token, &owner, &name, &head, &base, &title, &body).await
            } else {
                github::create_issue(&token, &owner, &name, &title, &body).await
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = false;
                match result {
                    Ok(item) => {
                        let number = item.number;
                        if let Load::Ready(items) = &mut this.items
                            && this.show_open
                        {
                            items.insert(0, item);
                        }
                        this.composing = None;
                        this.open_thread(number, window, cx);
                    }
                    Err(e) => {
                        let mut message = e.to_string();
                        if pull && message.contains("422") {
                            message.push_str(" Push your branch first, and check that a pull request for it isn't already open.");
                        }
                        this.error = Some(message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn clone_repo(&mut self, repo: Repo, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Clone Here".into()),
        });
        let handle = window.window_handle();
        let token = self.token(cx);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(parent) = paths.into_iter().next() else {
                return;
            };
            let name = repo
                .full_name
                .rsplit('/')
                .next()
                .unwrap_or(&repo.full_name)
                .to_string();
            let target = parent.join(&name);
            let Ok(rx) = this.update(cx, |this, cx| {
                this.cloning = Some(repo.full_name.clone());
                this.error = None;
                cx.notify();
                let (url, target) = (repo.clone_url.clone(), target.clone());
                rt::spawn(cx, async move {
                    if target.exists() {
                        return Err(sawit_core::CoreError::Git(format!(
                            "{} already exists. Pick another folder.",
                            target.display()
                        )));
                    }
                    git::clone(&url, &target, token.as_deref()).await
                })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.cloning = None;
                match result {
                    Ok(Ok(())) => {
                        cx.defer(move |cx| crate::cli::open_folder_in(target, Some(handle), cx))
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ------------------------------------------------------------ render

    fn render_sign_in(&self, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        let muted = cx.theme().muted_foreground;
        let card = match &store.sign_in {
            SignIn::Waiting(device) => {
                let (code, url) = (device.user_code.clone(), device.verification_uri.clone());
                let page = url.trim_start_matches("https://").to_string();
                let copied = self.code_copied;
                v_flex()
                    .gap_5()
                    .child(card_heading(
                        "Enter this code on GitHub",
                        "Sawit is waiting for you to approve it.",
                        cx,
                    ))
                    .child(
                        div()
                            .w_full()
                            .py_4()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .text_center()
                            .text_3xl()
                            .font_semibold()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(code.clone()),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(step(1, "Copy the code above.".into(), cx))
                            .child(step(2, format!("Paste it at {page}."), cx))
                            .child(step(3, "Click Authorize, then come back here.".into(), cx)),
                    )
                    .child(
                        Button::new("gh-open-device")
                            .primary()
                            .w_full()
                            .icon(if copied { Lucide::Check } else { Lucide::Copy })
                            .label(if copied {
                                "Copied, Opening GitHub…"
                            } else {
                                "Copy Code & Open GitHub"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                                cx.open_url(&url);
                                this.flash_copied(cx);
                            })),
                    )
                    .child(
                        h_flex()
                            .pt_3()
                            .gap_2()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .text_sm()
                            .text_color(muted)
                            .child(Spinner::new().small())
                            .child(div().flex_1().child("Waiting for approval"))
                            .child(
                                Button::new("gh-cancel-sign-in")
                                    .ghost()
                                    .small()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.store.update(cx, |s, cx| s.cancel_sign_in(cx))
                                    })),
                            ),
                    )
            }
            state => {
                let starting = matches!(state, SignIn::Starting);
                let failed = match state {
                    SignIn::Failed(e) => Some(e.clone()),
                    _ => None,
                };
                let perk = |text: &'static str, cx: &App| {
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(
                            Icon::new(Lucide::Check)
                                .small()
                                .text_color(cx.theme().success),
                        )
                        .child(text)
                };
                v_flex()
                    .gap_5()
                    .child(card_heading(
                        "Connect your GitHub account",
                        "Sign in once to work with GitHub from Sawit.",
                        cx,
                    ))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(perk("Pull requests and issues, with their timelines", cx))
                            .child(perk("Comment, close and merge", cx))
                            .child(perk("Clone your repositories", cx))
                            .child(perk("Push and pull without password prompts", cx)),
                    )
                    .when_some(failed, |el, e| el.child(Banner::error("gh-sign-in-error", e)))
                    .child(
                        Button::new("gh-sign-in")
                            .primary()
                            .w_full()
                            .icon(Lucide::Github)
                            .label(if starting { "Contacting GitHub…" } else { "Sign In with GitHub" })
                            .loading(starting)
                            .disabled(starting)
                            .on_click(cx.listener(|this, _, _, cx| this.store.update(cx, |s, cx| s.sign_in(cx)))),
                    )
                    .child(
                        h_flex()
                            .items_start()
                            .gap_1p5()
                            .text_xs()
                            .text_color(muted)
                            .child(div().pt(px(2.)).child(Icon::new(Lucide::Lock).xsmall()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child("Sawit never sees your password. Your sign-in is kept in the macOS Keychain."),
                            ),
                    )
            }
        };
        div()
            .id("gh-sign-in-page")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .min_h_full()
                    .p_6()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(400.))
                            .p_6()
                            .rounded(cx.theme().radius * 2.)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .child(card),
                    ),
            )
            .into_any_element()
    }

    /// The copy button says "Copied" for a moment.
    fn flash_copied(&mut self, cx: &mut Context<Self>) {
        self.code_copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(2500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.code_copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let store = self.store.read(cx);
        let login = store
            .account
            .as_ref()
            .map(|a| a.login.clone())
            .unwrap_or_default();
        let slug = store.slug.clone();
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
                    slug.map(|(o, n)| format!("{o}/{n}")).unwrap_or_else(|| "This Repository".into()),
                    Page::Repository,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Repository;
                    this.ensure_loaded(cx);
                    cx.notify();
                })),
            )
            .child(
                tab("gh-page-repos", "Your Repositories".into(), Page::Repos).on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Repos;
                    this.ensure_loaded(cx);
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("@{login}")))
            .child(
                Button::new("gh-sign-out")
                    .ghost()
                    .small()
                    .icon(Lucide::LogOut)
                    .tooltip("Sign Out of GitHub")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let answer = window.prompt(
                            PromptLevel::Warning,
                            "Sign out of GitHub?",
                            Some("Sawit forgets your sign-in. Your repositories and local changes stay as they are."),
                            &["Sign Out", "Cancel"],
                            cx,
                        );
                        let store = this.store.clone();
                        cx.spawn(async move |_, cx| {
                            if answer.await == Ok(0) {
                                store.update(cx, |s, cx| s.sign_out(cx));
                            }
                        })
                        .detach();
                    })),
            )
    }

    fn render_item_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.items().get(ix).cloned() else {
            return div().into_any_element();
        };
        let selected = self
            .thread
            .as_ref()
            .is_some_and(|t| t.read(cx).number() == item.number);
        let (icon, color) = match (item.pull, item.open) {
            (true, true) => (Lucide::GitPullRequest, cx.theme().success),
            (true, false) => (Lucide::GitMerge, cx.theme().muted_foreground),
            (false, true) => (Lucide::CircleDot, cx.theme().success),
            (false, false) => (Lucide::CircleCheck, cx.theme().muted_foreground),
        };
        let number = item.number;
        h_flex()
            .id(("gh-item", ix))
            .w_full()
            .h(px(ITEM_HEIGHT))
            .px_3()
            .gap_2()
            .cursor_pointer()
            .border_b_1()
            .border_color(cx.theme().border)
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .on_click(cx.listener(move |this, _, window, cx| this.open_thread(number, window, cx)))
            .child(Icon::new(icon).small().text_color(color))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(item.title.clone()))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "#{} · {} · {}",
                                item.number,
                                item.author,
                                ago(item.updated_at, now())
                            )),
                    ),
            )
            .when(item.comments > 0, |el| {
                el.child(
                    h_flex()
                        .gap_0p5()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(Lucide::MessageSquare).xsmall())
                        .child(item.comments.to_string()),
                )
            })
            .into_any_element()
    }

    fn render_thread(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(pull) = self.composing {
            return self.render_compose(pull, cx);
        }
        let Some(thread) = &self.thread else {
            return Placeholder::new(Lucide::MessageSquare, "Select a pull request or issue")
                .detail("Its conversation shows here, and you can reply.")
                .into_any_element();
        };
        thread.clone().into_any_element()
    }

    fn render_compose(&self, pull: bool, cx: &mut Context<Self>) -> AnyElement {
        let branch = self
            .store
            .read(cx)
            .status
            .as_ref()
            .and_then(|s| s.branch.clone());
        let ready = !self.new_title.read(cx).value().trim().is_empty() && !self.working;
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(div().text_lg().font_semibold().child(if pull { "New Pull Request" } else { "New Issue" }))
            .when(pull, |el| {
                el.child(div().text_sm().text_color(cx.theme().muted_foreground).child(match branch {
                    Some(b) => format!("From {b} into the default branch. Push {b} first so GitHub has your commits."),
                    None => "Switch to a branch first.".into(),
                }))
            })
            .child(Input::new(&self.new_title))
            .child(Textarea::new(&self.new_body).h(px(200.)).aria_label("Description"))
            .when_some(self.error.clone(), |el, e| el.child(Banner::error("gh-compose-error", e)))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("gh-compose-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composing = None;
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("gh-compose-create")
                            .primary()
                            .small()
                            .label(if pull { "Create Pull Request" } else { "Create Issue" })
                            .loading(self.working)
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_repository(&self, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        if store.slug.is_none() {
            let detail = if store.repo.is_none() {
                "Open a folder that's a GitHub repository, or clone one from Your Repositories."
            } else {
                "This repository's origin remote isn't on github.com."
            };
            return Placeholder::new(Lucide::Github, "No GitHub repository")
                .detail(detail)
                .action(
                    Button::new("gh-go-repos")
                        .small()
                        .label("Your Repositories")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.page = Page::Repos;
                            this.ensure_loaded(cx);
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        let show_open = self.show_open;
        let list: AnyElement = match &self.items {
            Load::Failed(e) => Placeholder::error("Unable to load from GitHub")
                .compact()
                .detail(e.clone())
                .action(
                    Button::new("gh-retry")
                        .small()
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.load_items(cx))),
                )
                .into_any_element(),
            Load::Ready(items) if items.is_empty() => Placeholder::new(
                Lucide::Check,
                if show_open {
                    "Nothing open"
                } else {
                    "Nothing closed"
                },
            )
            .compact()
            .detail(if show_open {
                "No open pull requests or issues."
            } else {
                "No closed pull requests or issues yet."
            })
            .into_any_element(),
            Load::Ready(items) => uniform_list(
                "gh-items",
                items.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| this.render_item_row(ix, cx)).collect()
                }),
            )
            .track_scroll(&self.item_scroll)
            .size_full()
            .into_any_element(),
            _ => Placeholder::loading("Loading from GitHub…")
                .compact()
                .into_any_element(),
        };
        let has_branch = store.status.as_ref().is_some_and(|s| s.branch.is_some());
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .w(px(340.))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .gap_1()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("gh-show-open")
                                    .ghost()
                                    .xsmall()
                                    .label("Open")
                                    .selected(show_open)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_open = true;
                                        this.ensure_loaded(cx);
                                    })),
                            )
                            .child(
                                Button::new("gh-show-closed")
                                    .ghost()
                                    .xsmall()
                                    .label("Closed")
                                    .selected(!show_open)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_open = false;
                                        this.ensure_loaded(cx);
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("gh-new-issue")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::CircleDot)
                                    .tooltip("New Issue")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.compose(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("gh-new-pull")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::GitPullRequest)
                                    .tooltip("New Pull Request from This Branch")
                                    .disabled(!has_branch)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.compose(true, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("gh-refresh")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::RefreshCw)
                                    .tooltip("Refresh")
                                    .on_click(cx.listener(|this, _, _, cx| this.load_items(cx))),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(list)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_thread(cx)),
            )
            .into_any_element()
    }

    fn filtered_repos(&self, cx: &App) -> Vec<Repo> {
        let query = self.repo_search.read(cx).value().to_lowercase();
        match &self.repos {
            Load::Ready(repos) => repos
                .iter()
                .filter(|r| {
                    query.is_empty()
                        || r.full_name.to_lowercase().contains(&query)
                        || r.description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(&query))
                })
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn render_repo_row(&self, ix: usize, repo: Repo, cx: &mut Context<Self>) -> AnyElement {
        let cloning = self.cloning.as_deref() == Some(repo.full_name.as_str());
        let busy = self.cloning.is_some();
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id(("gh-repo", ix))
            .w_full()
            .h(px(ITEM_HEIGHT))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Icon::new(if repo.private {
                    Lucide::Lock
                } else {
                    Lucide::BookMarked
                })
                .small()
                .text_color(muted),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(repo.full_name.clone()))
                    .child(div().text_xs().truncate().text_color(muted).child(
                        match &repo.description {
                            Some(d) => format!("{d} · pushed {}", ago(repo.pushed_at, now())),
                            None => format!("Pushed {}", ago(repo.pushed_at, now())),
                        },
                    )),
            )
            .child(
                Button::new(("gh-clone", ix))
                    .small()
                    .icon(Lucide::Download)
                    .label(if cloning { "Cloning…" } else { "Clone…" })
                    .loading(cloning)
                    .disabled(busy)
                    .tooltip("Clone into a Folder and Open It")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.clone_repo(repo.clone(), window, cx)
                    })),
            )
            .into_any_element()
    }

    fn render_repos(&self, cx: &mut Context<Self>) -> AnyElement {
        let repos = self.filtered_repos(cx);
        let count = repos.len();
        let list: AnyElement = match &self.repos {
            Load::Failed(e) => Placeholder::error("Unable to load your repositories")
                .detail(e.clone())
                .action(
                    Button::new("gh-repos-retry")
                        .small()
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.load_repos(cx))),
                )
                .into_any_element(),
            Load::Ready(all) if all.is_empty() => {
                Placeholder::new(Lucide::BookMarked, "No repositories")
                    .detail("Repositories you own or collaborate on show here.")
                    .into_any_element()
            }
            Load::Ready(_) if count == 0 => {
                Placeholder::new(IconName::Search, "No matching repositories")
                    .detail("Nothing matches your search.")
                    .into_any_element()
            }
            Load::Ready(_) => uniform_list(
                "gh-repos",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let repos = this.filtered_repos(cx);
                    range
                        .filter_map(|ix| Some(this.render_repo_row(ix, repos.get(ix)?.clone(), cx)))
                        .collect()
                }),
            )
            .track_scroll(&self.repo_scroll)
            .size_full()
            .into_any_element(),
            _ => Placeholder::loading("Loading your repositories…").into_any_element(),
        };
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.repo_search).small().cleanable(true)),
                    )
                    .child(
                        Button::new("gh-repos-refresh")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Refresh")
                            .on_click(cx.listener(|this, _, _, cx| this.load_repos(cx))),
                    ),
            )
            .when_some(
                self.error.clone().filter(|_| self.page == Page::Repos),
                |el, e| el.child(Banner::error("gh-repos-error", e)),
            )
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }
}

impl Render for GitHubView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let signed_in = self.token(cx).is_some();
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .when(!signed_in, |el| el.child(self.render_sign_in(cx)))
            .when(signed_in, |el| {
                el.child(self.render_header(cx))
                    .child(div().flex_1().min_h_0().child(match self.page {
                        Page::Repository => self.render_repository(cx),
                        Page::Repos => self.render_repos(cx),
                    }))
            })
    }
}

/// The GitHub mark, a title and one line under it, for the sign-in card.
fn card_heading(title: &'static str, subtitle: &'static str, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_3()
        .child(
            div()
                .flex_none()
                .size(px(40.))
                .rounded_full()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(Lucide::Github).size(px(20.))),
        )
        .child(
            v_flex()
                .gap_0p5()
                .child(div().text_base().font_semibold().child(title))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
}

/// A numbered step of the sign-in.
fn step(n: u8, text: String, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_2p5()
        .text_sm()
        .child(
            div()
                .flex_none()
                .size(px(20.))
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .flex()
                .items_center()
                .justify_center()
                .child(n.to_string()),
        )
        .child(text)
}
