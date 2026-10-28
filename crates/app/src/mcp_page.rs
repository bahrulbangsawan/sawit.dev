//! MCP page: installs the bundled `pgdesk-mcp` into every AI client and
//! profile found on this Mac (Claude Code profiles, Codex, Claude Desktop,
//! Cursor, Zed, …) and shows where each config lives.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::mcp_clients::{
    Client, Status, Target, discover, install, snippet, status, uninstall,
};
use pgdesk_core::probe::find_in_path;

use crate::rt;

struct Row {
    target: Target,
    status: Status,
}

pub struct McpPage {
    /// `pgdesk-mcp` next to the running app (inside the .app bundle, or the
    /// cargo target folder during development).
    server: PathBuf,
    server_exists: bool,
    rows: Vec<Row>,
    clis: HashMap<&'static str, PathBuf>,
    busy: HashSet<usize>,
    errors: HashMap<usize, String>,
    loading: bool,
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
            .and_then(|exe| exe.parent().map(|d| d.join("pgdesk-mcp")))
            .unwrap_or_else(|| PathBuf::from("pgdesk-mcp"));
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

    fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let busy = self.busy.contains(&ix);
        let disabled = !self.server_exists || busy;
        let (badge, color): (String, Hsla) = match &row.status {
            Status::Installed => ("Terpasang".into(), cx.theme().success),
            Status::Outdated(_) => ("Versi lain".into(), cx.theme().warning),
            Status::NotInstalled => ("Belum".into(), muted),
            Status::Unreadable(_) => ("Edit manual".into(), cx.theme().danger),
        };
        let detail = match &row.status {
            Status::Outdated(other) => Some(format!("menunjuk ke {other}")),
            Status::Unreadable(why) => Some(why.clone()),
            _ => None,
        };
        let via = match self.cli_for(row.target.client) {
            Some(_) => format!(
                "lewat CLI `{}`",
                row.target.client.cli().unwrap_or_default()
            ),
            None => "edit file".to_string(),
        };
        let action = match &row.status {
            Status::Installed => Button::new(("mcp-remove", ix))
                .ghost()
                .xsmall()
                .label("Hapus")
                .on_click(cx.listener(move |this, _, _, cx| this.act(ix, true, cx))),
            Status::Outdated(_) => Button::new(("mcp-update", ix))
                .primary()
                .xsmall()
                .label("Perbarui")
                .on_click(cx.listener(move |this, _, _, cx| this.act(ix, false, cx))),
            Status::NotInstalled => Button::new(("mcp-install", ix))
                .primary()
                .xsmall()
                .label("Pasang")
                .on_click(cx.listener(move |this, _, _, cx| this.act(ix, false, cx))),
            Status::Unreadable(_) => {
                let text = snippet(&row.target, &self.server);
                Button::new(("mcp-copy", ix))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Copy)
                    .label("Salin snippet")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                    })
            }
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
                    .child(
                        div()
                            .w(px(116.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .child(action.loading(busy).disabled(disabled)),
                    ),
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
                            .tooltip("Deteksi ulang")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .child(
                        Button::new("mcp-install-all")
                            .primary()
                            .small()
                            .label(format!("Pasang ke semua ({pending})"))
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
                        "Agent AI bisa melihat instance, menjalankan query read-only dan menulis DATABASE_URL. \
                         Setelah dipasang, mulai ulang klien (atau sesi Claude/Codex) agar server terbaca.",
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
                        el.child("— belum dibangun: cargo build --release -p pgdesk-mcp")
                    }),
            );
        let body = if self.loading {
            div()
                .text_sm()
                .text_color(muted)
                .child("Mendeteksi klien…")
                .into_any_element()
        } else if groups.is_empty() {
            div()
                .text_sm()
                .text_color(muted)
                .child("Tidak ada klien MCP yang terdeteksi (Claude Code, Codex, Claude Desktop, Cursor, Zed, …).")
                .into_any_element()
        } else {
            v_flex()
                .gap_4()
                .children(groups.into_iter().map(|(client, ixs)| {
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
                                .child(div().text_sm().font_semibold().child(client.name()))
                                .when(client.profile_var().is_some() && ixs.len() > 1, |el| {
                                    el.child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(format!("{} profil", ixs.len())),
                                    )
                                }),
                        )
                        .children(ixs.into_iter().map(|ix| {
                            let row = &self.rows[ix];
                            self.render_row(ix, row, cx).into_any_element()
                        }))
                }))
                .into_any_element()
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
