//! The cards: one section per category the filter leaves, each option's card, and its logo.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::stack_builder::{Category, StackOption};

use super::{CARD_GAP, NewProjectPage};

impl NewProjectPage {
    /// One section per category shown, its cards wrapping.
    pub(super) fn render_sections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.query(cx);
        // Each card takes its share of the line, with the gap on its right and below; the line
        // reaches one gap past the column, so the last card's gap falls outside it.
        let share = 1. / self.columns() as f32;
        let cursor = self
            .cursor
            .and_then(|ix| self.cards(&query).get(ix).copied())
            .map(|(c, o)| (c, o.id));
        let muted = cx.theme().muted_foreground;
        self.sections(&query)
            .into_iter()
            .map(|(category, options)| {
                let cards: Vec<AnyElement> = options
                    .into_iter()
                    .map(|option| {
                        let at = cursor == Some((category, option.id));
                        div()
                            .w(relative(share))
                            .pr(px(CARD_GAP))
                            .pb(px(CARD_GAP))
                            .child(self.render_card(category, option, at, cx))
                            .into_any_element()
                    })
                    .collect();
                v_flex()
                    .id(SharedString::from(format!(
                        "new-project-section-{}",
                        category.id()
                    )))
                    .debug_selector(move || format!("new-project-section-{}", category.id()))
                    .pb_5()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child(category.label().to_uppercase())
                            .when(category.multiple(), |el| {
                                el.child(div().font_normal().child("· any number"))
                            }),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_stretch()
                            .mr(px(-CARD_GAP))
                            .mb(px(-CARD_GAP))
                            .children(cards),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn render_card(
        &self,
        category: Category,
        option: &'static StackOption,
        at: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let picked = self.stack.is_picked(category, option.id);
        let reason = self.disabled(category, option.id);
        let id = option.id;
        let mut label = option.name.to_string();
        if picked {
            label.push_str(", selected");
        }
        if let Some(reason) = &reason {
            label.push_str(&format!(". {reason}"));
        }
        let border = if picked {
            theme.primary
        } else if at {
            theme.ring
        } else {
            theme.border
        };
        let tag = |text: &'static str, color: Hsla| {
            div()
                .flex_none()
                .px_1()
                .rounded(px(3.))
                .border_1()
                .border_color(color.opacity(0.35))
                .bg(color.opacity(0.1))
                .text_color(color)
                .text_size(px(10.))
                .child(text)
        };
        h_flex()
            .id(SharedString::from(format!(
                "new-project-card-{}-{id}",
                category.id()
            )))
            .debug_selector(move || format!("new-project-card-{}-{id}", category.id()))
            .role(if category.multiple() {
                Role::CheckBox
            } else {
                Role::RadioButton
            })
            .aria_label(label)
            .aria_toggled(picked.into())
            .size_full()
            .min_h(px(64.))
            .p_3()
            .gap_3()
            .items_start()
            .rounded(theme.radius_lg)
            .border_1()
            .when(at && !picked, |el| el.border_2())
            .border_color(border)
            .when(picked, |el| el.bg(theme.primary.opacity(0.06)))
            .when(reason.is_some(), |el| el.border_dashed().opacity(0.55))
            .when(reason.is_none(), |el| {
                el.cursor_pointer()
                    .hover(|el| el.border_color(theme.primary.opacity(0.6)))
            })
            .child(
                div()
                    .size(px(28.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius)
                    .bg(theme.secondary)
                    .child(logo(category, id, 16., cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_medium()
                                    .when(picked, |el| el.text_color(theme.primary))
                                    .child(option.name),
                            )
                            .when(option.experimental, |el| {
                                el.child(tag("Experimental", theme.warning))
                            })
                            .when(option.default && !picked, |el| {
                                el.child(tag("Default", theme.muted_foreground))
                            })
                            .when(picked, |el| {
                                el.child(
                                    Icon::new(Lucide::CircleCheck)
                                        .xsmall()
                                        .text_color(theme.primary),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(option.description),
                    )
                    .when_some(reason.clone(), |el, reason| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(theme.danger)
                                .whitespace_normal()
                                .line_clamp(2)
                                .child(reason),
                        )
                    }),
            )
            .when_some(reason, |el, reason| {
                el.tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.pick(category, id, cx);
            }))
            .into_any_element()
    }
}

/// An option's logo: its Simple Icons mark (assets/stack_logos), else a Lucide glyph, else its
/// initial, `size` px, tinted with the theme.
pub fn logo(category: Category, id: &str, size: f32, cx: &App) -> AnyElement {
    let color = cx.theme().foreground;
    let mark = |name: &str| {
        svg()
            .path(SharedString::from(format!("stack_logos/{name}.svg")))
            .size(px(size))
            .flex_none()
            .text_color(color)
            .into_any_element()
    };
    let glyph = |icon: Lucide| {
        Icon::new(icon)
            .size(px(size))
            .text_color(cx.theme().muted_foreground)
            .into_any_element()
    };
    let initial = |text: &'static str| {
        div()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size * 0.8))
            .font_semibold()
            .text_color(color)
            .child(text)
            .into_any_element()
    };
    if matches!(id, "none" | "false") {
        return glyph(Lucide::Ban);
    }
    match (category, id) {
        (_, "tanstack-router" | "tanstack-start" | "self-tanstack-start") => mark("tanstack"),
        (_, "react-router") => mark("reactrouter"),
        (_, "next" | "self-next") => mark("nextdotjs"),
        (_, "nuxt" | "self-nuxt") => mark("nuxt"),
        (_, "svelte" | "self-svelte") => mark("svelte"),
        (_, "solid" | "self-solid") => mark("solid"),
        (_, "astro" | "self-astro") => mark("astro"),
        (Category::NativeFrontend, _) => mark("expo"),
        (_, "node") => mark("nodedotjs"),
        (_, "workers" | "d1") => mark("cloudflareworkers"),
        (_, "postgres") => mark("postgresql"),
        (_, "prisma-postgres") => mark("prisma"),
        (_, "mongodb-atlas") => mark("mongodb"),
        (_, "better-auth") => mark("betterauth"),
        (_, "oxlint") => mark("oxc"),
        (_, "mcp") => mark("modelcontextprotocol"),
        (Category::Git, "true") => mark("git"),
        (Category::Install, "true") => glyph(Lucide::Download),
        (_, "orpc") => initial("o"),
        (_, "elysia") => initial("E"),
        (_, "polar") => initial("P"),
        (_, "husky") => initial("H"),
        (_, "ultracite") => initial("U"),
        (_, "axiom") => initial("A"),
        (_, "starlight" | "fumadocs") => glyph(Lucide::BookOpen),
        (_, "electrobun") => glyph(Lucide::Monitor),
        (_, "opentui") => glyph(Lucide::Terminal),
        (_, "skills") => glyph(Lucide::Sparkles),
        (_, "evlog") => glyph(Lucide::ScrollText),
        (_, "vite-plus") => glyph(Lucide::Zap),
        (_, "todo") => glyph(Lucide::ListChecks),
        (_, "ai") => glyph(Lucide::Bot),
        (_, name) => mark(name),
    }
}
