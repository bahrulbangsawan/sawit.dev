//! The New Project page drawn: its two columns side by side from [`WIDE`] (else stacked, the
//! page scrolling as one), the header with Presets, Reset and Create Project, the filter, and
//! the keys.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::stack_builder::{self, CLI_VERSION, PRESETS};

use super::{CARD_GAP, CARD_MIN, COLUMN_GAP, INSET, LEFT_W, NewProjectPage, WIDE};
use crate::catalog;

impl NewProjectPage {
    pub(super) fn wide(&self) -> bool {
        self.width.get() >= WIDE
    }

    /// How many cards share a line: as many as fit at [`CARD_MIN`] wide.
    pub(super) fn columns(&self) -> usize {
        let page = self.width.get().max(CARD_MIN + 2. * INSET);
        let cards = if self.wide() {
            page - 2. * INSET - LEFT_W - COLUMN_GAP
        } else {
            page - 2. * INSET
        };
        ((cards + CARD_GAP) / (CARD_MIN + CARD_GAP)).floor().max(1.) as usize
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let page = cx.entity().downgrade();
        let create_page = page.clone();
        let blocked = stack_builder::validate_project_name(&self.stack.project_name).is_some();
        h_flex()
            .gap_4()
            .flex_wrap()
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(260.))
                    .gap_1()
                    .child(div().text_xl().font_semibold().child("New Project"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "Pick a TypeScript stack from Better-T-Stack's builder. Sawit runs \
                                 create-better-t-stack {CLI_VERSION} in its terminal and opens the project."
                            )),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(
                        Button::new("new-project-presets")
                            .outline()
                            .small()
                            .icon(Lucide::LayoutTemplate)
                            .label("Presets")
                            .dropdown_caret(true)
                            .disabled(self.busy())
                            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                                for preset in PRESETS {
                                    let page = page.clone();
                                    menu = menu.item(PopupMenuItem::new(preset.name).on_click(
                                        move |_, _, cx| {
                                            let _ = page.update(cx, |this, cx| {
                                                this.apply_preset(preset, cx)
                                            });
                                        },
                                    ));
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new("new-project-reset")
                            .ghost()
                            .small()
                            .icon(Lucide::RotateCcw)
                            .label("Reset")
                            .tooltip("Back to the Default Stack")
                            .disabled(self.busy())
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    )
                    .child(
                        // Create Project makes the project and opens it; its menu also offers
                        // making it only.
                        DropdownButton::new("new-project-create-split")
                            .button(
                                Button::new("new-project-create")
                                    .debug_selector(|| "new-project-create".into())
                                    .primary()
                                    .small()
                                    .icon(Lucide::FolderPlus)
                                    .label("Create Project")
                                    .tooltip("Create the Project and Open It in Sawit")
                                    .loading(self.busy())
                                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                            )
                            .disabled(self.busy() || blocked)
                            .menu_accessibility_label("More Ways to Create")
                            .dropdown_menu_with_anchor(Anchor::TopRight, {
                                let page = create_page;
                                move |menu, _, _| {
                                    let page = page.clone();
                                    menu.item(
                                        PopupMenuItem::new("Create Project Only")
                                            .icon(Lucide::FolderPlus)
                                            .on_click(move |_, _, cx| {
                                                let _ = page
                                                    .update(cx, |this, cx| this.create_only(cx));
                                            }),
                                    )
                                }
                            }),
                    ),
            )
    }

    /// A small heading over a part of the left column.
    pub(super) fn heading(text: &'static str, cx: &App) -> Div {
        div()
            .text_xs()
            .font_medium()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    }

    fn render_filter(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let filter = self.filter.as_ref()?;
        Some(
            h_flex()
                .debug_selector(|| "new-project-filter".into())
                .child(catalog::search(filter, cx))
                .into_any_element(),
        )
    }
}

impl Render for NewProjectPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.name.is_none() {
            self.make_inputs(window, cx);
        }
        let measure = {
            let (width, page) = (self.width.clone(), cx.entity_id());
            canvas(
                move |bounds, _, cx| {
                    let now = f32::from(bounds.size.width);
                    if (now - width.get()).abs() >= 1. {
                        width.set(now);
                        // After this frame: a notify while it is drawn may draw nothing more.
                        cx.defer(move |cx| cx.notify(page));
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };
        let sections = self.render_sections(cx);
        let body = if self.wide() {
            v_flex()
                .size_full()
                .p_6()
                .gap_4()
                .child(self.render_header(cx))
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_6()
                        .items_start()
                        .child(
                            div()
                                .id("new-project-left")
                                .debug_selector(|| "new-project-left".into())
                                .w(px(LEFT_W))
                                .flex_none()
                                .h_full()
                                .overflow_y_scroll()
                                .child(self.render_left(cx).pb_4()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .gap_3()
                                .children(self.render_filter(cx))
                                .child(
                                    div()
                                        .id("new-project-cards")
                                        .debug_selector(|| "new-project-cards".into())
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.scroll)
                                        .children(sections),
                                ),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .id("new-project-page")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(
                    v_flex()
                        .p_6()
                        .pb_4()
                        .gap_4()
                        .child(self.render_header(cx))
                        .child(
                            div()
                                .debug_selector(|| "new-project-left".into())
                                .child(self.render_left(cx)),
                        )
                        .children(self.render_filter(cx)),
                )
                .children(sections.into_iter().map(|s| div().px_6().child(s)))
                .into_any_element()
        };
        div()
            .relative()
            .size_full()
            .key_context("NewProjectPage")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &catalog::FocusSearch, window, cx| {
                this.focus_filter(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &catalog::SelectNext, window, cx| {
                    this.step(true, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &catalog::SelectPrevious, window, cx| {
                    this.step(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &catalog::Confirm, _, cx| this.confirm(cx)))
            .bg(cx.theme().background)
            .child(measure)
            .child(body)
    }
}
