//! MCP page: installs the bundled `sawit-mcp` into every AI client and
//! profile found on this Mac (Claude Code profiles, Codex, Claude Desktop,
//! Cursor, Zed, …) and shows where each config lives.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_clients::{
    Category, Client, Status, Target, discover, install, snippet, status, uninstall,
};
use sawit_core::probe::find_in_path;

use crate::rt;

struct Row {
    target: Target,
    status: Status,
}

pub struct McpPage {
    /// `sawit-mcp` next to the running app (inside the .app bundle, or the
    /// cargo target folder during development).
    server: PathBuf,
    server_exists: bool,
    rows: Vec<Row>,
    clis: HashMap<&'static str, PathBuf>,
    busy: HashSet<usize>,
    errors: HashMap<usize, String>,
    loading: bool,
}

/// The client's logo: colour logos as images, one-colour ones tinted with
/// the theme, and a neutral glyph for clients without a free logo.
fn logo(client: Client, cx: &App) -> AnyElement {
    let (name, colour) = match client {
        Client::ClaudeCode => ("claudecode-color", true),
        Client::ClaudeDesktop => ("claude-color", true),
        Client::Codex => ("codex-color", true),
        Client::Gemini => ("geminicli-color", true),
        Client::Qwen => ("qwen-color", true),
        Client::Amp => ("amp-color", true),
        Client::Kiro => ("kiro-color", true),
        Client::Junie => ("junie-color", true),
        Client::Antigravity => ("antigravity-color", true),
        Client::Trae => ("trae-color", true),
        Client::OpenCode => ("opencode", false),
        Client::CopilotCli => ("githubcopilot", false),
        Client::Cline => ("cline", false),
        Client::Goose => ("goose", false),
        Client::Cursor => ("cursor", false),
        Client::Windsurf => ("windsurf", false),
        Client::Zed => ("zed", false),
        Client::Warp => ("warp", false),
        Client::RooCode => ("roocode", false),
        Client::KiloCode => ("kilocode", false),
        Client::LmStudio => ("lmstudio", false),
        _ => {
            // Same 18 px box as the logos, so names line up.
            return div()
                .size(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(Lucide::Bot)
                        .small()
                        .text_color(cx.theme().muted_foreground),
                )
                .into_any_element();
        }
    };
    let path: SharedString = format!("agent_logos/{name}.svg").into();
    if colour {
        img(path).size(px(18.)).flex_none().into_any_element()
    } else {
        svg()
            .path(path)
            .size(px(18.))
            .flex_none()
            .text_color(cx.theme().foreground)
            .into_any_element()
    }
}

/// `~/…` instead of the full home path.
fn tilde(path: &std::path::Path) -> String {
    let s = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if s.starts_with(&home) => s.replacen(&home, "~", 1),
        _ => s,
    }
}

