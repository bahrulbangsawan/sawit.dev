//! The status bar: instances and the branch on the left; the project's folder, the CLI and the
//! version on the right.

use super::*;

impl Workspace {
    /// Puts the project path on the clipboard; the button shows a check
    /// for a moment as confirmation.
    pub(super) fn copy_project_path(&mut self, path: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(path));
        self.path_copied = true;
        cx.notify();
        self._path_copied_reset = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.path_copied = false;
                cx.notify();
            });
        });
    }

    /// The status bar: instances and the branch on the left; the project's
    /// folder, the CLI and the version on the right.
    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let store = self.servers.read(cx);
        let running = store.running_count();
        let total = store.registry.instances.len();
        let project = self.root(cx).map(|r| r.display().to_string());
        let copied = self.path_copied;
        let git = self.current().git.read(cx);
        let branch = git.status.as_ref().map(|s| {
            let name = s
                .branch
                .clone()
                .or_else(|| s.head.clone())
                .unwrap_or_else(|| "main".into());
            let mut sync = String::new();
            if s.behind > 0 {
                sync.push_str(&format!(" ↓{}", s.behind));
            }
            if s.ahead > 0 {
                sync.push_str(&format!(" ↑{}", s.ahead));
            }
            (name, sync, s.files.len())
        });
        let muted = cx.theme().muted_foreground;
        let instances = match (running, total) {
            (_, 0) => "No instances".to_string(),
            (0, n) => format!("{n} stopped"),
            (r, n) => format!("{r} of {n} running"),
        };
        h_flex()
            .h(px(26.))
            .px_1p5()
            .gap_1()
            .text_xs()
            .text_color(muted)
            .bg(cx.theme().status_bar)
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .child(
                Button::new("status-instances")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(Lucide::Database).text_color(if running > 0 {
                        cx.theme().success
                    } else {
                        muted
                    }))
                    .label(instances)
                    .tooltip_with_action("Show Instances", &ShowInstances, Some("Workspace"))
                    .on_click(
                        cx.listener(|this, _, _, cx| this.show_panel(SidebarTab::Servers, cx)),
                    ),
            )
            .when_some(branch, |el, (name, sync, changed)| {
                el.child(crate::catalog::divider(cx)).child(
                    Button::new("status-git-branch")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::GitBranch)
                        .label(format!(
                            "{name}{sync}{}",
                            if changed > 0 {
                                format!(" · {changed} changed")
                            } else {
                                String::new()
                            }
                        ))
                        .tooltip_with_action("Show Git", &ShowGit, Some("Workspace"))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.show_panel(SidebarTab::Git, cx)),
                        ),
                )
            })
            .child(div().flex_1())
            .when_some(project, |el, project| {
                // The full path lives in the tooltip; the bar only offers the copy.
                el.child(
                    Button::new("copy-project-path")
                        .ghost()
                        .xsmall()
                        .icon(if copied { Lucide::Check } else { Lucide::Copy })
                        .label(if copied { "Copied" } else { "Copy Path" })
                        .tooltip_with_action(project.clone(), &CopyProjectPath, Some("Workspace"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.copy_project_path(project.clone(), cx)
                        })),
                )
                .child(crate::catalog::divider(cx))
            })
            .when(!self.cli_status.read(cx).installed, |el| {
                el.child(
                    Button::new("install-cli")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::SquareTerminal)
                        .label("Install CLI")
                        .tooltip_with_action(
                            "Install the ‘sawit’ Command to Open Folders from a Terminal",
                            &crate::cli::InstallCli,
                            Some("Workspace"),
                        )
                        .on_click(|_, _, cx| crate::cli::install_cli(cx)),
                )
            })
            .child(
                Button::new("open-changelog")
                    .ghost()
                    .xsmall()
                    .label(format!("v{}", Version::current()))
                    .tooltip_with_action("Version & Changelog", &OpenChangelog, Some("Workspace"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_changelog_page(cx))),
            )
    }
}
