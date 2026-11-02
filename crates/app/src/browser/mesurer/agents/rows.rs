//! mesurer's comment threads as rows of the Annotate panel. Each workspace mesurer saves is read
//! for the threads of the page on show: a new one becomes a row of kind Comment, with the
//! element's details as a pick has them (the page's `details` op, while the element is in the
//! page's own document) and a screenshot of it, as a pick takes one; a changed one updates its
//! row (messages, resolved); a deleted one drops it. A thread in one of Responsive's previews, or
//! a frame, keeps what mesurer captured of its element, and the preview's size.

use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::annotations::mesurer::threads_of;
use sawit_core::annotations::{
    Annotation, ElementDetails, Intent, Source, Thread, next_number, relative_source,
};
use sawit_core::browser::mesurer::page_artifacts;
use serde_json::json;

use super::super::super::BrowserView;
use super::super::super::annotate::padded_rect;
use super::super::super::notes::{PICK_SHOT_TIMEOUT, save_screenshot};
use super::super::super::shots::within;
use super::here;

/// How long a new thread's row waits for the page to tell of its element.
const DETAILS_TIMEOUT: Duration = Duration::from_secs(5);

/// The rows follow the threads of the page on show, as the workspace held has them. Nothing
/// changes while it isn't held (no mount has read it yet).
pub(super) fn sync(view: &mut BrowserView, cx: &mut Context<BrowserView>) {
    let (url, origin, page) = here(view);
    if !view.mesurer.store.holds(&origin) {
        return;
    }
    let artifacts = (view.mesurer.store.workspace(&origin))
        .map(|envelope| page_artifacts(envelope, &page))
        .unwrap_or_default();
    let threads = threads_of(&artifacts, &origin, &page);
    let first = (view.mesurer.agents.synced).insert((origin.clone(), page.clone()));
    let rows = view.annotate.read(cx).thread_ids(&origin, &page);
    let gone: Vec<&String> = rows
        .iter()
        .filter(|id| !threads.iter().any(|(t, _)| &t.id == *id))
        .collect();
    if !gone.is_empty() {
        view.annotate.update(cx, |a, cx| {
            for id in gone {
                a.drop_thread(&origin, id, cx);
            }
        });
    }
    for (thread, element) in threads {
        let id = thread.id.clone();
        let moved = view.annotate.read(cx).thread_moved(&thread);
        let updated = view
            .annotate
            .update(cx, |a, cx| a.update_thread(thread.clone(), cx));
        if updated && moved {
            look_again(view, thread, element, url.clone(), cx);
            continue;
        }
        // A row the panel removed (Undo can still bring it back) isn't made again.
        let taken = view.mesurer.agents.taken.contains_key(&id);
        if !updated && !taken && view.mesurer.agents.making.insert(id) {
            make_row(view, thread, element, !first, url.clone(), cx);
        }
    }
}

