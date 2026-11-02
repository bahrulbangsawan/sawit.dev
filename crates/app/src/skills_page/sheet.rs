//! A pack's sheet: the way it installs, the agents ticked, what each of its buttons acts on,
//! the commands shown before they run, and the sheet drawn over the page.

use std::collections::HashSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::package_manager::{PackageManager, Use};
use sawit_core::skills::{self, Action, Method, Pack, Status};

use super::{Pick, Sheet, SkillsPage, pack_logo, plural};
use crate::catalog::tilde;

impl SkillsPage {
    // Sheet

    pub(super) fn open_sheet(
        &mut self,
        pack: &'static Pack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let method = pack.methods[0];
        // What has it, to update or remove; else the agents the first way reaches.
        let mut selected: HashSet<usize> = self.having(pack).into_iter().collect();
        if selected.is_empty() {
            selected = (0..self.report.targets.len())
                .filter(|&t| method.reaches(&self.report.targets[t]))
                .take(1)
                .collect();
        }
        self.sheet = Some(Sheet {
            pack,
            method,
            selected,
        });
        self.sheet_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.refocus = true;
        cx.notify();
    }

    /// The ticked agents an action of the sheet acts on.
    pub(super) fn picked(&self, pick: Pick) -> Vec<usize> {
        let Some(sheet) = &self.sheet else {
            return Vec::new();
        };
        let mut out: Vec<usize> = sheet
            .selected
            .iter()
            .copied()
            .filter(|&t| t < self.report.targets.len())
            .filter(|&t| {
                let reaches = sheet.method.reaches(&self.report.targets[t]);
                let status = self.status(sheet.pack, t);
                match pick {
                    Pick::Install => !status.full() && reaches,
                    Pick::Update => status.full() && reaches,
                    Pick::Remove => status.any(),
                }
            })
            .collect();
        out.sort();
        out
    }

    /// The script `action` runs on `targets` from the sheet.
    pub(super) fn sheet_script(
        &self,
        action: Action,
        targets: &[usize],
    ) -> Result<Vec<String>, String> {
        let Some(sheet) = &self.sheet else {
            return Err("No sheet".into());
        };
        let places: Vec<skills::Place> = targets
            .iter()
            .map(|&t| (&self.report.targets[t], &self.report.installed[t]))
            .collect();
        skills::script(
            sheet.pack,
            sheet.method,
            action,
            &places,
            self.pm.unwrap_or(PackageManager::Npm),
            &self.env,
        )
    }

