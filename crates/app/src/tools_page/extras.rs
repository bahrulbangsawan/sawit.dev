//! What some rows add under their main line: the GitHub CLI's sign-in with Sign In with GitHub
//! CLI, HyperFrames' Run Doctor and its Chrome, Studio's HeyGen sign-in, voxl's permissions and
//! Start, and the settings an uninstall kept, with Delete Settings. Also asking before an
//! install pulls in prerequisites the user didn't pick.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::dev_tools::extras::{self, GhAuth, VOXL_PORT};
use sawit_core::dev_tools::{TOOLS, Tool, find, kept};

use super::{State, ToolsEvent, ToolsPage};
use crate::catalog::{self, tilde};
use crate::rt;

impl ToolsPage {
    /// Reads again what the extras show, after detection: the settings uninstalls kept, the
    /// GitHub CLI's sign-in (off the UI thread: gh asks GitHub), HyperFrames' Chrome.
    pub(super) fn refresh_extras(&mut self, cx: &mut Context<Self>) {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        self.kept = TOOLS
            .iter()
            .filter(|t| !t.keeps.is_empty() && !self.found.contains_key(t.id))
            .map(|t| (t.id, kept(t, &home)))
            .filter(|(_, places)| !places.is_empty())
            .collect::<HashMap<_, _>>();
        let env = |name: &str| std::env::var(name).ok();
        self.chrome = Some(extras::hyperframes_browser(&home, &env));
        let Some(gh) = self.found.get("gh").map(|f| f.path.clone()) else {
            self.gh_auth = None;
            return;
        };
        let rx = rt::spawn(cx, async move { extras::gh_auth(&gh).await });
        cx.spawn(async move |this, cx| {
            if let Ok(auth) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.gh_auth = auth;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Installs `id` once detection has said what is installed, asking first as the row's
    /// Install does: what Install GitHub CLI elsewhere in Sawit asks of the page.
    pub fn install_when_detected(&mut self, id: &str, cx: &mut Context<Self>) {
        self.pending_install = find(id).map(|t| t.id);
        cx.notify();
    }

    /// Install from a row (its button, or ↩): when the install also needs tools the user
    /// didn't pick and doesn't have (FFmpeg for HyperFrames), it asks first, naming them.
    pub(super) fn request_install(
        &mut self,
        tool: &'static Tool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let extra: Vec<&'static str> = self
            .plan_for(&[tool.id])
            .iter()
            .filter(|t| t.id != tool.id)
            .map(|t| t.name)
            .collect();
        if extra.is_empty() {
            self.enqueue(&[tool.id], cx);
            return;
        }
        let detail = format!(
            "{} needs {}, which {} installed. Sawit installs {} first.",
            tool.name,
            extra.join(", "),
            if extra.len() == 1 { "isn't" } else { "aren't" },
            if extra.len() == 1 { "it" } else { "them" },
        );
        let install = format!("Install {}", extra.len() + 1);
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Install {} and What It Needs?", tool.name),
            Some(&detail),
            &[install.as_str(), "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| this.enqueue(&[tool.id], cx));
            }
        })
        .detach();
    }

    /// Asks, then deletes the settings `tool`'s uninstall kept. Its own choice, never part of
    /// the uninstall.
    fn confirm_delete_kept(
        &mut self,
        tool: &'static Tool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(places) = self.kept.get(tool.id).cloned() else {
            return;
        };
        let list = places
            .iter()
            .map(|p| tilde(p))
            .collect::<Vec<_>>()
            .join("\n");
        let answer = crate::confirm::Confirm::new(
            format!("Delete {}'s settings?", tool.name),
            "Delete Settings",
        )
        .thing(tool.name)
        .detail("This can't be undone.")
        .items(list.lines().map(str::to_string).collect::<Vec<_>>())
        .ask(window, cx);
        cx.spawn(async move |this, cx| {
            if !answer.await {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                let failed: Vec<String> = places
                    .iter()
                    .filter_map(|p| {
                        let gone = if p.is_dir() {
                            std::fs::remove_dir_all(p)
                        } else {
                            std::fs::remove_file(p)
                        };
                        gone.err().map(|e| format!("{}: {e}", tilde(p)))
                    })
                    .collect();
                if !failed.is_empty() {
                    this.states.insert(
                        tool.id,
                        State::Failed(format!("Couldn't delete {}", failed.join("; ")), None),
                    );
                }
                this.refresh_extras(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts voxl in a new terminal: on this Mac only, with a new random password that is in
    /// the terminal's environment, never in its command line.
    fn start_voxl(&mut self, cx: &mut Context<Self>) {
        match extras::voxl_launch(VOXL_PORT) {
            Ok(launch) => cx.emit(ToolsEvent::Launch {
                title: "voxl".into(),
                command: launch.command,
                env: launch.env,
            }),
            Err(e) => {
                self.states.insert(
                    "voxl",
                    State::Failed(format!("Couldn't make a password: {e}"), None),
                );
                cx.notify();
            }
        }
    }

    /// The line under `tool`'s row, when it has one.
    pub(super) fn render_extras(
        &self,
        ix: usize,
        tool: &'static Tool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let installed = self.installed(tool.id) && !self.busy(tool.id);
        let muted = cx.theme().muted_foreground;
        let note = |text: String| -> AnyElement {
            div()
                .min_w_0()
                .whitespace_normal()
                .text_xs()
                .text_color(muted)
                .child(text)
                .into_any_element()
        };
        let button = |id: &'static str, icon: Lucide, label: &'static str| {
            Button::new((id, ix))
                .outline()
                .xsmall()
                .icon(icon)
                .label(label)
        };
        let mut parts: Vec<AnyElement> = Vec::new();
        match tool.id {
            "gh" if installed => match &self.gh_auth {
                Some(GhAuth::SignedIn(login)) => parts.push(note(match login {
                    Some(login) => format!("GitHub CLI is signed in as @{login}."),
                    None => "GitHub CLI is signed in.".into(),
                })),
                Some(GhAuth::SignedOut) => {
                    parts.push(note(
                        "GitHub CLI isn't signed in. Sawit's own GitHub sign-in is separate."
                            .into(),
                    ));
                    parts.push(
                        button("tool-gh-login", Lucide::LogIn, "Sign In with GitHub CLI")
                            .tooltip("Runs gh auth login in a Terminal")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(ToolsEvent::RunInTerminal("gh auth login".into()))
                            }))
                            .into_any_element(),
                    );
                }
                None => {}
            },
            "hyperframes" if installed => {
                parts.push(
                    button("tool-doctor", Lucide::Stethoscope, "Run Doctor")
                        .tooltip("Runs hyperframes doctor in a Terminal")
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(ToolsEvent::RunInTerminal("hyperframes doctor".into()))
                        }))
                        .into_any_element(),
                );
                if !self.installed("ffmpeg") {
                    parts.push(note(
                        "FFmpeg isn't installed: rendering video needs it.".into(),
                    ));
                }
                if matches!(self.chrome, Some(None)) {
                    parts.push(note(
                        "No Chrome or Chromium found. Run hyperframes browser ensure to download one, or set HYPERFRAMES_BROWSER_PATH."
                            .into(),
                    ));
                }
            }
            "hyperframes-studio" if self.states.get(tool.id) == Some(&State::Done) => {
                parts.push(note(
                    "Finish in HeyGen's installer that opened, then sign in with your HeyGen account in the app."
                        .into(),
                ));
            }
            "hyperframes-studio" if installed => parts.push(note(
                "Sign in with your free HeyGen account in the app. Sawit never handles that sign-in."
                    .into(),
            )),
            "voxl" if installed => {
                parts.push(note(
                    "Shares this Mac's screen and takes mouse and keyboard input. Sawit starts it on this Mac only (127.0.0.1), with a new password."
                        .into(),
                ));
                for privacy in extras::privacy_needs("voxl") {
                    let granted = privacy.granted();
                    let url = privacy.settings_url();
                    let state = if granted { "Allowed" } else { "Not Allowed" };
                    parts.push(
                        catalog::tag(None, format!("{}: {state}", privacy.name()), cx)
                            .into_any_element(),
                    );
                    if !granted {
                        parts.push(
                            Button::new(SharedString::from(format!(
                                "tool-privacy-{}-{ix}",
                                privacy.name()
                            )))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Settings)
                            .label("Open Settings")
                            .tooltip(format!("Privacy & Security, {}", privacy.name()))
                            .on_click(move |_, _, cx| cx.open_url(url))
                            .into_any_element(),
                        );
                    }
                }
                parts.push(
                    button("tool-voxl-start", Lucide::Play, "Start")
                        .tooltip("Starts voxl on 127.0.0.1:6901 in a Terminal, with a New Password")
                        .on_click(cx.listener(|this, _, _, cx| this.start_voxl(cx)))
                        .into_any_element(),
                );
            }
            _ => {}
        }
        if let Some(places) = self.kept.get(tool.id).filter(|_| !self.installed(tool.id)) {
            let list = places
                .iter()
                .map(|p| tilde(p))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(note(format!("Settings kept in {list}.")));
            parts.push(
                Button::new(("tool-delete-kept", ix))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(Lucide::Trash).text_color(cx.theme().danger))
                    .label("Delete Settings…")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(tool) = find(tool.id) {
                            this.confirm_delete_kept(tool, window, cx)
                        }
                    }))
                    .into_any_element(),
            );
        }
        (!parts.is_empty()).then(|| {
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .children(parts)
                .into_any_element()
        })
    }
}
