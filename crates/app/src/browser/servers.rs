//! The local servers beside the address: a button whose popover lists the dev servers listening
//! right now (port, what runs them, their project), scanned each time it opens and never while it
//! is closed. Choosing one opens `http://localhost:<port>/` in this tab (⌘-click too, until the
//! browser has tabs of its own).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, v_flex};
use gpui_kit::*;
use sawit_core::ports::{LocalServer, local_servers};

use super::BrowserView;
use crate::rt;

/// The list as the popover shows it: scanning, the servers found, or why the scan failed.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Scan {
    Scanning,
    Found(Vec<LocalServer>),
    Failed(String),
}

impl BrowserView {
    /// Opens the list and scans the ports for it; closes it.
    pub(super) fn set_servers_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if !open {
            self.servers = None;
            cx.notify();
            return;
        }
        self.servers = Some(Scan::Scanning);
        cx.notify();
        let rx = rt::spawn(cx, sawit_core::probe::listening_jobs());
        cx.spawn(async move |this, cx| {
            let scan = match rx.await {
                Ok(Ok(jobs)) => Scan::Found(local_servers(&jobs)),
                Ok(Err(e)) => Scan::Failed(e.to_string()),
                Err(_) => Scan::Failed("The ports couldn't be checked.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                // Closed meanwhile: nothing to show.
                if this.servers.is_some() {
                    this.servers = Some(scan);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Opens a server from the list, and closes it.
    fn open_server(&mut self, server: &LocalServer, cx: &mut Context<Self>) {
        self.servers = None;
        self.navigate(&server.url(), cx);
    }

    /// The button beside the address, and its list while open.
    pub(super) fn servers_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scan = self.servers.clone();
        let open = scan.is_some();
        let weak = cx.entity().downgrade();
        let rows_view = weak.clone();
        let button = Button::new("browser-servers")
            .debug_selector(|| "browser-servers".into())
            .ghost()
            .xsmall()
            .icon(Lucide::Radio)
            .accessibility_label("Local Servers")
            .tooltip("Local Servers");
        Popover::new("browser-servers-popover")
            .anchor(Anchor::TopLeft)
            .trigger(button)
            .open(open)
            .on_open_change(move |open, _, cx| {
                let _ = weak.update(cx, |this, cx| this.set_servers_open(*open, cx));
            })
            .content(move |_, _, cx| servers_list(scan.as_ref(), &rows_view, cx))
    }
}

/// The popover's content: a heading, then one row per server, or what the scan says.
fn servers_list(scan: Option<&Scan>, view: &WeakEntity<BrowserView>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let note = |text: String| div().px_2().py_1().text_sm().text_color(muted).child(text);
    let body: Vec<AnyElement> = match scan {
        None | Some(Scan::Scanning) => vec![note("Looking for servers…".into()).into_any_element()],
        Some(Scan::Failed(e)) => vec![note(e.clone()).into_any_element()],
        Some(Scan::Found(found)) if found.is_empty() => {
            vec![note("No dev servers running".into()).into_any_element()]
        }
        Some(Scan::Found(found)) => found
            .iter()
            .enumerate()
            .map(|(i, server)| {
                let (detail, view, server) = (server_detail(server), view.clone(), server.clone());
                let host = format!("localhost:{}", server.port);
                Button::new(("browser-server", i))
                    .debug_selector(move || format!("browser-server-{i}"))
                    .ghost()
                    .w_full()
                    .h_auto()
                    .py_1()
                    .justify_start()
                    .accessibility_label(format!("{host} · {detail}"))
                    .tooltip(format!("Open http://{host}/ in this tab"))
                    .child(
                        v_flex()
                            .min_w_0()
                            .items_start()
                            .child(div().text_sm().child(host))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .max_w(px(256.))
                                    .child(detail),
                            ),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |this, cx| this.open_server(&server, cx));
                    })
                    .into_any_element()
            })
            .collect(),
    };
    v_flex()
        .id("browser-servers-list")
        .debug_selector(|| "browser-servers-list".into())
        .w(px(280.))
        .max_w(px(360.))
        .gap(px(2.))
        .child(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .text_color(muted)
                .child("Local Servers"),
        )
        .children(body)
}

/// What runs a server, and its project: `bun run dev · shop`.
pub(super) fn server_detail(server: &LocalServer) -> String {
    match &server.project {
        Some(project) => format!("{} · {project}", server.name),
        None => server.name.clone(),
    }
}