impl McpPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let server = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("sawit-mcp")))
            .unwrap_or_else(|| PathBuf::from("sawit-mcp"));
        let mut this = Self {
            server_exists: false,
            server,
            rows: Vec::new(),
            clis: HashMap::new(),
            busy: HashSet::new(),
            errors: HashMap::new(),
            loading: true,
        };
        this.reload(cx);
        this
    }

    /// Re-detects clients and re-reads every config (off the UI thread).
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let server = self.server.clone();
        let rx = rt::spawn(cx, async move {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
            let rows: Vec<(Target, Status)> = discover(&home, claude_dir)
                .into_iter()
                .map(|t| {
                    let s = status(&t, &server);
                    (t, s)
                })
                .collect();
            let clis: HashMap<&'static str, PathBuf> = ["claude", "codex"]
                .into_iter()
                .filter_map(|n| find_in_path(n).map(|p| (n, p)))
                .collect();
            (rows, clis, server.is_file())
        });
        cx.spawn(async move |this, cx| {
            if let Ok((rows, clis, exists)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.rows = rows
                        .into_iter()
                        .map(|(target, status)| Row { target, status })
                        .collect();
                    this.clis = clis;
                    this.server_exists = exists;
                    this.loading = false;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn cli_for(&self, client: Client) -> Option<PathBuf> {
        client.cli().and_then(|n| self.clis.get(n).cloned())
    }

    fn act(&mut self, ix: usize, remove: bool, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(ix) else { return };
        if !self.busy.insert(ix) {
            return;
        }
        self.errors.remove(&ix);
        let target = row.target.clone();
        let server = self.server.clone();
        let cli = self.cli_for(target.client);
        let rx = rt::spawn(cx, async move {
            let result = if remove {
                uninstall(&target, cli.as_deref()).await
            } else {
                install(&target, &server, cli.as_deref()).await
            };
            (result, status(&target, &server))
        });
        cx.spawn(async move |this, cx| {
            if let Ok((result, new_status)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.busy.remove(&ix);
                    if let Err(e) = result {
                        this.errors.insert(ix, e.to_string());
                    }
                    if let Some(row) = this.rows.get_mut(ix) {
                        row.status = new_status;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn install_all(&mut self, cx: &mut Context<Self>) {
        let todo: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r.status, Status::NotInstalled | Status::Outdated(_)))
            .map(|(ix, _)| ix)
            .collect();
        for ix in todo {
            self.act(ix, false, cx);
        }
    }

    fn render_group(&self, client: Client, ixs: Vec<usize>, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let profiles = ixs.len();
        v_flex()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .bg(cx.theme().secondary)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(logo(client, cx))
                    .child(div().text_sm().font_semibold().child(client.name()))
                    .when(client.profile_var().is_some() && profiles > 1, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{profiles} profiles")),
                        )
                    }),
            )
            .children(ixs.into_iter().map(|ix| {
                let row = &self.rows[ix];
                self.render_row(ix, row, cx).into_any_element()
            }))
    }

    fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let busy = self.busy.contains(&ix);
        let disabled = !self.server_exists || busy;
        let (badge, color): (String, Hsla) = match &row.status {
            Status::Installed => ("Installed".into(), cx.theme().success),
            Status::Outdated(_) => ("Other version".into(), cx.theme().warning),
            Status::NotInstalled => ("Not installed".into(), muted),
            Status::Unreadable(_) => ("Edit manually".into(), cx.theme().danger),
            Status::Manual => ("Manual".into(), muted),
        };
        let detail = match &row.status {
            Status::Outdated(other) => Some(format!("points to {other}")),
            Status::Unreadable(why) => Some(why.clone()),
            _ => None,
        };
        let via = match self.cli_for(row.target.client) {
            Some(_) => format!("via CLI `{}`", row.target.client.cli().unwrap_or_default()),
            None if row.target.client.editable() => "edit file".to_string(),
            None => "copy & paste".to_string(),
        };
        let copy_snippet = |ix: usize, text: String| {
            Button::new(("mcp-copy", ix))
                .ghost()
                .xsmall()
                .icon(Lucide::Copy)
                .label("Copy Snippet")
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                })
        };
        let action = match &row.status {
            Status::Installed if !row.target.client.editable() => {
                copy_snippet(ix, snippet(&row.target, &self.server))
            }
            Status::Manual => copy_snippet(ix, snippet(&row.target, &self.server)),
            Status::Installed => Button::new(("mcp-remove", ix))
                .ghost()
                .xsmall()
                .label("Remove")
                .on_click(cx.listener(move |this, _, _, cx| this.act(ix, true, cx))),
            Status::Outdated(_) => Button::new(("mcp-update", ix))
                .primary()
                .xsmall()
                .label("Update")
                .on_click(cx.listener(move |this, _, _, cx| this.act(ix, false, cx))),
            Status::NotInstalled => Button::new(("mcp-install", ix))
                .primary()
                .xsmall()
                .label("Install")
                .on_click(cx.listener(move |this, _, _, cx| this.act(ix, false, cx))),
            Status::Unreadable(_) => copy_snippet(ix, snippet(&row.target, &self.server)),
        };
        v_flex()
            .px_3()
            .py_2()
            .gap_0p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(110.))
                            .flex_none()
                            .text_sm()
                            .font_semibold()
                            .child(row.target.profile.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(tilde(&row.target.config)),
                    )
                    .child(div().text_xs().text_color(muted).child(via))
                    .child(
                        h_flex()
                            .gap_1()
                            .w(px(92.))
                            .flex_none()
                            .text_xs()
                            .text_color(color)
                            .child(div().size(px(6.)).rounded_full().bg(color))
                            .child(badge),
                    )
                    .child(div().w(px(116.)).flex_none().flex().justify_end().child(
                        action.loading(busy).disabled(
                            disabled
                                && !matches!(row.status, Status::Manual | Status::Unreadable(_)),
                        ),
                    )),
            )
            .when_some(detail, |el, d| {
                el.child(
                    div()
                        .pl(px(118.))
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .child(d),
                )
            })
            .when_some(self.errors.get(&ix).cloned(), |el, e| {
                el.child(
                    div()
                        .pl(px(118.))
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
    }
}

impl Render for McpPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        // Rows grouped under their client, in discovery order.
        let mut groups: Vec<(Client, Vec<usize>)> = Vec::new();
        for (ix, row) in self.rows.iter().enumerate() {
            match groups.iter_mut().find(|(c, _)| *c == row.target.client) {
                Some((_, ixs)) => ixs.push(ix),
                None => groups.push((row.target.client, vec![ix])),
            }
        }
        // Agents first, then editors; each in the table's order.
        groups.sort_by_key(|(c, _)| {
            (
                c.category() == Category::Editor,
                Client::ALL.iter().position(|x| x == c),
            )
        });
        let pending = self
            .rows
            .iter()
            .filter(|r| matches!(r.status, Status::NotInstalled | Status::Outdated(_)))
            .count();
        let header = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_lg().font_semibold().flex_1().child("MCP server"))
                    .child(
                        Button::new("mcp-reload")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Detect Again")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .child(
                        Button::new("mcp-install-all")
                            .primary()
                            .small()
                            .label(format!("Install All ({pending})"))
                            .disabled(pending == 0 || !self.server_exists)
                            .on_click(cx.listener(|this, _, _, cx| this.install_all(cx))),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .whitespace_normal()
                    .child(
                        "AI agents can see instances, run read-only queries and write DATABASE_URL. \
                         After installing, restart the client (or the Claude/Codex session) so it picks up the server.",
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(if self.server_exists {
                        muted
                    } else {
                        cx.theme().danger
                    })
                    .child(tilde(&self.server))
                    .when(!self.server_exists && !self.loading, |el| {
                        el.child("— not built yet: cargo build --release -p sawit-mcp")
                    }),
            );
        let body = if self.loading {
            div()
                .text_sm()
                .text_color(muted)
                .child("Detecting clients…")
                .into_any_element()
        } else if groups.is_empty() {
            div()
                .text_sm()
                .text_color(muted)
                .child(
                    "No MCP clients detected (Claude Code, Codex, Claude Desktop, Cursor, Zed, …).",
                )
                .into_any_element()
        } else {
            let section = |title: &'static str| {
                div()
                    .pt_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child(title)
            };
            let mut seen_editor = false;
            let mut children: Vec<AnyElement> = vec![section("AGENT & CLI").into_any_element()];
            for (client, ixs) in groups {
                if client.category() == Category::Editor && !seen_editor {
                    seen_editor = true;
                    children.push(section("IDE & EDITOR").into_any_element());
                }
                children.push(self.render_group(client, ixs, cx).into_any_element());
            }
            v_flex().gap_3().children(children).into_any_element()
        };
        div()
            .id("mcp-page")
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .p_6()
                    .gap_5()
                    .max_w(px(980.))
                    .child(header)
                    .child(body),
            )
    }
}