/// Makes the row of a thread new to the panel: its element as the page tells it, a screenshot of
/// it when `shoot` (a thread just written), then the row, as the thread is by then.
fn make_row(
    view: &mut BrowserView,
    thread: Thread,
    element: ElementDetails,
    shoot: bool,
    url: String,
    cx: &mut Context<BrowserView>,
) {
    let details = (thread.in_document() && !element.selector.is_empty())
        .then(|| view.page_op(json!({ "op": "details", "selector": element.selector })));
    let viewport = view.viewport_json();
    let size = (
        viewport["width"].as_f64().unwrap_or_default() as u32,
        viewport["height"].as_f64().unwrap_or_default() as u32,
    );
    let project = view.project.clone();
    let title = view.title.clone();
    cx.spawn(async move |this, cx| {
        let mut element = element;
        if let Some(details) = details {
            let timer = cx.background_executor().timer(DETAILS_TIMEOUT);
            if let Some(Ok(Ok(answer))) = within(details, timer).await
                && let Ok(mut found) =
                    serde_json::from_value::<ElementDetails>(answer["element"].clone())
            {
                if let Some(source) = &mut found.source {
                    source.file = relative_source(&source.file, project.as_deref());
                }
                element = found;
            }
        }
        let rect = padded_rect(&element.rect, size, 8.);
        let shot = if shoot && thread.in_document() && rect.2 >= 1. && rect.3 >= 1. {
            let (tx, rx) = oneshot::channel();
            let _ = this.update(cx, |this, cx| this.shoot(Some(rect), tx, cx));
            let timer = cx.background_executor().timer(PICK_SHOT_TIMEOUT);
            // A row without a screenshot beats no row.
            within(rx, timer)
                .await
                .and_then(Result::ok)
                .and_then(Result::ok)
        } else {
            None
        };
        let _ = this.update(cx, |this, cx| {
            this.mesurer.agents.making.remove(&thread.id);
            // The thread as it is now: written to since, or deleted.
            let now = (this.mesurer.store.workspace(&thread.origin))
                .map(|envelope| page_artifacts(envelope, &thread.page))
                .map(|page| threads_of(&page, &thread.origin, &thread.page))
                .and_then(|all| all.into_iter().find(|(t, _)| t.id == thread.id));
            let Some((thread, _)) = now else { return };
            if this.mesurer.agents.taken.contains_key(&thread.id)
                || this
                    .annotate
                    .update(cx, |a, cx| a.update_thread(thread.clone(), cx))
            {
                return;
            }
            let existing = this.annotate.read(cx).annotations(cx);
            let n = next_number(&existing, &url);
            let screenshot = shot.and_then(|shot| {
                save_screenshot(&shot, project.as_deref(), &url, n)
                    .map_err(|e| eprintln!("sawit: no screenshot for comment {n}: {e}"))
                    .ok()
            });
            let annotation = Annotation {
                n,
                url,
                title,
                element,
                comment: thread.first().to_string(),
                intent: Intent::Fix,
                screenshot,
                include_screenshot: true,
                source: Source::Comment(thread),
            };
            this.annotate
                .update(cx, |a, cx| a.add_comment(annotation, cx));
        });
    })
    .detach();
}

/// The thread's pin was moved to another element: its row describes that one, as the page tells
/// it, with a screenshot of it (a thread in a preview or a frame keeps what mesurer captured).
fn look_again(
    view: &mut BrowserView,
    thread: Thread,
    element: ElementDetails,
    url: String,
    cx: &mut Context<BrowserView>,
) {
    let on_page = thread.in_document() && !element.selector.is_empty();
    let details =
        on_page.then(|| view.page_op(json!({ "op": "details", "selector": element.selector })));
    let viewport = view.viewport_json();
    let size = (
        viewport["width"].as_f64().unwrap_or_default() as u32,
        viewport["height"].as_f64().unwrap_or_default() as u32,
    );
    let project = view.project.clone();
    cx.spawn(async move |this, cx| {
        let mut element = element;
        if let Some(details) = details {
            let timer = cx.background_executor().timer(DETAILS_TIMEOUT);
            if let Some(Ok(Ok(answer))) = within(details, timer).await
                && let Ok(mut found) =
                    serde_json::from_value::<ElementDetails>(answer["element"].clone())
            {
                if let Some(source) = &mut found.source {
                    source.file = relative_source(&source.file, project.as_deref());
                }
                element = found;
            }
        }
        let rect = padded_rect(&element.rect, size, 8.);
        let shot = if on_page && rect.2 >= 1. && rect.3 >= 1. {
            let (tx, rx) = oneshot::channel();
            let _ = this.update(cx, |this, cx| this.shoot(Some(rect), tx, cx));
            let timer = cx.background_executor().timer(PICK_SHOT_TIMEOUT);
            within(rx, timer)
                .await
                .and_then(Result::ok)
                .and_then(Result::ok)
        } else {
            None
        };
        let _ = this.update(cx, |this, cx| {
            let Some(n) = (this.annotate.read(cx)).thread_number(&thread.origin, &thread.id) else {
                return;
            };
            let screenshot = shot.and_then(|shot| {
                save_screenshot(&shot, project.as_deref(), &url, n)
                    .map_err(|e| eprintln!("sawit: no screenshot for comment {n}: {e}"))
                    .ok()
            });
            this.annotate.update(cx, |a, cx| {
                a.set_thread_element(&thread.origin, &thread.id, element, screenshot, cx)
            });
        });
    })
    .detach();
}
