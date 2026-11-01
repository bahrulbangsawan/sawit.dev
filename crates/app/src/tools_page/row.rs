//! A Dev Tools row: the tool's logo, name, prerequisites, path or command and status, its
//! actions, and under it why Sawit leaves it alone, its last line of output, its failure or its
//! output.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::dev_tools::{Found, Tool, find, install_command, npm_package};
use sawit_core::package_manager::{PackageManager, Use};

use super::{Job, State, ToolsPage, category_name, logo};
use crate::catalog::{self, Mark, tilde};

impl ToolsPage {
    pub(super) fn render_row(
        &self,
        ix: usize,
        tool: &'static Tool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let id = tool.id;
        let state = self.states.get(id).cloned();
        let found = self.found.get(id);
        let installed = self.installed(id);
        let busy = self.busy(id);
        let newer = self.newer(id).cloned();
        let upgrading = self.upgrades.contains(id);

        let (mark, badge, color): (Mark, String, Hsla) = match (&state, found) {
            (Some(State::Queued { for_: Some(f) }), _) => {
                (Mark::Icon(Lucide::Clock), format!("Queued for {f}"), muted)
            }
            (Some(State::Queued { for_: None }), _) => {
                (Mark::Icon(Lucide::Clock), "Queued".into(), muted)
            }
            (Some(State::Installing), _) if upgrading => {
                (Mark::Spinner, "Upgrading…".into(), theme.warning)
            }
            (Some(State::Installing), _) => (Mark::Spinner, "Installing…".into(), theme.warning),
            (Some(State::Uninstalling), _) => {
                (Mark::Spinner, "Uninstalling…".into(), theme.warning)
            }
            (Some(State::Removed), _) => (
                Mark::Icon(Lucide::CircleMinus),
                "Removed · restart shell".into(),
                muted,
            ),
            (Some(State::Stopped), _) => (
                Mark::Icon(Lucide::CircleStop),
                "Stopped".into(),
                theme.warning,
            ),
            (Some(State::Failed(..)), _) => {
                (Mark::Icon(Lucide::CircleX), "Failed".into(), theme.danger)
            }
            (Some(State::Skipped(need)), _) => (
                Mark::Icon(Lucide::TriangleAlert),
                format!("Skipped · needs {need}"),
                theme.danger,
            ),
            (Some(State::InTerminal), _) => (
                Mark::Icon(Lucide::SquareTerminal),
                "In terminal".into(),
                theme.warning,
            ),
            (Some(State::Done), None) if !self.loading => (
                Mark::Icon(Lucide::CircleCheck),
                "Installed · restart shell".into(),
                theme.success,
            ),
            (
                _,
                Some(Found {
                    version: Some(v), ..
                }),
            ) if newer.is_some() => (
                Mark::Icon(Lucide::CircleArrowUp),
                format!("Update · {v} → {}", newer.as_deref().unwrap_or_default()),
                theme.info,
            ),
            (_, Some(f)) => (
                Mark::Icon(Lucide::CircleCheck),
                f.version
                    .as_ref()
                    .map_or("Installed".into(), |v| format!("Installed · {v}")),
                theme.success,
            ),
            (Some(State::Done), None) => (
                Mark::Icon(Lucide::CircleCheck),
                "Installed".into(),
                theme.success,
            ),
            (None, None) if self.loading => (Mark::Spinner, "Checking…".into(), muted),
            (None, None) => (
                Mark::Icon(Lucide::CircleDashed),
                "Not installed".into(),
                muted,
            ),
        };

        let retry = || {
            Button::new(("tool-retry", ix))
                .primary()
                .small()
                .icon(Lucide::RotateCcw)
                .label("Retry")
        };
        let action = match &state {
            Some(State::Installing | State::Uninstalling) => Some(
                Button::new(("tool-stop", ix))
                    .outline()
                    .small()
                    .icon(Lucide::CircleStop)
                    .label("Stop")
                    .tooltip(match state {
                        Some(State::Installing) if upgrading => "Stop the Upgrade",
                        Some(State::Installing) => "Stop the Installer",
                        _ => "Stop the Uninstaller",
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.stop_running(cx))),
            ),
            Some(State::Queued { .. }) => Some(
                Button::new(("tool-dequeue", ix))
                    .ghost()
                    .small()
                    .icon(Lucide::X)
                    .label("Cancel")
                    .tooltip("Take It Out of the Queue")
                    .on_click(cx.listener(move |this, _, _, cx| this.dequeue(id, cx))),
            ),
            // A copy found after its uninstall worked: nothing to run again (the trash button
            // offers that copy's removal when Sawit has one).
            Some(State::Failed(_, None)) => None,
            Some(State::Failed(_, Some(Job::Uninstall(_)))) => Some(
                retry()
                    .tooltip("Run the Uninstaller Again")
                    .on_click(cx.listener(move |this, _, window, cx| this.retry(tool, window, cx))),
            ),
            Some(State::Failed(..) | State::Skipped(_)) => Some(
                retry()
                    .on_click(cx.listener(move |this, _, window, cx| this.retry(tool, window, cx))),
            ),
            _ if newer.is_some() => Some(
                Button::new(("tool-upgrade", ix))
                    .primary()
                    .small()
                    .icon(Lucide::CircleArrowUp)
                    .label("Upgrade")
                    .tooltip(self.upgrade_line(tool))
                    .on_click(cx.listener(move |this, _, _, cx| this.upgrade(&[id], cx))),
            ),
            // Installed outside its installer: the note under the row says why nothing runs.
            _ if installed && !self.offers_reinstall(id) => None,
            _ if installed => Some(
                Button::new(("tool-reinstall", ix))
                    .outline()
                    .small()
                    .icon(Lucide::RotateCw)
                    .label("Reinstall")
                    .tooltip("Run the Installer Again (Updates to the Latest Version)")
                    .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            ),
            _ if tool.interactive => Some(
                Button::new(("tool-terminal", ix))
                    .primary()
                    .small()
                    .icon(Lucide::SquareTerminal)
                    .label("Install in Terminal")
                    .tooltip("Asks for Your Password, So It Runs in the Terminal")
                    .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            ),
            _ => Some(
                Button::new(("tool-install", ix))
                    .primary()
                    .small()
                    .icon(Lucide::Download)
                    .label("Install")
                    .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            ),
        };

        let can_uninstall = installed && !busy && self.uninstall_command(tool).is_some();
        let checkbox = if installed || busy || tool.interactive {
            div().size(px(16.)).flex_none().into_any_element()
        } else {
            let checked = self.selected.contains(id);
            Checkbox::new(("tool-pick", ix))
                .accessibility_label(format!("Select {}", tool.name))
                .checked(checked)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    if *checked {
                        this.selected.insert(id);
                    } else {
                        this.selected.remove(id);
                    }
                    cx.notify();
                }))
                .into_any_element()
        };

        // One chip per prerequisite: green once it's installed.
        let needs = tool.needs.iter().filter_map(|n| find(n)).map(|need| {
            let ok = self.installed(need.id);
            h_flex()
                .h(px(18.))
                .gap_1()
                .px_1p5()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(if ok {
                    cx.theme().success.opacity(0.4)
                } else {
                    cx.theme().border
                })
                .text_xs()
                .text_color(if ok { cx.theme().success } else { muted })
                .child(logo(need.id, 12., cx))
                .child(format!("needs {}", need.name))
                .when(ok, |el| el.child(Icon::new(Lucide::Check).xsmall()))
        });

        let log = self.logs.get(id);
        let running = matches!(state, Some(State::Installing | State::Uninstalling));
        let last_line = log.filter(|_| running).and_then(|l| l.last().cloned());
        let expanded = self.expanded.contains(id) && log.is_some();
        let upgrade_shown = newer.is_some() || upgrading;
        let command = if upgrade_shown {
            self.upgrade_line(tool)
        } else {
            install_command(tool, self.pm)
        };
        // A tool npm installs globally, not yet installed: the package manager to install it with.
        let pm_menu = (npm_package(tool).is_some() && found.is_none())
            .then(|| {
                crate::package_managers::menu(
                    ("tool-pm", ix),
                    &self.pms,
                    Use::Global,
                    self.pm,
                    cx.listener(|this, pm: &PackageManager, _, cx| {
                        this.pm = Some(*pm);
                        crate::package_managers::remember(*pm);
                        cx.notify();
                    }),
                )
            })
            .flatten();
        // Under the name: past the checkbox, the tile and their gaps.
        let indent = px(16. + catalog::TILE + 24.);

        catalog::list_item(v_flex(), ("tool-row", ix), tool.name)
            .px_4()
            .py_3()
            .gap_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .hover(|el| el.bg(cx.theme().list_hover))
            .when(running, |el| el.bg(cx.theme().list_hover))
            .when(selected, |el| el.bg(cx.theme().list_active))
            .child(catalog::columns(
                // Level with the middle of the tile.
                Some(
                    h_flex()
                        .h(px(catalog::TILE))
                        .child(checkbox)
                        .into_any_element(),
                ),
                catalog::tile(logo(id, 22., cx), cx),
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(div().text_sm().font_semibold().child(tool.name))
                            .child(catalog::tag(None, category_name(tool.category), cx))
                            .children(needs),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(tool.description),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .min_w_0()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .font_family(mono.clone())
                                    .text_color(muted)
                                    .child(match found {
                                        Some(f) => tilde(&f.path),
                                        None => command.clone(),
                                    }),
                            )
                            .child(
                                Button::new(("tool-copy", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::Copy)
                                    .accessibility_label(if upgrade_shown {
                                        "Copy Upgrade Command"
                                    } else {
                                        "Copy Install Command"
                                    })
                                    .tooltip(if upgrade_shown {
                                        "Copy Upgrade Command"
                                    } else {
                                        "Copy Install Command"
                                    })
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            command.clone(),
                                        ))
                                    }),
                            )
                            .children(pm_menu),
                    ),
                catalog::pill(mark, badge, color),
                h_flex()
                    .gap_1()
                    .when(log.is_some(), |el| {
                        el.child(
                            Button::new(("tool-log", ix))
                                .ghost()
                                .small()
                                .icon(Lucide::ScrollText)
                                .selected(expanded)
                                .accessibility_label("Output")
                                .toggled(expanded)
                                .tooltip(if expanded {
                                    "Hide Output"
                                } else {
                                    "Show Output"
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.expanded.remove(id) {
                                        this.expanded.insert(id);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .when(can_uninstall, |el| {
                        el.child(
                            Button::new(("tool-uninstall", ix))
                                .ghost()
                                .small()
                                .icon(Icon::new(Lucide::Trash).text_color(cx.theme().danger))
                                .accessibility_label(format!("Uninstall {}", tool.name))
                                .tooltip(format!("Uninstall {}", tool.name))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_uninstall(tool, window, cx)
                                })),
                        )
                    })
                    .children(action),
            ))
            .when_some(
                self.outside(id).filter(|_| state.is_none()),
                |el, outside| {
                    el.child(
                        h_flex()
                            .debug_selector(move || format!("tool-outside-{id}"))
                            .pl(indent)
                            .gap_1p5()
                            .items_start()
                            .text_xs()
                            .text_color(muted)
                            .child(div().pt(px(1.)).child(Icon::new(Lucide::Info).xsmall()))
                            .child(div().whitespace_normal().child(format!(
                                "{}, so Sawit doesn't upgrade, reinstall or remove this copy.",
                                outside.text()
                            ))),
                    )
                },
            )
            .when_some(last_line.filter(|_| !expanded), |el, line| {
                el.child(
                    div()
                        .pl(indent)
                        .text_xs()
                        .font_family(mono.clone())
                        .text_color(muted)
                        .truncate()
                        .child(line),
                )
            })
            .when_some(
                match &state {
                    Some(State::Failed(e, _)) if !expanded => Some(e.clone()),
                    _ => None,
                },
                |el, e| {
                    el.child(
                        h_flex()
                            .pl(indent)
                            .gap_1p5()
                            .items_start()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(
                                div()
                                    .pt(px(1.))
                                    .child(Icon::new(Lucide::CircleAlert).xsmall()),
                            )
                            .child(div().whitespace_normal().child(e)),
                    )
                },
            )
            .when(expanded, |el| {
                let lines = log
                    .map(|l| l[l.len().saturating_sub(14)..].join("\n"))
                    .unwrap_or_default();
                el.child(
                    div()
                        .ml(indent)
                        .p_2()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary)
                        .text_xs()
                        .font_family(mono)
                        .text_color(muted)
                        .whitespace_normal()
                        .child(lines),
                )
            })
            .into_any_element()
    }
}
