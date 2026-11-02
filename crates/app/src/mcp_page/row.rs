//! A server's row in the list: what it is and runs, where it's installed, what's wrong, and
//! its buttons.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_catalog::{Auth, SERVERS};
use sawit_core::mcp_clients::Status;

use super::logos::server_logo;
use super::{Kind, McpPage, plural, target_label};
use crate::catalog::{self, Mark, tilde};
use crate::git::account::{self, SignInToGitHub};

impl McpPage {
    pub(super) fn render_server(
        &self,
        s: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let server = &SERVERS[s];
        let total = self.targets.len();
        let installed = (0..total)
            .filter(|t| self.status.get(&(s, *t)) == Some(&Status::Installed))
            .count();
        let other = (0..total)
            .filter(|t| {
                matches!(self.status.get(&(s, *t)), Some(Status::Outdated(_)))
                    && self.other_region((s, *t)).is_none()
            })
            .count();
        // Installed in another Region: installed, not out of date.
        let mut elsewhere: Vec<String> = (0..total)
            .filter_map(|t| self.other_region((s, t)))
            .collect();
        let in_other_regions = elsewhere.len();
        elsewhere.sort();
        elsewhere.dedup();
        let to_sign_in = self.pending_sign_ins(Some(s)).len();
        let checking = server.auth == Auth::Browser
            && (0..total).any(|t| self.checking.contains(&t) && self.is_installed((s, t)));
        let missing = server.missing_program_with(|c| self.programs.get(c).cloned(), self.run_pm());
        let bundled_missing = s == 0 && !self.server_exists;
        let busy = (0..total).any(|t| self.busy.contains(&(s, t)));
        let none = installed + other + in_other_regions == 0;

        let (mark, summary, summary_color) = if busy {
            (Mark::Spinner, "Working…".to_string(), theme.warning)
        } else if none {
            (
                Mark::Icon(Lucide::CircleDashed),
                "Not installed".to_string(),
                muted,
            )
        } else if other > 0 {
            (
                Mark::Icon(Lucide::CircleAlert),
                format!(
                    "{} of {total} · {other} to update",
                    installed + in_other_regions
                ),
                theme.warning,
            )
        } else {
            (
                Mark::Icon(Lucide::CircleCheck),
                format!("Installed · {} of {total}", installed + in_other_regions),
                theme.success,
            )
        };

        let mut notes: Vec<(Lucide, String, Hsla)> = Vec::new();
        if bundled_missing {
            notes.push((
                Lucide::TriangleAlert,
                format!(
                    "{} not found. Build it with: cargo build --release -p sawit-mcp",
                    tilde(&self.server)
                ),
                theme.danger,
            ));
        }
        if let Some(program) = missing {
            notes.push((
                Lucide::TriangleAlert,
                format!("`{program}` isn't installed, so clients can't start this server."),
                theme.warning,
            ));
        } else if let Some(note) = server.note {
            notes.push((Lucide::Info, note.to_string(), muted));
        }
        let bridge_only: Vec<&str> = (0..total)
            .filter(|&t| self.bridge_blocked((s, t)))
            .map(|t| self.targets[t].client.name())
            .fold(Vec::new(), |mut names, name| {
                if !names.contains(&name) {
                    names.push(name);
                }
                names
            });
        if !bridge_only.is_empty() {
            notes.push((
                Lucide::TriangleAlert,
                format!(
                    "{} reach remote servers through `{runner}`, which isn't installed: install Node.js, or pick another package manager.",
                    bridge_only.join(", "),
                    runner = self.run_pm().runner()
                ),
                theme.warning,
            ));
        }
        if !elsewhere.is_empty() {
            notes.push((
                Lucide::Globe,
                format!(
                    "{} in another Region: {}.",
                    plural(in_other_regions, "client has it", "clients have it"),
                    elsewhere.join(", ")
                ),
                muted,
            ));
        }
        if let Auth::EnvKey(var) = server.auth {
            notes.push((
                Lucide::KeyRound,
                format!("Uses an API key: set {var} in the environment the client starts in."),
                muted,
            ));
        }
        // GitHub's server: what the one sign-in lacks, or that there is none.
        let lacks = self.github_lacks(s, cx);
        let signed_out = self.github_gate(s, cx).is_some();
        let gated = self.row_gated(s, cx);
        if !lacks.is_empty() {
            notes.push((
                Lucide::TriangleAlert,
                format!(
                    "Your GitHub sign-in lacks {}, which GitHub's server needs (workflow: changing files in .github/workflows). Sign in again to allow it.",
                    lacks.join(", ")
                ),
                theme.warning,
            ));
        }
        if signed_out && !none {
            notes.push((
                Lucide::TriangleAlert,
                "Sawit is signed out of GitHub: the clients it hands its sign-in to can't reach GitHub until you sign in again. Nothing was uninstalled.".to_string(),
                theme.warning,
            ));
        }
        // What failed or was stopped, where it was started: the row's Uninstall
        // and Install don't open the sheet that lists errors per client.
        let failures: Vec<String> = (0..total)
            .filter_map(|t| {
                let error = self.errors.get(&(s, t))?;
                Some(format!(
                    "Couldn't change {}: {}",
                    target_label(&self.targets[t]),
                    error.lines().next().unwrap_or_default()
                ))
            })
            .collect();
        let kind = Kind::of(s);

        let row = catalog::columns(
            None,
            catalog::tile(server_logo(server, cx), cx),
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(div().text_sm().font_semibold().child(server.name))
                        .child(catalog::tag(Some(kind.icon()), kind.label(), cx))
                        .children(
                            Self::tags(server)
                                .into_iter()
                                .map(|tag| catalog::tag(Some(Lucide::KeyRound), tag, cx)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child(server.about),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .font_family(theme.mono_font_family.clone())
                        .whitespace_normal()
                        .child(server.address_with(&self.region, self.run_pm())),
                )
                .children(notes.into_iter().map(|(icon, text, color)| {
                    h_flex()
                        .gap_1p5()
                        .items_start()
                        .text_xs()
                        .text_color(color)
                        .child(div().pt(px(1.)).child(Icon::new(icon).xsmall()))
                        // Wraps in the column rather than running under Status: a flex row
                        // lays its text out whole unless it may shrink.
                        .child(div().flex_1().min_w_0().whitespace_normal().child(text))
                }))
                .when(!failures.is_empty(), |el| {
                    let more = failures.len().saturating_sub(2);
                    el.child(
                        v_flex()
                            .debug_selector(|| "mcp-row-error".into())
                            .gap_0p5()
                            .text_xs()
                            .text_color(theme.danger)
                            .children(failures.into_iter().take(2).map(|line| {
                                h_flex()
                                    .gap_1p5()
                                    .items_start()
                                    .child(
                                        div().pt(px(1.)).child(Icon::new(Lucide::CircleX).xsmall()),
                                    )
                                    .child(div().whitespace_normal().child(line))
                            }))
                            .when(more > 0, |el| {
                                el.child(format!(
                                    "And {} more: see Manage.",
                                    plural(more, "client", "clients")
                                ))
                            }),
                    )
                }),
            v_flex()
                .gap_1()
                .items_start()
                .child(catalog::pill(mark, summary, summary_color))
                .when(to_sign_in > 0, |el| {
                    el.child(catalog::pill(
                        Mark::Icon(Lucide::KeyRound),
                        format!(
                            "Sign-in needed · {}",
                            plural(to_sign_in, "profile", "profiles")
                        ),
                        theme.warning,
                    ))
                })
                .when(checking && to_sign_in == 0, |el| {
                    el.child(catalog::pill(Mark::Spinner, "Checking sign-in…", muted))
                }),
            h_flex()
                .gap_1()
                .when(busy, |el| {
                    el.child(
                        Button::new(("mcp-stop", s))
                            .outline()
                            .small()
                            .icon(Lucide::CircleStop)
                            .label("Stop")
                            .tooltip("Stop Installing or Removing")
                            .on_click(cx.listener(move |this, _, _, cx| this.stop(s, cx))),
                    )
                })
                .when(!busy && !self.removable(s).is_empty(), |el| {
                    el.child(
                        Button::new(("mcp-uninstall", s))
                            .ghost()
                            .small()
                            .icon(Icon::new(Lucide::Trash).text_color(theme.danger))
                            .accessibility_label(format!(
                                "Uninstall {} from Every Client",
                                server.name
                            ))
                            .tooltip(format!("Uninstall {} from Every Client", server.name))
                            .disabled(self.loading)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_uninstall(s, window, cx)
                            })),
                    )
                })
                .when(to_sign_in > 0, |el| {
                    el.child(
                        Button::new(("mcp-sign-in", s))
                            .outline()
                            .small()
                            .icon(Lucide::KeyRound)
                            .label("Sign In")
                            .disabled(self.loading)
                            .tooltip(format!(
                                "Sign In on {}",
                                plural(to_sign_in, "Profile", "Profiles")
                            ))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let queue = this.pending_sign_ins(Some(s));
                                this.start_sign_in(queue, window, cx)
                            })),
                    )
                })
                .when(!lacks.is_empty() && !signed_out, |el| {
                    el.child(
                        Button::new(("mcp-github-sign-in-again", s))
                            .debug_selector(|| "mcp-github-sign-in-again".into())
                            .outline()
                            .small()
                            .icon(Lucide::KeyRound)
                            .label("Sign In Again")
                            .tooltip(format!(
                                "Sign In to GitHub Again to Allow {}",
                                lacks.join(", ")
                            ))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(SignInToGitHub), cx)
                            }),
                    )
                })
                .when(signed_out, |el| {
                    el.child(
                        account::sign_in_link(("mcp-github-sign-in", s))
                            .debug_selector(|| "mcp-github-sign-in".into()),
                    )
                })
                .child({
                    let open =
                        Button::new(("mcp-open", s))
                            .debug_selector(move || format!("mcp-open-{s}"))
                            .when(none, |b| {
                                b.primary().icon(Lucide::Download).label("Install")
                            })
                            .when(!none, |b| {
                                b.outline().icon(Lucide::Settings2).label("Manage")
                            })
                            .small()
                            // It opens the sheet that installs, which the ellipsis says, as a Mac
                            // button that opens a dialog does.
                            .accessibility_label(if none {
                                format!("Install {}…", server.name)
                            } else {
                                format!("Manage {}…", server.name)
                            })
                            .tooltip(if none {
                                "Choose the Clients to Install Into"
                            } else {
                                "Install, Update or Remove per Client"
                            })
                            .disabled(self.loading || self.targets.is_empty())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_install(s, window, cx)
                            }));
                    // GitHub's Install while signed out: off, saying "Sign in to GitHub to
                    // install", with Sign In to GitHub beside it.
                    if gated {
                        account::gate(open, self.github.read(cx), "install", cx)
                    } else {
                        open
                    }
                }),
        );
        catalog::list_item(row, ("mcp-row", s), server.name)
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(theme.border)
            .hover(|el| el.bg(theme.list_hover))
            .when(selected, |el| el.bg(theme.list_active))
    }
}
