//! The left column: the name, the location, the command with the package manager it runs with,
//! what the last pick changed, the stack as tags, the create's status, and the category nav.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::package_manager::{self, Use};
use sawit_core::stack_builder::{self, Category, Pm};

use super::cards::logo;
use super::{Creating, NewProjectPage};
use crate::catalog::{self, Mark, tilde};

impl NewProjectPage {
    pub(super) fn render_left(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let name_error = stack_builder::validate_project_name(&self.stack.project_name);
        let command = stack_builder::command(&self.stack);
        let pms = self.pms.clone().unwrap_or_default();
        let pm_menu = crate::package_managers::menu(
            "new-project-pm",
            &pms,
            Use::Run,
            Some(self.stack.package_manager.package_manager()),
            cx.listener(|this, pm: &package_manager::PackageManager, _, cx| {
                if let Some(pm) = Pm::from_package_manager(*pm) {
                    this.pick(Category::PackageManager, pm.id(), cx);
                }
            }),
        );
        let quiet = stack_builder::quiet_flags(&self.stack).join(" ");
        v_flex()
            .gap_4()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(Self::heading("Project Name", cx))
                    .when_some(self.name.as_ref(), |el, name| {
                        el.child(Input::new(name).small())
                    })
                    .when_some(name_error, |el, error| {
                        el.child(
                            catalog::label("new-project-name-error", error)
                                .text_xs()
                                .text_color(theme.danger),
                        )
                    }),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(Self::heading("Location", cx))
                    .child(
                        Button::new("new-project-location")
                            .outline()
                            .small()
                            .w_full()
                            .icon(Lucide::FolderOpen)
                            .label(tilde(&self.location))
                            .tooltip("Choose the Folder to Create the Project In")
                            .disabled(self.busy())
                            .on_click(cx.listener(|this, _, _, cx| this.choose_location(cx))),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Self::heading("Command", cx))
                            .child(div().flex_1())
                            .children(pm_menu)
                            .child(
                                Button::new("new-project-copy")
                                    .outline()
                                    .xsmall()
                                    .icon(if self.copied {
                                        Lucide::Check
                                    } else {
                                        Lucide::Copy
                                    })
                                    .label(if self.copied { "Copied" } else { "Copy Command" })
                                    .on_click(cx.listener(|this, _, _, cx| this.copy_command(cx))),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "new-project-command".into())
                            .p_3()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.secondary)
                            .font_family(theme.mono_font_family.clone())
                            .text_xs()
                            .whitespace_normal()
                            .child(catalog::label("new-project-command-text", command)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(format!(
                                "Create Project also passes {quiet}, so the CLI doesn't stop to ask."
                            )),
                    ),
            )
            .children(self.render_note(cx))
            .child(self.render_tags(cx))
            .children(self.render_status(cx))
            .child(self.render_nav(cx))
    }

    /// What the last pick changed, and upstream's desktop build note.
    fn render_note(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notes = stack_builder::resolve(&self.stack).notes;
        if self.note.is_empty() && notes.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let lines = self
            .note
            .iter()
            .map(|c| c.message.clone())
            .chain(notes)
            .enumerate()
            .map(|(ix, text)| {
                h_flex()
                    .gap_1p5()
                    .items_start()
                    .child(
                        div().pt(px(2.)).child(
                            Icon::new(Lucide::Info)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        ),
                    )
                    .child(
                        catalog::label(("new-project-note", ix), text)
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal(),
                    )
            });
        Some(
            v_flex()
                .debug_selector(|| "new-project-note".into())
                .gap_1()
                .px_3()
                .py_2()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.info.opacity(0.35))
                .bg(theme.info.opacity(0.08))
                .text_xs()
                .children(lines)
                .into_any_element(),
        )
    }

    /// The stack as tags, each with a × that takes it out.
    fn render_tags(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let tags = stack_builder::selected(&self.stack)
            .into_iter()
            .enumerate()
            .map(|(ix, (category, option))| {
                let id = option.id;
                h_flex()
                    .id(("new-project-tag", ix))
                    .role(Role::ListItem)
                    .aria_label(format!("{}: {}", category.label(), option.name))
                    .h(px(24.))
                    .pl_1p5()
                    .pr_0p5()
                    .gap_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary)
                    .text_xs()
                    .child(logo(category, id, 12., cx))
                    .child(option.name)
                    .child(
                        Button::new(("new-project-untag", ix))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::X).xsmall())
                            .accessibility_label(format!("Remove {}", option.name))
                            .disabled(self.busy())
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.unpick(category, id, cx)),
                            ),
                    )
            });
        v_flex()
            .gap_1p5()
            .child(Self::heading("Your Stack", cx))
            .child(h_flex().flex_wrap().gap_1p5().children(tags))
    }

    /// Creating, failed, made, or why Create Project didn't start.
    fn render_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let name = |dir: &PathBuf| tilde(dir);
        let box_ = |color: Hsla| {
            h_flex()
                .gap_2()
                .px_3()
                .py_2()
                .items_start()
                .rounded(theme.radius)
                .border_1()
                .border_color(color.opacity(0.35))
                .bg(color.opacity(0.08))
                .text_xs()
        };
        Some(match (&self.creating, &self.problem) {
            (Creating::Running { dir, .. }, _) => catalog::pill(
                Mark::Spinner,
                format!("Creating {}…", name(dir)),
                theme.primary,
            )
            .into_any_element(),
            (Creating::Done(dir), _) => catalog::pill(
                Mark::Icon(Lucide::CircleCheck),
                format!("Created {}, opening it…", name(dir)),
                theme.success,
            )
            .into_any_element(),
            (Creating::Failed { .. }, _) => box_(theme.danger)
                .child(
                    Icon::new(Lucide::TriangleAlert)
                        .small()
                        .text_color(theme.danger),
                )
                .child(
                    catalog::label("new-project-failed", "Creation failed, see the terminal.")
                        .flex_1(),
                )
                .child(
                    Button::new("new-project-retry")
                        .outline()
                        .xsmall()
                        .icon(Lucide::RotateCcw)
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                )
                .into_any_element(),
            (Creating::Idle, Some(problem)) => box_(theme.warning)
                .child(
                    Icon::new(Lucide::TriangleAlert)
                        .small()
                        .text_color(theme.warning),
                )
                .child(
                    catalog::label("new-project-problem", problem.clone())
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal(),
                )
                .into_any_element(),
            (Creating::Idle, None) => return None,
        })
    }

    /// The categories as chips that scroll to their section; a dot when one is picked.
    fn render_nav(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let query = self.query(cx);
        let chips = self.sections(&query).into_iter().map(|(category, _)| {
            let chosen = stack_builder::selected(&self.stack)
                .iter()
                .any(|(c, _)| *c == category);
            Button::new(SharedString::from(format!(
                "new-project-nav-{}",
                category.id()
            )))
            .ghost()
            .xsmall()
            .child(
                h_flex()
                    .gap_1()
                    .child(div().size(px(5.)).rounded_full().bg(if chosen {
                        theme.primary
                    } else {
                        theme.muted_foreground.opacity(0.4)
                    }))
                    .child(category.label()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let query = this.query(cx);
                this.scroll_to(category, &query);
                cx.notify();
            }))
        });
        v_flex()
            .gap_1p5()
            .child(Self::heading("Jump To", cx))
            .child(h_flex().flex_wrap().gap_1().children(chips))
    }
}
