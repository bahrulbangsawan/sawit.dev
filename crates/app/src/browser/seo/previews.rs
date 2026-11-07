//! How the link looks when shared, drawn in GPUI from the page's tags: a search result, and the
//! cards WhatsApp, LinkedIn, Facebook and X show. Approximations in the theme's colours, with the
//! real title, description, host and share image, each cut where those platforms cut it.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::web::seo::Report;

use super::super::panel;

/// A share image's height for a width, at the 1.91:1 Open Graph shape.
const OG_RATIO: f32 = 1.91;

/// `text` cut to `n` characters with an ellipsis, as the platforms do.
pub fn cut(text: &str, n: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= n {
        text.to_string()
    } else {
        format!(
            "{}…",
            text.chars()
                .take(n.saturating_sub(1))
                .collect::<String>()
                .trim_end()
        )
    }
}

/// A favicon in a square slot, or a globe when there is none.
pub fn favicon_slot(image: Option<Arc<RenderImage>>, side: f32, cx: &App) -> AnyElement {
    let slot = div()
        .size(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();
    match image {
        Some(image) => slot
            .child(img(image).size(px(side)).object_fit(ObjectFit::Contain))
            .into_any_element(),
        None => slot
            .child(
                Icon::new(Lucide::Globe)
                    .xsmall()
                    .text_color(cx.theme().muted_foreground),
            )
            .into_any_element(),
    }
}

/// The share image, or a placeholder that says there is none.
fn picture(image: Option<Arc<RenderImage>>, w: Option<f32>, h: f32, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let frame = div()
        .h(px(h))
        .flex_none()
        .when_some(w, |d, w| d.w(px(w)))
        .when(w.is_none(), |d| d.w_full())
        .overflow_hidden()
        .bg(theme.muted);
    match image {
        Some(image) => frame
            .child(img(image).size_full().object_fit(ObjectFit::Cover))
            .into_any_element(),
        None => frame
            .flex()
            .flex_col()
            .gap_1()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(Icon::new(Lucide::ImageOff).small())
            .when(w.is_none_or(|w| w > 100.), |d| d.child("No share image"))
            .into_any_element(),
    }
}

struct Card {
    title: String,
    description: String,
    host: String,
    has_image: bool,
}

fn card(report: &Report) -> Card {
    let f = &report.facts;
    Card {
        title: f
            .og_title
            .clone()
            .or_else(|| f.title.clone())
            .unwrap_or_else(|| f.host.clone()),
        description: f
            .og_description
            .clone()
            .or_else(|| f.description.clone())
            .unwrap_or_default(),
        host: f.host.clone(),
        has_image: f.og_image.is_some() || f.twitter_image.is_some(),
    }
}

pub fn previews(
    report: &Report,
    image: Option<Arc<RenderImage>>,
    favicon: Option<Arc<RenderImage>>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let f = &report.facts;
    let c = card(report);
    let image = image.filter(|_| c.has_image);
    let width = panel::PANEL_WIDTH - 24. - 2.;
    let big = width / OG_RATIO;
    let box_ = || {
        v_flex()
            .w_full()
            .overflow_hidden()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
    };
    let search_title = f.title.clone().unwrap_or_else(|| c.host.clone());
    let search_desc = f.description.clone().unwrap_or_else(|| {
        "No meta description: the search engine picks text from the page.".into()
    });
    let x_title = f.twitter_title.clone().unwrap_or_else(|| c.title.clone());
    let large_x = f.twitter_card.as_deref() == Some("summary_large_image");
    v_flex()
        .gap_4()
        // A search result.
        .child(panel::section(
            "Google search",
            None,
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .size(px(26.))
                                .rounded_full()
                                .bg(theme.muted)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(favicon_slot(favicon.clone(), 16., cx)),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.foreground)
                                        .truncate()
                                        .child(
                                            f.site_name.clone().unwrap_or_else(|| c.host.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .truncate()
                                        .child(report.final_url.clone()),
                                ),
                        ),
                )
                .child(
                    div()
                        .pt_1()
                        .text_base()
                        .text_color(theme.link)
                        .child(cut(&search_title, 60)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(cut(&search_desc, 158)),
                ),
            cx,
        ))
        // WhatsApp: a picture at the side, title, description, host.
        .child(panel::section(
            "WhatsApp",
            None,
            h_flex()
                .w_full()
                .overflow_hidden()
                .rounded(theme.radius_lg)
                .bg(theme.secondary)
                .child(picture(image.clone(), Some(84.), 84., cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .py_1p5()
                        .gap_0p5()
                        .text_xs()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(cut(&c.title, 60)),
                        )
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(cut(&c.description, 80)),
                        )
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(c.host.clone()),
                        ),
                ),
            cx,
        ))
        // LinkedIn: a wide picture, the title, the host.
        .child(panel::section(
            "LinkedIn",
            None,
            box_().child(picture(image.clone(), None, big, cx)).child(
                v_flex()
                    .px_3()
                    .py_2()
                    .gap_0p5()
                    .text_xs()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(cut(&c.title, 70)),
                    )
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(c.host.clone()),
                    ),
            ),
            cx,
        ))
        // Facebook: a wide picture, the host in capitals, the title, the description.
        .child(panel::section(
            "Facebook",
            None,
            box_().child(picture(image.clone(), None, big, cx)).child(
                v_flex()
                    .px_3()
                    .py_2()
                    .gap_0p5()
                    .text_xs()
                    .bg(theme.muted)
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(c.host.to_uppercase()),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(cut(&c.title, 70)),
                    )
                    .when(!c.description.is_empty(), |d| {
                        d.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(c.description.clone()),
                        )
                    }),
            ),
            cx,
        ))
        // X: a large picture with the title over it, or a small card, and the host under it.
        .child(panel::section(
            if large_x {
                "X (large card)"
            } else {
                "X (summary card)"
            },
            None,
            v_flex()
                .gap_1()
                .child(if large_x {
                    v_flex()
                        .w_full()
                        .overflow_hidden()
                        .rounded(px(16.))
                        .border_1()
                        .border_color(theme.border)
                        .relative()
                        .child(picture(image.clone(), None, big, cx))
                        .child(
                            div()
                                .absolute()
                                .left_2()
                                .bottom_2()
                                .max_w(px(width - 32.))
                                .px_1p5()
                                .rounded(px(4.))
                                .bg(gpui_kit::black().opacity(0.7))
                                .text_xs()
                                .text_color(gpui_kit::white())
                                .truncate()
                                .child(cut(&x_title, 70)),
                        )
                        .into_any_element()
                } else {
                    h_flex()
                        .w_full()
                        .overflow_hidden()
                        .rounded(px(16.))
                        .border_1()
                        .border_color(theme.border)
                        .child(picture(image.clone(), Some(96.), 96., cx))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .px_3()
                                .gap_0p5()
                                .text_xs()
                                .child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child(c.host.clone()),
                                )
                                .child(div().text_color(theme.foreground).child(cut(&x_title, 70)))
                                .child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child(cut(&c.description, 90)),
                                ),
                        )
                        .into_any_element()
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("From {}", c.host)),
                ),
            cx,
        ))
}
