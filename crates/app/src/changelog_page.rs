//! Version & changelog page: the running version, update status with
//! "Restart to Update", notes of a newer release and the bundled
//! CHANGELOG.md.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::text::TextView;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::update::{REPO, Version};

use crate::code_actions::code_block_actions;
use crate::update::{UpdateState, Updater, updater};

/// The changelog as of this build (kept current by scripts/release.sh).
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

pub struct ChangelogPage {
    updater: Entity<Updater>,
    _observe: Subscription,
}

impl ChangelogPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let updater = updater(cx);
        Self {
            _observe: cx.observe(&updater, |_, _, cx| cx.notify()),
            updater,
        }
    }

    fn status(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let u = self.updater.read(cx);
        let (text, button) = match &u.state {
            UpdateState::Ready { release, .. } => (
                format!(
                    "Version {} is downloaded and ready to install.",
                    release.version
                ),
                Some("Restart to Update"),
            ),
            UpdateState::Available(release) => (
                format!("Version {} is available on GitHub.", release.version),
                Some("Open Release Page"),
            ),
            UpdateState::Downloading(v) => (format!("Downloading version {v}…"), None),
            UpdateState::Idle => match &u.latest {
                Some(_) => ("Sawit is up to date.".to_string(), None),
                None => ("Sawit hasn't checked for updates yet.".to_string(), None),
            },
        };
        h_flex()
            .gap_3()
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .child(format!("Sawit {}", Version::current())),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(text),
                    ),
            )
            .child(
                Button::new("check-update")
                    .ghost()
                    .small()
                    .icon(Lucide::RefreshCw)
                    .label("Check for Updates")
                    .tooltip("Check GitHub for the Latest Release")
                    .on_click(
                        cx.listener(|this, _, _, cx| this.updater.update(cx, |u, cx| u.check(cx))),
                    ),
            )
            .children(button.map(|label| {
                Button::new("restart-update")
                    .primary()
                    .small()
                    .label(label)
                    .on_click(
                        cx.listener(|this, _, _, cx| {
                            this.updater.update(cx, |u, cx| u.restart(cx))
                        }),
                    )
            }))
    }

    fn markdown(&self, cx: &App) -> SharedString {
        let mut text = String::new();
        if let Some(release) = self.updater.read(cx).pending() {
            text.push_str(&format!(
                "## New in {}\n\n{}\n\n---\n\n",
                release.version,
                release.notes.trim()
            ));
        }
        text.push_str(CHANGELOG);
        text.push_str(&format!(
            "\n\n---\n\nAll releases: [github.com/{REPO}/releases](https://github.com/{REPO}/releases)\n"
        ));
        text.into()
    }
}

impl Render for ChangelogPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(div().p_4().pb_0().child(self.status(cx)))
            .child(
                div().flex_1().min_h_0().child(
                    TextView::markdown("changelog", self.markdown(cx))
                        .scrollable(true)
                        .selectable(true)
                        .code_block_actions(code_block_actions)
                        .on_link_click(|url, _, _, cx| {
                            if url.starts_with("https://") {
                                cx.open_url(url);
                            }
                        })
                        .p_6(),
                ),
            )
    }
}
