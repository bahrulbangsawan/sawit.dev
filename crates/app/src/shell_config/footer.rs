//! The Config Files panel's GitHub footer: one control for each sign-in state, and the sync
//! once the repository is found.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::FileState;
use sawit_core::shell_config::{BACKUP_REPO, FileSync, Scope};
use sawit_core::text_diff::DiffLine;

use super::ShellConfigPanel;
use super::sync::Remote;
use crate::git::account::{Session, SignInToGitHub, UNREADABLE};

/// What the footer shows, by the sign-in and the repository: one way on at a time.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FooterState {
    Checking,
    SignedOut,
    FinishSigningIn,
    /// The Keychain couldn't be read, or GitHub couldn't be reached: why, with Try Again.
    Problem(String),
    LookingForRepo,
    NoRepo,
    Sync,
    Failed(String),
}

impl ShellConfigPanel {
    pub(crate) fn footer_state(&self, cx: &App) -> FooterState {
        let a = self.account.read(cx);
        if a.flow_under_way() {
            return FooterState::FinishSigningIn;
        }
        match a.session() {
            Session::Loading | Session::Unconfirmed { error: None, .. } => {
                return FooterState::Checking;
            }
            Session::SignedOut => return FooterState::SignedOut,
            Session::Unreadable(why) => {
                return FooterState::Problem(format!("{UNREADABLE}. {why}"));
            }
            Session::Unconfirmed { error: Some(e), .. } => {
                return FooterState::Problem(format!("Can't reach GitHub. {e}"));
            }
            Session::SignedIn { .. } => {}
        }
        match &self.sync.remote {
            Remote::Idle | Remote::Checking => FooterState::LookingForRepo,
            Remote::Failed(e) => FooterState::Failed(e.clone()),
            Remote::Ready(s) if s.backup.is_none() => FooterState::NoRepo,
            Remote::Ready(_) => FooterState::Sync,
        }
    }

