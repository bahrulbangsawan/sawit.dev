//! Annotate, from the browser's side: a pick becomes a note with a screenshot of its element,
//! the panel and the page's markers follow each other, and the notes go out as Markdown.

use std::path::PathBuf;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::annotations::{
    Annotation, ElementDetails, Intent, PageGroup, PageInfo, next_number, page_key,
    relative_source, screenshot_dir, screenshot_name,
};
use sawit_core::browser::ViewportMode;
use serde_json::{Value, json};

use super::BrowserView;
use super::agents::AgentSource;
use super::annotate::{Marker, group_order, padded_rect};
use super::shots::within;

/// How long a pick waits for its screenshot (its turn included) before adding its row
/// without one.
const PICK_SHOT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

impl BrowserView {
    /// Where Annotate finds the agents running in the terminals, to send its notes to.
    pub fn set_agent_source(&mut self, source: AgentSource, cx: &mut Context<Self>) {
        self.annotate
            .update(cx, |a, cx| a.set_agent_source(source, cx));
    }

    /// The workspace's answer to [`BrowserEvent::SendToAgent`] or [`BrowserEvent::OpenAgent`]:
    /// what to tell the user. Sent, the notes count as sent; not, the panel looks again for the
    /// agents, as the one it offered may have ended.
    pub fn finish_send(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        self.annotate.update(cx, |a, cx| match result {
            Ok(note) => {
                a.mark_sent(cx);
                a.set_note(note, cx);
            }
            Err(why) => {
                a.refresh_agents(cx);
                a.set_note(why, cx);
            }
        });
    }

    /// Annotation notes that would be lost with the tab: not copied, sent or read by an agent.
    pub fn unsent_notes(&self, cx: &App) -> usize {
        self.annotate.read(cx).unsent(cx)
    }

    /// The annotations as the markdown agents read; `None` without any.
    pub fn annotations_markdown(&self, cx: &App) -> Option<String> {
        let panel = self.annotate.read(cx);
        let items = panel.annotations(cx);
        if items.is_empty() {
            return None;
        }
        let url = self.current_url();
        let viewport = self.viewport_json();
        let size = (
            viewport["width"].as_f64().unwrap_or_default() as u32,
            viewport["height"].as_f64().unwrap_or_default() as u32,
        );
        let device = match &self.viewport {
            ViewportMode::Fixed { name, .. } => name.clone(),
            ViewportMode::Responsive => None,
        };
        let groups: Vec<PageGroup> = group_order(&url, &items)
            .into_iter()
            .map(|key| {
                // Another page's title as it was when its first note was made.
                let title = if key == page_key(&url) && !self.title.is_empty() {
                    self.title.clone()
                } else {
                    items
                        .iter()
                        .find(|a| page_key(&a.url) == key && !a.title.is_empty())
                        .map_or_else(|| key.clone(), |a| a.title.clone())
                };
                PageGroup {
                    page: PageInfo {
                        title,
                        url: key.clone(),
                        viewport: size,
                        device: device.clone(),
                    },
                    items: items
                        .iter()
                        .filter(|a| page_key(&a.url) == key)
                        .cloned()
                        .collect(),
                }
            })
            .collect();
        let errors = if panel.include_errors() {
            self.logs.page_errors(20)
        } else {
            Vec::new()
        };
        Some(sawit_core::annotations::markdown(
            &groups,
            Some((page_key(&url), &errors)),
        ))
    }