    pub(super) fn render_sheet(&self, sheet: &Sheet, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let pack = sheet.pack;
        let method = sheet.method;
        let install = self.picked(Pick::Install);
        let update = self.picked(Pick::Update);
        let remove = self.picked(Pick::Remove);
        let busy = self.busy(pack.id);
        let blocked = self.blocked(method);
        // What Install (else Update) would run.
        let preview = if !install.is_empty() {
            self.sheet_script(Action::Install, &install)
        } else if !update.is_empty() {
            self.sheet_script(Action::Update, &update)
        } else {
            Err("Tick the agents to install into.".to_string())
        };
        let ways = pack.methods.iter().copied().enumerate().map(|(ix, m)| {
            Button::new(("skills-way", ix))
                .ghost()
                .xsmall()
                .selected(m == method)
                .label(m.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(sheet) = &mut this.sheet {
                        sheet.method = m;
                    }
                    cx.notify();
                }))
        });
        let pm_menu = method
            .uses_package_manager()
            .then(|| {
                crate::package_managers::menu(
                    "skills-pm",
                    &self.pms,
                    Use::Run,
                    self.pm,
                    cx.listener(|this, pm: &PackageManager, _, cx| {
                        this.pm = Some(*pm);
                        crate::package_managers::remember(*pm);
                        cx.notify();
                    }),
                )
            })
            .flatten();
        let rows = self.report.targets.iter().enumerate().map(|(t, target)| {
            let status = self.status(pack, t);
            let reaches = method.reaches(target);
            let (badge, color) = match status {
                Status::Installed { plugin: true } => {
                    ("Installed (plugin)".to_string(), theme.success)
                }
                Status::Installed { plugin: false } => ("Installed".to_string(), theme.success),
                // Sawit's skill, edited there or another Sawit's version.
                Status::Differs => ("Differs from Sawit".to_string(), theme.warning),
                Status::Partial { have, of } => (format!("{have} of {of} skills"), theme.warning),
                Status::Missing if !reaches => ("Not with this way".to_string(), muted),
                Status::Missing => ("Not installed".to_string(), muted),
            };
            let checked = sheet.selected.contains(&t);
            h_flex()
                .px_3()
                .py_1p5()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div().w(px(200.)).flex_none().child(
                        Checkbox::new(("skills-target", t))
                            .label(target.label.clone())
                            .checked(checked)
                            .disabled(!reaches && !status.any())
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if let Some(sheet) = &mut this.sheet {
                                    if *checked {
                                        sheet.selected.insert(t);
                                    } else {
                                        sheet.selected.remove(&t);
                                    }
                                }
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .font_family(mono.clone())
                        .child(tilde(&target.dir)),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .w(px(148.))
                        .flex_none()
                        .text_xs()
                        .text_color(color)
                        .child(div().size(px(6.)).rounded_full().bg(color))
                        .child(badge),
                )
        });
        let copy = preview.as_ref().ok().map(|lines| lines.join("\n"));
        v_flex()
            .id("skills-sheet")
            .role(Role::Dialog)
            .aria_label(pack.name)
            .w(px(760.))
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
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(pack_logo(pack, cx))
                            .child(
                                div()
                                    .text_base()
                                    .font_semibold()
                                    .flex_1()
                                    .child(format!("Install {}", pack.name)),
                            )
                            .child(
                                Button::new("skills-sheet-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::X)
                                    .accessibility_label("Close")
                                    .tooltip("Close")
                                    .on_click(cx.listener(|this, _, _, cx| this.close_sheet(cx))),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "by {} · {} · {}",
                                pack.by,
                                plural(pack.skills.len(), "skill", "skills"),
                                pack.repo.trim_start_matches("https://")
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(div().text_xs().text_color(muted).child("Install with"))
                            .child(
                                h_flex()
                                    .p(px(2.))
                                    .gap(px(2.))
                                    .rounded(theme.radius)
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(theme.secondary)
                                    .children(ways),
                            )
                            .when_some(pm_menu, |el, menu| {
                                el.child(div().text_xs().text_color(muted).child("run by"))
                                    .child(menu)
                            }),
                    )
                    .when_some(blocked.clone(), |el, why| {
                        el.child(
                            h_flex()
                                .gap_1p5()
                                .items_start()
                                .text_xs()
                                .text_color(theme.warning)
                                .child(
                                    div()
                                        .pt(px(1.))
                                        .child(Icon::new(Lucide::TriangleAlert).xsmall()),
                                )
                                .child(div().whitespace_normal().child(why)),
                        )
                    }),
            )
            .child(
                div()
                    .id("skills-sheet-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(v_flex().children(rows)),
            )
            .child(
                v_flex()
                    .p_3()
                    .gap_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().text_xs().text_color(muted).child(
                                if !install.is_empty() {
                                    "Runs in your login shell:"
                                } else {
                                    "Updating runs:"
                                },
                            ))
                            .when_some(copy, |el, text| {
                                el.child(
                                    Button::new("skills-sheet-copy")
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::Copy)
                                        .accessibility_label("Copy the Commands")
                                        .tooltip("Copy the Commands")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                text.clone(),
                                            ))
                                        }),
                                )
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "skills-sheet-commands".into())
                            .p_2()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.secondary)
                            .text_xs()
                            .font_family(mono)
                            .text_color(if preview.is_ok() {
                                theme.foreground
                            } else {
                                muted
                            })
                            .whitespace_normal()
                            .child(match &preview {
                                Ok(lines) => lines.join("\n"),
                                Err(why) => why.clone(),
                            }),
                    ),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().flex_1())
                    .child(
                        Button::new("skills-sheet-remove")
                            .debug_selector(|| "skills-sheet-remove".into())
                            .danger()
                            .outline()
                            .small()
                            .label(format!("Remove from {}", remove.len()))
                            .disabled(remove.is_empty() || busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_remove(pack, remove.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new("skills-sheet-update")
                            .outline()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .label(format!("Update {}", update.len()))
                            .tooltip(match method {
                                Method::Bundled { .. } => {
                                    "Copy This Sawit's Version Where It's Installed"
                                }
                                _ => "Fetch the Latest Where It's Installed",
                            })
                            .disabled(update.is_empty() || busy || blocked.is_some())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.run_sheet(Action::Update, cx)),
                            ),
                    )
                    .child(
                        Button::new("skills-sheet-install")
                            .debug_selector(|| "skills-sheet-install".into())
                            .primary()
                            .small()
                            .label(format!("Install to {}", install.len()))
                            .loading(busy)
                            .disabled(install.is_empty() || busy || blocked.is_some())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.run_sheet(Action::Install, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
