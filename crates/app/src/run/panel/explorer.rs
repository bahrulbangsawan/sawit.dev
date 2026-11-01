//! The Run panel's Local Explorer section, for a project with Cloudflare Workers: one row per
//! Worker that opens its dev server's Local Explorer while one runs, starts the script that serves
//! it while none does, and says which Wrangler or Vite plugin version it needs when the installed
//! one is too old, with the upgrade command to copy or run (never run on its own).

use sawit_core::cloudflare::{self, DevScript, DevTool, Support, WorkersProject};

use super::*;

/// A Worker of the project, as the section shows it; read with the project's scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Worker {
    pub project: WorkersProject,
    pub support: Support,
    /// A short line on what its dev tool still needs ([`note`]).
    pub note: Option<String>,
    /// The command that upgrades its dev tool, with the project's package manager.
    pub upgrade: String,
    /// The script that starts its dev server.
    pub script: Option<DevScript>,
}

/// Every Worker of the project at `root`.
pub(super) fn read(root: &Path) -> Vec<Worker> {
    cloudflare::detect_project(root)
        .into_iter()
        .map(|project| Worker {
            support: cloudflare::support(&project),
            note: note(&project),
            upgrade: cloudflare::upgrade_command(&project, root),
            script: cloudflare::dev_scripts(&project).into_iter().next(),
            project,
        })
        .collect()
}

/// What the row says under its status, short for the sidebar: that invoking crons needs a newer
/// tool when the config has crons, or that the tool isn't installed yet. A version too old for
/// Local Explorer at all is the status itself, with its upgrade command.
fn note(project: &WorkersProject) -> Option<String> {
    let tool = project.tool;
    match cloudflare::support(project) {
        Support::Ready { cron: false } if cloudflare::declares_crons(project) => {
            tool.cron_since().map(|v| {
                format!(
                    "Triggering crons from it needs {} {v} or later.",
                    tool.name()
                )
            })
        }
        Support::Unknown if project.version.is_none() && tool != DevTool::CfCli => {
            tool.explorer_since().map(|v| {
                format!(
                    "Not installed yet: Local Explorer needs {} {v} or later.",
                    tool.name()
                )
            })
        }
        _ => None,
    }
}

/// What a Worker's dev server is started with when no script of its own does.
fn fallback_command(tool: DevTool) -> &'static str {
    match tool {
        DevTool::Wrangler => "npx wrangler dev",
        DevTool::VitePlugin => "npx vite",
        DevTool::CfCli => "cf dev",
    }
}

impl RunPanel {
    /// The Run panel row of `w`'s dev script, as the scripts list has it (so its state is the
    /// row's), else a terminal of its own with [`fallback_command`].
    fn dev_item(&self, w: &Worker) -> Item {
        let script = w.script.as_ref().and_then(|s| {
            self.groups
                .iter()
                .filter(|g| g.dir == w.project.root && g.source == "package.json")
                .flat_map(Self::group_items)
                .find(|item| item.name == s.name)
        });
        script.unwrap_or_else(|| {
            let command = fallback_command(w.project.tool).to_string();
            Item {
                name: command.clone(),
                detail: command.clone(),
                launches: vec![Launch {
                    tag: JobTag {
                        key: format!("explorer-dev\u{0}{}", w.project.root.display()),
                        name: format!("{} dev", w.project.tool.package()),
                    },
                    dir: w.project.root.clone(),
                    command,
                }],
                saved: None,
            }
        })
    }

    /// The section's rows, under its heading; nothing for a project with no Worker.
    pub(super) fn render_explorer(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.workers.is_empty() {
            return Vec::new();
        }
        let mut rows = vec![
            self.render_section(
                "run-section-explorer",
                "LOCAL EXPLORER".into(),
                None,
                "Cloudflare's browser for your Worker's local KV, R2, D1, Durable Objects and Workflows, served by its dev server".into(),
                None,
                cx,
            )
            .into_any_element(),
        ];
        for (ix, w) in self.workers.clone().into_iter().enumerate() {
            rows.push(self.render_worker(ix, w, cx));
        }
        rows
    }

