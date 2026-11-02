//! What the view hears: the page's and the engine's messages ([`FromPage`]), and the side panels'
//! asks.

use gpui_kit::*;
use sawit_core::browser::{ConsoleEntry, PageEvent, parse_ipc};
use serde_json::{Value, json};

use super::annotate::AnnotateEvent;
use super::design::DesignEvent;
use super::engine::EngineEvent;
use super::navigation::{NewWindow, new_window_action};
use super::notes::{intent_of, markers_op};
use super::picking::{pick_mode, pick_wanted, rearm_ops, theme_op};
use super::{BrowserEvent, BrowserView, FromPage, Panel, PickMode};

impl BrowserView {
    /// Takes a message from the page or the engine, and draws the view again when it may look
    /// different. The page's console and network entries and its hovers change nothing the view
    /// draws (the panels draw themselves again when they change), so a page that logs or fetches
    /// all the time, or a pointer moving over it, doesn't keep the window redrawing; unless
    /// WebKit's address moved since the toolbar was drawn ([`Self::address_moved`]).
    pub(super) fn receive(
        &mut self,
        message: FromPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let quiet = matches!(
            message,
            FromPage::Event(PageEvent::Console(_) | PageEvent::Network(_) | PageEvent::Hovered(_))
        );
        match message {
            FromPage::Event(PageEvent::Console(entry)) => {
                let error = entry.level == "error";
                self.logs.push_console(entry);
                if error {
                    self.sync_error_count(cx);
                }
            }
            FromPage::Event(PageEvent::Network(entry)) => self.logs.push_network(entry),
            FromPage::Event(PageEvent::Picked(pick)) => {
                // A page can arm the picker itself: only picks Sawit asked for count.
                if !pick_wanted(self.panel, self.pick, pick["mode"].as_str().unwrap_or("")) {
                    return;
                }
                self.last_pick = pick["element"]["selector"].as_str().map(str::to_string);
                if pick["mode"] == "annotate" {
                    // Written in the page's popover, or (`null`) to be written in the panel.
                    let note = pick["comment"]
                        .as_str()
                        .map(|c| (c.to_string(), intent_of(&pick["intent"])));
                    match serde_json::from_value(pick["element"].clone()) {
                        Ok(element) => self.on_picked(element, note, window, cx),
                        Err(e) => eprintln!("sawit: a picked element couldn't be read: {e}"),
                    }
                } else if let Some(selector) = &self.last_pick {
                    self.inspect_element(selector.clone(), true, cx);
                }
                cx.notify();
            }
            FromPage::Event(PageEvent::Hovered(hover)) => {
                if let Some(selector) = hover["selector"].as_str() {
                    self.inspect_element(selector.to_string(), false, cx);
                }
            }
            // From the page: Esc while picking. ⇧⌘. never comes from it (browser_overlay.js):
            // the user's own is the keymap's, and Sawit answers an agent's itself.
            FromPage::Event(PageEvent::Key(key)) => {
                if key == "escape" {
                    self.set_pick(PickMode::Off, cx);
                }
            }
            FromPage::Event(PageEvent::Annotation(change)) => {
                self.on_page_annotation(&change, window, cx);
            }
            // The user's press in the page gave it AppKit's keyboard: GPUI's follows. Never
            // while the tab is hidden: nobody can press in a hidden page, and the keyboard
            // would leave the terminal or field that has it for a view that isn't drawn, which
            // the workspace then takes it from (`workspace::KeyboardHome`).
            FromPage::Event(PageEvent::Pressed) => {
                if self.page_shown() {
                    self.focus.focus(window, cx);
                }
            }
            FromPage::Event(PageEvent::Url(url)) => {
                // The page says the address changed; WebKit says what it is.
                self.url = self
                    .engine
                    .as_ref()
                    .and_then(|e| e.url())
                    .filter(|u| !u.is_empty())
                    .unwrap_or(url);
                self.design_url_changed();
                self.scan_if_design_open(cx);
                self.show_address(window, cx);
                // Route changes without a load still move to another page.
                let url = self.current_url();
                self.sync_page_info(&url, cx);
                self.sync_markers(cx);
            }
            // The message the page sent says whether to draw again.
            FromPage::Engine(EngineEvent::Ipc(body)) => {
                if let Some(event) = parse_ipc(&body, &self.token) {
                    self.receive(FromPage::Event(event), window, cx);
                }
                return;
            }
            FromPage::Engine(EngineEvent::Load { started, url }) => {
                self.loading = started;
                if !started {
                    self.agent_page_loaded();
                }
                if started {
                    self.design_load_started();
                    self.sync_design(cx);
                }
                if !started {
                    self.scan_if_design_open(cx);
                    for op in self.ops_after_load(cx) {
                        if op["op"] == "pick" {
                            self.send_pick(op, cx);
                        } else {
                            drop(self.page_op(op));
                        }
                    }
                }
                if started && url != "about:blank" {
                    // Marks where each page's messages begin.
                    self.logs.push_console(ConsoleEntry {
                        level: "info".into(),
                        text: format!("— navigated to {url}"),
                        url: None,
                        line: None,
                        time_ms: 0.,
                    });
                }
                self.url = url;
                self.show_address(window, cx);
                let url = self.current_url();
                self.sync_page_info(&url, cx);
            }
            FromPage::Engine(EngineEvent::Title(title)) => {
                self.title = title;
                let url = self.current_url();
                self.sync_page_info(&url, cx);
            }
            FromPage::Engine(EngineEvent::NewWindow(url)) => {
                match new_window_action(&url, &self.current_url()) {
                    NewWindow::Ignore => {}
                    NewWindow::Load(url) => self.load(url, cx),
                    NewWindow::External(url) => self.open_external(url, window, cx),
                }
            }
            FromPage::Engine(EngineEvent::External(url)) => self.open_external(url, window, cx),
            FromPage::Shoot { rect, reply } => self.shoot(rect, reply, cx),
        }
        if !quiet || self.address_moved() {
            cx.notify();
        }
    }