    /// An element was annotated: capture it, then add a row and redraw the markers. A note
    /// written in the page's popover (`note`: its comment and kind) is added as it is, and the
    /// keyboard stays in the page; without one (the page couldn't show the popover yet) the
    /// row's comment field takes the keyboard.
    pub(super) fn on_picked(
        &mut self,
        mut element: ElementDetails,
        note: Option<(String, Intent)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = self.current_url();
        if let Some(source) = &mut element.source {
            source.file = relative_source(&source.file, self.project.as_deref());
        }
        let viewport = self.viewport_json();
        let size = (
            viewport["width"].as_f64().unwrap_or_default() as u32,
            viewport["height"].as_f64().unwrap_or_default() as u32,
        );
        let rect = padded_rect(&element.rect, size, 8.);
        let (tx, rx) = oneshot::channel();
        if rect.2 >= 1. && rect.3 >= 1. {
            self.shoot(Some(rect), tx, cx);
        } else {
            let _ = tx.send(Err("The element isn't on screen.".into()));
        }
        let project = self.project.clone();
        let title = self.title.clone();
        cx.spawn_in(window, async move |this, cx| {
            // Shots wait their turn; a row without a screenshot beats no row.
            let timer = cx.background_executor().timer(PICK_SHOT_TIMEOUT);
            let shot = match within(rx, timer).await {
                Some(shot) => shot.unwrap_or_else(|_| Err("The screenshot was cancelled.".into())),
                None => Err("The screenshot took too long.".into()),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                let existing = this.annotate.read(cx).annotations(cx);
                let n = next_number(&existing, &url);
                let screenshot =
                    match shot.and_then(|v| save_screenshot(&v, project.as_deref(), &url, n)) {
                        Ok(path) => Some(path),
                        Err(e) => {
                            eprintln!("sawit: no screenshot for annotation {n}: {e}");
                            None
                        }
                    };
                let from_page = note.is_some();
                let (comment, intent) = note.unwrap_or_default();
                let annotation = Annotation {
                    n,
                    url,
                    title,
                    element,
                    comment,
                    intent,
                    screenshot,
                    include_screenshot: true,
                };
                if !from_page {
                    // The keyboard moves from the page to the panel.
                    if let Some(engine) = &this.engine {
                        engine.focus_parent();
                    }
                }
                this.annotate.update(cx, |panel, cx| {
                    panel.add(annotation, !from_page, window, cx)
                });
            });
        })
        .detach();
    }

    /// The user saved or removed a note from its popover on the page: the panel follows.
    pub(super) fn on_page_annotation(
        &mut self,
        change: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(n) = change["n"].as_u64().and_then(|n| u32::try_from(n).ok()) else {
            return;
        };
        let url = self.current_url();
        match change["action"].as_str() {
            Some("update") => {
                let comment = change["comment"].as_str().unwrap_or_default().to_string();
                let intent = intent_of(&change["intent"]);
                self.annotate.update(cx, |a, cx| {
                    a.update_from_page(&url, n, &comment, intent, window, cx)
                });
            }
            Some("remove") => {
                self.annotate
                    .update(cx, |a, cx| a.remove_from_page(&url, n, cx));
            }
            _ => {}
        }
    }

    /// Tells the Annotate panel which page is on show, and its title.
    pub(super) fn sync_page_info(&self, url: &str, cx: &mut Context<Self>) {
        let title = self.title.clone();
        self.annotate
            .update(cx, |a, cx| a.set_current(url, &title, cx));
        self.sync_error_count(cx);
    }

    /// Tells the Annotate panel how many console errors the page on show has logged.
    pub(super) fn sync_error_count(&self, cx: &mut Context<Self>) {
        let errors = self.logs.page_errors(usize::MAX).len();
        self.annotate.update(cx, |a, cx| a.set_errors(errors, cx));
    }

    /// Draws the numbered markers of the page on show (none clears them).
    pub(super) fn sync_markers(&self, cx: &App) {
        let markers = self.annotate.read(cx).markers_for(&self.current_url(), cx);
        drop(self.page_op(markers_op(&self.token, &markers)));
    }
}

/// Numbers drawn over the annotated elements of the page, with what each one's popover shows.
/// Signed with the page token: the overlay shows comments only from Sawit's own call.
pub(super) fn markers_op(token: &str, markers: &[Marker]) -> Value {
    let items: Vec<Value> = markers
        .iter()
        .map(|m| {
            json!({
                "n": m.n,
                "selector": m.selector,
                "comment": m.comment,
                "intent": m.intent.label(),
                "label": m.label,
            })
        })
        .collect();
    json!({ "op": "markers", "token": token, "items": items })
}

/// The intent a page message names (`"change"`), Fix for anything else.
pub(super) fn intent_of(v: &Value) -> Intent {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

/// Writes a screenshot answer (`png_base64`) to the project's annotation
/// folder (else the app cache); `None` when it can't be saved.
fn save_screenshot(
    shot: &Value,
    project: Option<&std::path::Path>,
    url: &str,
    n: u32,
) -> Result<PathBuf, String> {
    let png = shot["png_base64"]
        .as_str()
        .and_then(sawit_core::browser::base64_decode)
        .ok_or("the capture has no readable image")?;
    let cache = sawit_core::instances::AppPaths::default_macos().cache_dir();
    let dir = screenshot_dir(project, &cache, &sawit_core::date::local_date());
    match project {
        Some(project) => sawit_core::nyawit::ensure(project, &dir),
        None => std::fs::create_dir_all(&dir),
    }
    .map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = sawit_core::nyawit::unique(&dir, &screenshot_name(url, n));
    std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}