    pub(super) fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let state = self.footer_state(cx);
        let line = |text: String, spinner: bool| {
            h_flex()
                .gap_1p5()
                .text_xs()
                .text_color(muted)
                .when(spinner, |el| el.child(Spinner::new().xsmall()))
                .child(div().flex_1().min_w_0().whitespace_normal().child(text))
        };
        let body = match state {
            FooterState::Checking => {
                line("Checking your GitHub sign-in…".into(), true).into_any_element()
            }
            FooterState::LookingForRepo => {
                line(format!("Looking for {BACKUP_REPO} on GitHub…"), true).into_any_element()
            }
            FooterState::SignedOut | FooterState::FinishSigningIn => {
                let finish = state == FooterState::FinishSigningIn;
                Button::new("shell-config-sign-in")
                    .outline()
                    .small()
                    .w_full()
                    .icon(Lucide::Github)
                    .label(if finish {
                        "Finish Signing In to GitHub"
                    } else {
                        "Sign In to GitHub"
                    })
                    .tooltip(
                        "Sign in once to save and sync your shell files with a private repository",
                    )
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx))
                    .into_any_element()
            }
            FooterState::Problem(why) => v_flex()
                .gap_1()
                .child(line(why, false).text_color(cx.theme().warning))
                .child(
                    Button::new("shell-config-github-try-again")
                        .outline()
                        .small()
                        .w_full()
                        .icon(Lucide::RefreshCw)
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.account.update(cx, |a, cx| match a.session() {
                                Session::Unreadable(_) => a.load(cx),
                                _ => a.check_again(cx),
                            })
                        })),
                )
                .into_any_element(),
            FooterState::Failed(why) => v_flex()
                .gap_1()
                .child(line(why, false).text_color(cx.theme().danger))
                .child(
                    Button::new("shell-config-sync-again")
                        .outline()
                        .small()
                        .w_full()
                        .icon(Lucide::RefreshCw)
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.check_sync(true, cx))),
                )
                .into_any_element(),
            FooterState::NoRepo => {
                let user_files = self
                    .configs
                    .iter()
                    .filter(|c| c.scope == Scope::User)
                    .count();
                Button::new("shell-config-save-github")
                    .outline()
                    .small()
                    .w_full()
                    .icon(Lucide::Github)
                    .label(if self.saving { "Saving to GitHub…" } else { "Save Shell Files to GitHub" })
                    .loading(self.saving)
                    .disabled(self.saving || user_files == 0)
                    .tooltip(format!(
                        "Create the Private Repository {BACKUP_REPO} and Commit Your {user_files} Shell Files"
                    ))
                    .on_click(cx.listener(|this, _, window, cx| this.save_to_github(window, cx)))
                    .into_any_element()
            }
            FooterState::Sync => self.render_sync(cx).into_any_element(),
        };
        v_flex()
            .id("shell-config-github")
            .p_2()
            .gap_1()
            .max_h(px(360.))
            .overflow_y_scroll()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(body)
            .when_some(self.saved.clone(), |el, (message, url)| {
                el.child(
                    h_flex()
                        .gap_1()
                        .child(line(message, false))
                        .when_some(url, |el, url| {
                            el.child(
                                Button::new("shell-config-open-repo")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::ExternalLink)
                                    .tooltip("Open the Repository on GitHub")
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                            )
                        }),
                )
            })
    }

    fn render_sync(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let Some(status) = self.status() else {
            return v_flex();
        };
        let url = status
            .backup
            .as_ref()
            .map(|b| b.repo.html_url.clone())
            .unwrap_or_default();
        let ups = self.upload_count();
        let downs = self.download_count();
        let busy = self.sync.busy;
        let header = h_flex()
            .gap_1()
            .items_center()
            .child(Icon::new(Lucide::Github).xsmall().text_color(muted))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .truncate()
                            .child(format!("{}/{BACKUP_REPO}", status.login)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(self.last_sync_text()),
                    ),
            )
            .child(
                Button::new("shell-config-sync-refresh")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Check Again")
                    .on_click(cx.listener(|this, _, _, cx| this.check_sync(true, cx))),
            )
            .child(
                Button::new("shell-config-open-repo-sync")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ExternalLink)
                    .tooltip("Open the Repository on GitHub")
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        let rows = status
            .files
            .iter()
            .cloned()
            .map(|f| self.render_file(f, cx).into_any_element())
            .collect::<Vec<_>>();
        let actions = h_flex()
            .gap_1()
            .child(
                Button::new("shell-config-upload")
                    .outline()
                    .small()
                    .flex_1()
                    .icon(Lucide::Upload)
                    .label(match busy {
                        Some(b) if b.starts_with("Upload") => b.to_string(),
                        _ => "Upload".to_string(),
                    })
                    .disabled(ups == 0 || busy.is_some())
                    .tooltip(format!("Upload {ups} Ticked Files Changed Here to GitHub"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.sync_upload(None, window, cx)),
                    ),
            )
            .child(
                Button::new("shell-config-download")
                    .outline()
                    .small()
                    .flex_1()
                    .icon(Lucide::Download)
                    .label("Download")
                    .disabled(downs == 0 || busy.is_some())
                    .tooltip(format!(
                        "Replace {downs} Ticked Files Here with GitHub's, Backed Up First"
                    ))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.sync_download(None, window, cx)),
                    ),
            );
        v_flex()
            .gap_1()
            .child(header)
            .children(rows)
            .child(actions)
            .when_some(self.sync.notice.clone(), |el, notice| {
                el.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(muted)
                        .child(div().flex_1().min_w_0().whitespace_normal().child(notice))
                        .when(self.sync.undo.is_some(), |el| {
                            el.child(
                                Button::new("shell-config-undo-download")
                                    .ghost()
                                    .xsmall()
                                    .label("Undo")
                                    .tooltip("Put Back the Files as They Were Before the Download")
                                    .on_click(cx.listener(|this, _, _, cx| this.sync_undo(cx))),
                            )
                        }),
                )
            })
            .when(!self.sync.to_source.is_empty(), |el| {
                let names = self
                    .sync
                    .to_source
                    .iter()
                    .map(|p| sawit_core::recent::tildify(p, std::env::var_os("HOME").map(std::path::PathBuf::from).as_deref()))
                    .collect::<Vec<_>>()
                    .join(", ");
                el.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().text_color(muted).whitespace_normal().child(format!(
                            "Open terminals still run the old {names}: source it there, or open a new shell."
                        )))
                        .child(
                            Button::new("shell-config-source")
                                .outline()
                                .small()
                                .w_full()
                                .icon(Lucide::SquareTerminal)
                                .label("Source in Open Terminals")
                                .tooltip("Runs `source` on the new files in this window's terminals that wait at their prompt")
                                .on_click(cx.listener(|this, _, _, cx| this.source_in_terminals(cx))),
                        ),
                )
            })
    }

    fn render_file(&self, f: FileSync, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let rel = f.rel.clone();
        let movable = f.state.uploads() || f.state.downloads();
        let conflict = f.state == FileState::ChangedOnBoth;
        let color = match f.state {
            FileState::Same => muted,
            FileState::ChangedOnBoth => cx.theme().warning,
            _ => cx.theme().info,
        };
        let has_diff = self
            .sync
            .diffs
            .get(&rel)
            .is_some_and(|d| d.iter().any(|l| !matches!(l, DiffLine::Same(_))));
        let open = self.sync.open_diff.as_deref() == Some(rel.as_str());
        let id = |name: &str| SharedString::from(format!("shell-sync-{name}-{rel}"));
        let row = h_flex()
            .gap_1()
            .items_center()
            .child(
                div().flex_1().min_w_0().child(
                    Checkbox::new(id("pick"))
                        .label(format!("~/{rel}"))
                        .checked(self.sync.chosen.contains(&rel))
                        .disabled(!movable)
                        .on_click(cx.listener({
                            let rel = rel.clone();
                            move |this, on: &bool, _, cx| this.toggle_chosen(&rel, *on, cx)
                        })),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(color)
                    .flex_none()
                    .child(f.state.label()),
            )
            .when(has_diff, |el| {
                el.child(
                    Button::new(id("diff"))
                        .ghost()
                        .xsmall()
                        .icon(if open {
                            Lucide::ChevronUp
                        } else {
                            Lucide::FileDiff
                        })
                        .tooltip(if open {
                            "Hide the Differences"
                        } else {
                            "Show What Differs from GitHub"
                        })
                        .on_click(cx.listener({
                            let rel = rel.clone();
                            move |this, _, _, cx| this.toggle_diff(&rel, cx)
                        })),
                )
            });
        v_flex()
            .gap_0p5()
            .child(row)
            .when(conflict, |el| {
                let (keep_local, keep_github, both) = (f.clone(), f.clone(), f.clone());
                el.child(
                    h_flex()
                        .gap_1()
                        .pl_5()
                        .child(
                            Button::new(id("keep-local"))
                                .ghost()
                                .xsmall()
                                .label("Keep Local")
                                .tooltip("Upload This Mac's Version over GitHub's")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.sync_upload(Some(vec![keep_local.clone()]), window, cx)
                                })),
                        )
                        .child(
                            Button::new(id("keep-github"))
                                .ghost()
                                .xsmall()
                                .label("Keep GitHub's")
                                .tooltip(
                                    "Replace This Mac's Version with GitHub's, Backed Up First",
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.sync_download(Some(vec![keep_github.clone()]), window, cx)
                                })),
                        )
                        .child(
                            Button::new(id("open-both"))
                                .ghost()
                                .xsmall()
                                .label("Open Both")
                                .tooltip("Open Both Versions to Merge by Hand")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.open_both(&both, cx)),
                                ),
                        ),
                )
            })
            .when(open, |el| el.child(self.render_diff(&rel, cx)))
    }

    /// The diff from here to GitHub: `−` lines are this Mac's, `+` lines GitHub's.
    fn render_diff(&self, rel: &str, cx: &App) -> impl IntoElement + use<> {
        let lines = self.sync.diffs.get(rel).cloned().unwrap_or_default();
        let shown = 400;
        let more = lines.len().saturating_sub(shown);
        v_flex()
            .p_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().secondary)
            .text_xs()
            .font_family(cx.theme().mono_font_family.clone())
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("− this Mac   + GitHub"),
            )
            .children(lines.into_iter().take(shown).map(|l| {
                let (sign, text, color) = match l {
                    DiffLine::Same(t) => (" ", t, cx.theme().muted_foreground),
                    DiffLine::Added(t) => ("+", t, cx.theme().success),
                    DiffLine::Removed(t) => ("−", t, cx.theme().danger),
                };
                div()
                    .text_color(color)
                    .truncate()
                    .child(format!("{sign} {text}"))
            }))
            .when(more > 0, |el| el.child(format!("… {more} more lines")))
    }
}
