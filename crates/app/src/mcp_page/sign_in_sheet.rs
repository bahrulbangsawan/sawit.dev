//! The sign-in sheet: the link to approve in the browser, the address to paste back, and what
//! was signed in.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_auth::is_callback_url;
use sawit_core::mcp_catalog::SERVERS;

use super::logos::client_logo;
use super::{McpPage, Phase, Sheet, SignInFlow, plural, target_label};

impl McpPage {
    pub(super) fn render_sign_in_sheet(
        &self,
        flow: &SignInFlow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let finished = flow.pos >= flow.queue.len();
        let current = flow.queue.get(flow.pos).copied();
        let (s, t) = current
            .or_else(|| flow.queue.last().copied())
            .unwrap_or((0, 0));
        let server = &SERVERS[s];
        let target = &self.targets[t];
        let agent = target.client.name();
        let step = |n: &'static str, title: &'static str| {
            h_flex()
                .gap_2()
                .child(
                    div()
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .bg(theme.primary)
                        .text_color(theme.primary_foreground)
                        .text_xs()
                        .font_semibold()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(n),
                )
                .child(div().text_sm().font_semibold().child(title))
        };
        let address_ok = is_callback_url(&flow.address.read(cx).value());

        let body = if finished {
            let names: Vec<String> = flow
                .done
                .iter()
                .map(|(s, t)| {
                    format!(
                        "{} on {}",
                        SERVERS[*s].name,
                        target_label(&self.targets[*t])
                    )
                })
                .collect();
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(Icon::new(Lucide::CircleCheck).text_color(theme.success))
                        .child(div().text_sm().font_semibold().child(if names.is_empty() {
                            "Nothing was signed in.".to_string()
                        } else {
                            format!("Signed in: {}", plural(names.len(), "sign-in", "sign-ins"))
                        })),
                )
                .children(names.into_iter().map(|n| {
                    div().pl_6().text_xs().text_color(muted).child(n)
                }))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child("The clients saved each sign-in themselves. New sessions can use the tools; running sessions pick them up after /mcp or a restart."),
                )
                .into_any_element()
        } else {
            let link_box = match &flow.link {
                Some(link) => div()
                    .id("mcp-link")
                    .p_2()
                    .rounded(theme.radius)
                    .bg(theme.secondary)
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .whitespace_normal()
                    .line_clamp(3)
                    .child(link.clone())
                    .into_any_element(),
                None => h_flex()
                    .p_2()
                    .gap_2()
                    .rounded(theme.radius)
                    .bg(theme.secondary)
                    .text_xs()
                    .text_color(muted)
                    .child(Spinner::new().xsmall().color(muted))
                    .child(format!("Starting {agent}'s sign-in…"))
                    .into_any_element(),
            };
            let link = flow.link.clone();
            let link_copy = flow.link.clone();
            let (status_icon, status_text, status_color): (AnyElement, String, Hsla) =
                match &flow.phase {
                    Phase::Starting => (
                        Spinner::new().xsmall().color(muted).into_any_element(),
                        "Starting the sign-in…".into(),
                        muted,
                    ),
                    Phase::Waiting => (
                        Spinner::new().xsmall().color(muted).into_any_element(),
                        "Waiting for approval…".into(),
                        muted,
                    ),
                    Phase::Finishing => (
                        Spinner::new().xsmall().color(muted).into_any_element(),
                        "Finishing the sign-in…".into(),
                        muted,
                    ),
                    Phase::Failed(why) => (
                        Icon::new(Lucide::CircleX)
                            .xsmall()
                            .text_color(theme.danger)
                            .into_any_element(),
                        format!("Sign-in didn't finish: {why}"),
                        theme.danger,
                    ),
                };
            let can_paste = flow.phase == Phase::Waiting;
            v_flex()
                .gap_4()
                .child(
                    h_flex()
                        .gap_2()
                        .child(client_logo(target.client, 18., cx))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(format!("{} on {}", server.name, target_label(target))),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(step("1", "Open the link and approve access"))
                        .child(link_box)
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("mcp-open-link")
                                        .primary()
                                        .small()
                                        .icon(Lucide::ExternalLink)
                                        .label("Open in Browser")
                                        .disabled(link.is_none())
                                        .on_click(move |_, _, cx| {
                                            if let Some(link) = &link {
                                                cx.open_url(link);
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("mcp-copy-link")
                                        .outline()
                                        .small()
                                        .icon(if flow.copied { Lucide::Check } else { Lucide::Copy })
                                        .label(if flow.copied { "Copied" } else { "Copy Link" })
                                        .disabled(link_copy.is_none())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(link) = &link_copy {
                                                cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                                            }
                                            if let Some(Sheet::SignIn(flow)) = &mut this.sheet {
                                                flow.copied = true;
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .whitespace_normal()
                                .child(format!(
                                    "Opened on this Mac, the sign-in finishes here on its own after you approve. Sign in to {} with the account you want this profile to use.",
                                    server.name
                                )),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(step("2", "Or paste the address the browser ends on"))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().flex_1().child(Input::new(&flow.address).disabled(!can_paste)))
                                .child(
                                    Button::new("mcp-complete")
                                        .outline()
                                        .label("Complete Sign-In")
                                        .loading(flow.phase == Phase::Finishing)
                                        .disabled(!can_paste || !address_ok)
                                        .on_click(cx.listener(|this, _, _, cx| this.submit_address(cx))),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .whitespace_normal()
                                .child("Used the link on another device, or the page says it can't connect? Copy the whole address from that browser's address bar and paste it here."),
                        )
                        .when_some(flow.problem.clone(), |el, p| {
                            el.child(
                                h_flex()
                                    .gap_1p5()
                                    .items_start()
                                    .text_xs()
                                    .text_color(theme.danger)
                                    .child(div().pt(px(1.)).child(Icon::new(Lucide::TriangleAlert).xsmall()))
                                    .child(div().whitespace_normal().child(p)),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_start()
                        .text_xs()
                        .text_color(status_color)
                        .child(div().pt(px(1.)).child(status_icon))
                        .child(div().flex_1().whitespace_normal().child(status_text))
                        .when(matches!(flow.phase, Phase::Failed(_)), |el| {
                            el.child(
                                Button::new("mcp-retry")
                                    .outline()
                                    .xsmall()
                                    .icon(Lucide::RefreshCw)
                                    .label("Try Again")
                                    .on_click(cx.listener(|this, _, window, cx| this.begin_current(window, cx))),
                            )
                        }),
                )
                .into_any_element()
        };

        let more = flow.queue.len() > 1;
        v_flex()
            .w(px(640.))
            .max_w_full()
            .max_h_full()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .child(
                v_flex()
                    .p_4()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_base()
                                    .font_semibold()
                                    .flex_1()
                                    .child(if more && !finished {
                                        format!("Sign In ({} of {})", flow.pos + 1, flow.queue.len())
                                    } else {
                                        format!("Sign In to {}", server.name)
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "Runs {agent}'s own sign-in for each profile. Sawit never sees your password or tokens."
                            )),
                    ),
            )
            .child(
                div()
                    .id("mcp-sign-in-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(body),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().flex_1())
                    .when(!finished && flow.pos + 1 < flow.queue.len(), |el| {
                        el.child(
                            Button::new("mcp-skip")
                                .ghost()
                                .small()
                                .label("Skip This Profile")
                                .on_click(cx.listener(|this, _, window, cx| this.skip(window, cx))),
                        )
                    })
                    .child(
                        Button::new("mcp-sign-in-close")
                            .when(finished, |b| b.primary())
                            .when(!finished, |b| b.outline())
                            .small()
                            .label(if finished { "Done" } else { "Cancel" })
                            .on_click(cx.listener(|this, _, _, cx| this.close_sheet(cx))),
                    ),
            )
            .into_any_element()
    }
}