    /// Whether WebKit's address is no longer the one the toolbar was last drawn with. A load that
    /// fails before its page commits (a port nobody serves, a download) takes it back to the page
    /// with no event, as wry tells only of commits and finishes: a quiet message then draws the
    /// view again, for one read of the address instead of a redraw each time.
    fn address_moved(&self) -> bool {
        (self.drawn_url.as_ref()).is_some_and(|drawn| *drawn != self.current_url())
    }

    /// Ops to run when a page has loaded: its agent starts fresh, so the overlay takes
    /// Sawit's colours again, picking is armed again and the markers drawn again.
    fn ops_after_load(&self, cx: &App) -> Vec<Value> {
        let mut ops = vec![theme_op(cx)];
        ops.extend(rearm_ops(self.pick));
        let markers = self.annotate.read(cx).markers_for(&self.current_url(), cx);
        if !markers.is_empty() {
            ops.push(markers_op(&self.token, &markers));
        }
        ops
    }

    pub(super) fn on_annotate(&mut self, event: &AnnotateEvent, cx: &mut Context<Self>) {
        match event {
            AnnotateEvent::Show(selector) => {
                drop(self.page_op(json!({ "op": "flash", "selector": selector })));
            }
            AnnotateEvent::Changed => self.sync_markers(cx),
            AnnotateEvent::Copy => {
                if let Some(md) = self.annotations_markdown(cx) {
                    cx.write_to_clipboard(ClipboardItem::new_string(md));
                    self.annotate.update(cx, |a, cx| {
                        a.mark_sent(cx);
                        a.set_note("Copied", cx);
                    });
                }
            }
            // The notes count as sent once the workspace says they went (`finish_send`).
            AnnotateEvent::SendTo(agent) => {
                if let Some(markdown) = self.annotations_markdown(cx) {
                    cx.emit(BrowserEvent::SendToAgent {
                        markdown,
                        agent: agent.clone(),
                    });
                }
            }
            AnnotateEvent::Open(kind) => {
                if let Some(markdown) = self.annotations_markdown(cx) {
                    cx.emit(BrowserEvent::OpenAgent {
                        markdown,
                        kind: *kind,
                    });
                }
            }
            AnnotateEvent::Pick => {
                if self.panel == Some(Panel::Annotate) {
                    self.set_pick(PickMode::Annotate, cx);
                }
            }
        }
    }

    pub(super) fn on_design(&mut self, event: &DesignEvent, cx: &mut Context<Self>) {
        match event {
            // Choosing Inspect again after Esc picks again.
            DesignEvent::Segment(segment) => {
                if self.panel == Some(Panel::Design) {
                    self.set_pick(pick_mode(Panel::Design, *segment), cx);
                }
            }
            DesignEvent::Flash(selector) => {
                drop(self.page_op(json!({ "op": "flash", "selector": selector })));
            }
            DesignEvent::CopyHtml(selector) => {
                // Not the latest-only path: a hover meanwhile neither drops nor is dropped.
                let info = self.style_info(json!({ "selector": selector }), true, cx);
                cx.spawn(async move |this, cx| {
                    let note = match info.await {
                        Ok(info) => match info.html {
                            Some(html) => {
                                cx.update(|cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(html));
                                });
                                "Copied".to_string()
                            }
                            None => "The page gave no markup.".to_string(),
                        },
                        Err(e) => e,
                    };
                    let _ = this.update(cx, |this, cx| this.design_note(note, cx));
                })
                .detach();
            }
            DesignEvent::Rescan => {
                self.design_load_started();
                self.scan_design(cx).detach();
            }
            DesignEvent::Thumbnails(assets) => self.fetch_thumbnails(assets.clone(), cx),
            DesignEvent::Pin(selector) => self.inspect_element(selector.clone(), true, cx),
            DesignEvent::SaveAssets(assets) => self.save_assets(assets.clone(), cx),
            DesignEvent::SaveAll { include_videos } => self.save_all_assets(*include_videos, cx),
            DesignEvent::Export { include_videos } => self.export_from_panel(*include_videos, cx),
            DesignEvent::Reveal(file) => {
                let file = file.clone();
                let job = crate::rt::spawn_blocking(cx, move || {
                    let status = std::process::Command::new("open")
                        .arg("-R")
                        .arg(&file)
                        .status()
                        .map_err(|e| e.to_string())?;
                    status
                        .success()
                        .then_some(())
                        .ok_or_else(|| format!("open exited with {status}"))
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Err(e)) = job.await {
                        let _ = this.update(cx, |this, cx| {
                            this.design_note(format!("Couldn't reveal in Finder: {e}"), cx)
                        });
                    }
                })
                .detach();
            }
        }
    }
}