    fn render_worker(&self, ix: usize, w: Worker, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let running = self
            .ports
            .read(cx)
            .explorers()
            .iter()
            .filter(|e| e.runs_in(&w.project.root))
            .min_by_key(|e| e.port)
            .cloned();
        let item = self.dev_item(&w);
        let starting = {
            let terminal = self.terminal.read(cx);
            item.keys().any(|k| {
                terminal
                    .job_status(k, cx)
                    .is_some_and(|s| s.running.is_some())
            })
        };
        let rel = w
            .project
            .root
            .strip_prefix(self.root.as_deref().unwrap_or(Path::new("")))
            .ok()
            .filter(|r| !r.as_os_str().is_empty())
            .map(|r| format!("{}/", r.display()))
            .unwrap_or_default();
        let name = w
            .project
            .name
            .clone()
            .unwrap_or_else(|| format!("{rel}{}", w.project.config_name()));
        let kinds = w
            .project
            .explorer_kinds()
            .iter()
            .map(|k| k.label().to_string())
            .collect::<Vec<_>>()
            .join(" · ");
        let tool = match &w.project.version {
            Some(v) => format!("{} {v}", w.project.tool.name()),
            None => w.project.tool.name().to_string(),
        };
        // The bindings first: a narrow sidebar cuts the end. The config's path is the name's
        // tooltip.
        let detail = [kinds, tool]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let config: SharedString = format!("{rel}{}", w.project.config_name()).into();
        let too_old = match w.support {
            Support::TooOld { installed, needed } => Some((installed, needed)),
            _ => None,
        };
        let status: SharedString = match (&running, too_old, starting) {
            (Some(e), ..) => format!("Running on :{}", e.port).into(),
            (None, Some((installed, needed)), _) => format!(
                "Needs {} {needed} or later; {installed} is installed.",
                w.project.tool.name()
            )
            .into(),
            (None, None, true) => "Starting… it opens once the dev server answers.".into(),
            (None, None, false) => "Start the dev server to open Local Explorer.".into(),
        };
        let action: Option<AnyElement> = match (&running, too_old, starting) {
            (Some(e), ..) => {
                let url = e.url();
                Some(
                    Button::new(("run-explorer-open", ix))
                        .xsmall()
                        .primary()
                        .icon(Lucide::ExternalLink)
                        .label("Open")
                        .tooltip(format!("Open {url} in Sawit's browser (⌃⌘E)"))
                        .on_click(
                            cx.listener(move |_, _, _, cx| cx.emit(RunEvent::Browse(url.clone()))),
                        )
                        .into_any_element(),
                )
            }
            (None, None, false) => {
                let tip = format!("Run {} in a terminal", item.detail);
                let item = std::rc::Rc::new(item);
                Some(
                    Button::new(("run-explorer-start", ix))
                        .xsmall()
                        .icon(Lucide::Play)
                        .label("Run")
                        .tooltip(tip)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.start(&item, Origin::User, cx)),
                        )
                        .into_any_element(),
                )
            }
            _ => None,
        };
        let upgrade = too_old.map(|_| {
            let command = w.upgrade.clone();
            let copy = command.clone();
            let launch = Launch {
                tag: JobTag {
                    key: format!("explorer-upgrade\u{0}{}", w.project.root.display()),
                    name: format!("upgrade {}", w.project.tool.package()),
                },
                dir: w.project.root.clone(),
                command: command.clone(),
            };
            h_flex()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px_1()
                        .rounded(px(3.))
                        .bg(cx.theme().secondary)
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .truncate()
                        .child(command),
                )
                .child(
                    Button::new(("run-explorer-copy", ix))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Copy)
                        .tooltip("Copy the Upgrade Command")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        }),
                )
                .child(
                    Button::new(("run-explorer-upgrade", ix))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Play)
                        .tooltip("Run the Upgrade Command in a Terminal")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(RunEvent::Start(vec![launch.clone()], Origin::User))
                        })),
                )
        });
        let note = w.note.clone();
        h_flex()
            .id(("run-explorer", ix))
            .w_full()
            .px_2()
            .py_1()
            .gap_1p5()
            .items_start()
            .text_sm()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .child(div().mt(px(2.)).child(crate::local_explorer::logo(14.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .id(("run-explorer-name", ix))
                            .font_semibold()
                            .truncate()
                            .child(name)
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(config.clone())
                                    .build(window, cx)
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .when(too_old.is_some(), |el| el.text_color(cx.theme().warning))
                            .when(too_old.is_none(), |el| el.text_color(muted))
                            .child(status),
                    )
                    .children(upgrade)
                    .when_some(note, |el, note| {
                        el.child(div().text_xs().text_color(muted).child(note))
                    })
                    .child(div().text_xs().text_color(muted).truncate().child(detail)),
            )
            .children(action)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use sawit_core::cloudflare::{DevTool, Support};

    use super::{fallback_command, read};

    /// The section reads each Worker with what it needs: an old Wrangler is told with its
    /// upgrade command, and the dev script is the one that starts `wrangler dev`.
    #[test]
    fn workers_are_read_with_what_they_need() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"build":"tsc","dev":"wrangler dev"},"devDependencies":{"wrangler":"^4"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("wrangler.toml"), "name = \"api\"\n").unwrap();
        std::fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::create_dir_all(root.join("node_modules/wrangler")).unwrap();
        std::fs::write(
            root.join("node_modules/wrangler/package.json"),
            r#"{"version":"4.90.0"}"#,
        )
        .unwrap();
        let workers = read(root);
        assert_eq!(workers.len(), 1);
        let w = &workers[0];
        assert!(matches!(w.support, Support::TooOld { .. }));
        assert_eq!(w.upgrade, "pnpm add -D wrangler@latest");
        assert_eq!(w.script.as_ref().map(|s| s.name.as_str()), Some("dev"));
        // Too old is the status itself, with its command: no note besides.
        assert_eq!(w.note, None);
        assert_eq!(fallback_command(DevTool::VitePlugin), "npx vite");

        // New enough for the explorer, not for crons the config has.
        std::fs::write(
            root.join("node_modules/wrangler/package.json"),
            r#"{"version":"4.120.0"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("wrangler.toml"),
            "name = \"api\"\n[triggers]\ncrons = [\"* * * * *\"]\n",
        )
        .unwrap();
        let w = &read(root)[0];
        assert_eq!(w.support, Support::Ready { cron: false });
        assert_eq!(
            w.note.as_deref(),
            Some("Triggering crons from it needs Wrangler 4.139.0 or later.")
        );

        // Not installed yet.
        std::fs::remove_dir_all(root.join("node_modules")).unwrap();
        let w = &read(root)[0];
        assert!(w.note.as_deref().unwrap().starts_with("Not installed yet"));
    }
}
