//! The rows: notes added from the page, folded and opened, kept in step with the page's popover,
//! removed (with Undo for a while), and which of them were copied or sent; and mesurer's comment
//! threads as rows, which follow its workspace.

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::*;
use sawit_core::annotations::{Annotation, ElementDetails, Intent, Thread, page_key};

use super::{AnnotateEvent, AnnotatePanel, Marker, Row, ThreadChange, notes_lost};
use crate::browser::panel::note_timer_for;

/// How long Undo stays after a delete or a clear.
const UNDO_FOR: Duration = Duration::from_secs(8);

/// From how many notes on, picking another collapses the ones before it.
const KEEP_OPEN: usize = 3;

impl AnnotatePanel {
    /// Appends `a`. Its comment, written in the page's popover, is the row's; without one the
    /// row's comment field takes the keyboard (`focus`), as when the page couldn't ask. The new
    /// row opens; past [`KEEP_OPEN`] notes the others fold to a line each, so the list stays
    /// short enough to scan.
    pub fn add(&mut self, a: Annotation, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let comment =
            cx.new(|cx| TextareaState::new(window, cx).placeholder("What should change?"));
        if !a.comment.is_empty() {
            let text = a.comment.clone();
            comment.update(cx, |c, cx| c.set_value(text, window, cx));
        }
        if focus {
            let handle = comment.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        let changed = cx.subscribe(&comment, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.emit(AnnotateEvent::Changed);
            }
        });
        self.push(a, Some(comment), Some(changed), cx);
    }

    /// Appends a row of mesurer's comment thread (`a.source`), whose words are the thread's.
    /// Opened as a new note is, as the user just wrote it.
    pub fn add_comment(&mut self, a: Annotation, cx: &mut Context<Self>) {
        self.push(a, None, None, cx);
    }

    fn push(
        &mut self,
        a: Annotation,
        comment: Option<Entity<TextareaState>>,
        changed: Option<Subscription>,
        cx: &mut Context<Self>,
    ) {
        if self.rows.len() >= KEEP_OPEN {
            for row in &mut self.rows {
                row.collapsed = true;
            }
        }
        self.rows.push(Row {
            key: self.next_key,
            annotation: a,
            comment,
            collapsed: false,
            head: cx.focus_handle(),
            _changed: changed,
        });
        self.next_key += 1;
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    /// The comment threads with rows, for the page `page` of the workspace `origin` (mesurer's
    /// keys), by id.
    pub fn thread_ids(&self, origin: &str, page: &str) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| r.annotation.thread())
            .filter(|t| t.origin == origin && t.page == page)
            .map(|t| t.id.clone())
            .collect()
    }

    fn thread_row(&self, origin: &str, id: &str) -> Option<usize> {
        self.rows.iter().position(|r| {
            r.annotation
                .thread()
                .is_some_and(|t| t.origin == origin && t.id == id)
        })
    }

    /// Whether mesurer moved the thread's pin to another element since its row last took it.
    pub fn thread_moved(&self, thread: &Thread) -> bool {
        self.thread_row(&thread.origin, &thread.id)
            .and_then(|ix| self.rows[ix].annotation.thread())
            .is_some_and(|t| t.target != thread.target)
    }

    /// The number of the thread's row, if it has one.
    pub fn thread_number(&self, origin: &str, id: &str) -> Option<u32> {
        self.thread_row(origin, id)
            .map(|ix| self.rows[ix].annotation.n)
    }

    /// The thread's pin was moved: its row describes the new element, with its screenshot.
    pub fn set_thread_element(
        &mut self,
        origin: &str,
        id: &str,
        element: ElementDetails,
        screenshot: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.thread_row(origin, id) {
            let a = &mut self.rows[ix].annotation;
            a.element = element;
            a.screenshot = screenshot;
            cx.emit(AnnotateEvent::Changed);
            cx.notify();
        }
    }

    /// mesurer saved the thread: its row takes its messages and state (resolved, reopened).
    /// False when it has no row.
    pub fn update_thread(&mut self, thread: Thread, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.thread_row(&thread.origin, &thread.id) else {
            return false;
        };
        let row = &mut self.rows[ix];
        if row.annotation.thread() != Some(&thread) {
            row.annotation.comment = thread.first().to_string();
            row.annotation.source = sawit_core::annotations::Source::Comment(thread);
            cx.notify();
        }
        true
    }

    /// mesurer deleted the thread (or its workspace was cleared): its row goes, with no Undo,
    /// as it went in mesurer.
    pub fn drop_thread(&mut self, origin: &str, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.thread_row(origin, id) {
            self.rows.remove(ix);
            self.copied_threads.remove(id);
            cx.emit(AnnotateEvent::Changed);
            cx.notify();
        }
    }

    /// The comment rows removed or put back since last asked, for the browser to carry to
    /// mesurer.
    pub fn take_thread_changes(&mut self) -> Vec<ThreadChange> {
        std::mem::take(&mut self.thread_changes)
    }

    /// mesurer's "Copy for agent" copied the open threads of the page `page` of `origin`: their
    /// rows count as sent, and the footer says so.
    pub fn comments_copied(&mut self, origin: &str, page: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = (self.rows.iter())
            .filter_map(|r| r.annotation.thread())
            .filter(|t| t.origin == origin && t.page == page && !t.resolved())
            .map(|t| t.id.clone())
            .collect();
        let n = ids.len();
        self.copied_threads.extend(ids);
        self.set_note(
            match n {
                1 => "Copied 1 comment for an agent".to_string(),
                n => format!("Copied {n} comments for an agent"),
            },
            cx,
        );
    }

    /// Whether the thread `id` was copied for an agent.
    pub(in crate::browser) fn thread_copied(&self, id: &str) -> bool {
        self.copied_threads.contains(id)
    }

    /// Folds row `ix` to one line, or opens it.
    pub(super) fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get_mut(ix) {
            row.collapsed = !row.collapsed;
            cx.notify();
        }
    }

    /// Folds every row on the page `key`, or opens them all.
    pub(super) fn collapse_page(&mut self, key: &str, collapsed: bool, cx: &mut Context<Self>) {
        for row in &mut self.rows {
            if page_key(&row.annotation.url) == key {
                row.collapsed = collapsed;
            }
        }
        cx.notify();
    }

    /// Whether each row is folded, in order (for tests).
    #[cfg(test)]
    pub(super) fn collapsed(&self) -> Vec<bool> {
        self.rows.iter().map(|r| r.collapsed).collect()
    }

    /// The rows, with the comments as typed so far: notes and comment threads, resolved ones
    /// too (what goes to an agent leaves those out).
    pub fn annotations(&self, cx: &App) -> Vec<Annotation> {
        self.rows
            .iter()
            .map(|r| Annotation {
                comment: r.text(cx),
                ..r.annotation.clone()
            })
            .collect()
    }

    /// The markers of the notes on the page at `url` (hash ignored). A comment thread has
    /// mesurer's pin for a marker.
    pub fn markers_for(&self, url: &str, cx: &App) -> Vec<Marker> {
        let key = page_key(url);
        self.rows
            .iter()
            .filter(|r| r.comment.is_some() && page_key(&r.annotation.url) == key)
            .map(|r| Marker {
                n: r.annotation.n,
                selector: r.annotation.element.selector.clone(),
                comment: r.text(cx),
                intent: r.annotation.intent,
                label: r.annotation.element.label.clone(),
            })
            .collect()
    }

    /// The row of note `n` on the page at `url`.
    fn find(&self, url: &str, n: u32) -> Option<usize> {
        let key = page_key(url);
        self.rows.iter().position(|r| {
            r.comment.is_some() && r.annotation.n == n && page_key(&r.annotation.url) == key
        })
    }

    /// The user saved note `n` in its popover on the page at `url`. False when there is no
    /// such note.
    pub fn update_from_page(
        &mut self,
        url: &str,
        n: u32,
        comment: &str,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ix) = self.find(url, n) else {
            return false;
        };
        let row = &mut self.rows[ix];
        row.annotation.intent = intent;
        if let Some(field) = row.comment.clone() {
            let text = comment.to_string();
            field.update(cx, |c, cx| c.set_value(text, window, cx));
        }
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
        true
    }

    /// The user removed note `n` from the page at `url`.
    pub fn remove_from_page(&mut self, url: &str, n: u32, cx: &mut Context<Self>) -> bool {
        match self.find(url, n) {
            Some(ix) => {
                self.remove(ix, cx);
                true
            }
            None => false,
        }
    }

    /// Drops every row for good (an agent read them with `clear`), but resolved comment threads,
    /// which weren't read: their threads stay in mesurer. The browser deletes the threads read.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.rows.retain(|r| r.annotation.resolved());
        self.sent.clear();
        self.removed.clear();
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    /// The rows taken by a delete or a clear: their threads go from mesurer too.
    fn taken(&mut self, rows: &[(usize, Row)]) {
        for (_, row) in rows {
            if let Some(thread) = row.annotation.thread() {
                self.thread_changes
                    .push(ThreadChange::Removed(thread.clone()));
            }
        }
    }

    /// Takes every row, which Undo can bring back for a while.
    fn clear_with_undo(&mut self, cx: &mut Context<Self>) {
        let n = self.rows.len();
        let removed: Vec<(usize, Row)> = self.rows.drain(..).enumerate().collect();
        self.taken(&removed);
        self.removed = removed;
        cx.emit(AnnotateEvent::Changed);
        self.undo_note(
            if n == 1 {
                "Cleared 1 annotation".to_string()
            } else {
                format!("Cleared {n} annotations")
            },
            cx,
        );
    }

    /// Each note's `(url, n, comment)`, the comment as typed so far. Comment threads aren't
    /// notes: mesurer keeps them in its files.
    fn notes(&self, cx: &App) -> Vec<(String, u32, String)> {
        self.rows
            .iter()
            .filter(|r| r.comment.is_some())
            .map(|r| {
                let a = &r.annotation;
                (a.url.clone(), a.n, r.text(cx))
            })
            .collect()
    }

    /// How many notes would be lost with the rows: comments not copied, sent or read by an
    /// agent as they are now. The notes live only here (spec D6), so closing the tab or
    /// Clear asks first; comment threads are kept in mesurer's files, and don't count.
    pub fn unsent(&self, cx: &App) -> usize {
        self.notes(cx)
            .into_iter()
            .filter(|note| !note.2.trim().is_empty() && !self.sent.contains(note))
            .count()
    }

    /// The notes as they are now were copied, sent or read by an agent.
    pub fn mark_sent(&mut self, cx: &App) {
        self.sent = self.notes(cx);
    }

    /// Types `note` as the comment of row `ix` (for tests).
    #[cfg(test)]
    pub(in crate::browser) fn type_note(
        &mut self,
        ix: usize,
        note: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(comment) = self.rows[ix].comment.clone() {
            comment.update(cx, |c, cx| c.set_value(note.to_string(), window, cx));
        }
    }

    /// The Clear button: asks first before it takes several notes, or one not yet sent, and
    /// offers Undo after.
    pub(super) fn request_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.rows.len();
        let unsent = self.unsent(cx);
        if count == 0 {
            return;
        }
        if count == 1 && unsent == 0 {
            return self.clear_with_undo(cx);
        }
        let title = if count == 1 {
            "Clear 1 annotation?".to_string()
        } else {
            format!("Clear {count} annotations?")
        };
        let mut detail = String::new();
        if unsent > 0 {
            detail.push_str(&notes_lost(unsent));
            detail.push(' ');
        }
        detail.push_str("Their screenshots stay in .nyawit/annotations. You can undo right after.");
        let confirm = clear_label(count);
        let answer = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(&detail),
            &[confirm.as_str(), "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if let Ok(0) = answer.await {
                let _ = this.update(cx, |this, cx| this.clear_with_undo(cx));
            }
        })
        .detach();
    }

    /// Shows `text` with Undo, for longer.
    fn undo_note(&mut self, text: String, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self._note_timer = Some(note_timer_for(cx, UNDO_FOR, |this: &mut Self| {
            this.note = None;
            this.removed.clear();
        }));
        cx.notify();
    }

    /// Brings back what the last delete or clear took, comment threads into mesurer too.
    pub(in crate::browser) fn undo(&mut self, cx: &mut Context<Self>) {
        let mut back = std::mem::take(&mut self.removed);
        back.sort_by_key(|(ix, _)| *ix);
        for (ix, row) in back {
            if let Some(thread) = row.annotation.thread() {
                self.thread_changes
                    .push(ThreadChange::Restored(thread.clone()));
            }
            let at = ix.min(self.rows.len());
            self.rows.insert(at, row);
        }
        self.note = None;
        self._note_timer = None;
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    pub(in crate::browser) fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.rows.len() {
            let row = self.rows.remove(ix);
            let n = row.annotation.n;
            let what = if row.annotation.thread().is_some() {
                "comment"
            } else {
                "annotation"
            };
            let removed = vec![(ix, row)];
            self.taken(&removed);
            self.removed = removed;
            cx.emit(AnnotateEvent::Changed);
            self.undo_note(format!("Removed {what} {n}"), cx);
        }
    }

    pub(super) fn set_intent(&mut self, ix: usize, intent: Intent, cx: &mut Context<Self>) {
        if let Some(r) = self.rows.get_mut(ix) {
            r.annotation.intent = intent;
            cx.emit(AnnotateEvent::Changed);
            cx.notify();
        }
    }
}

/// The confirmation button of Clear, naming what goes.
pub(super) fn clear_label(count: usize) -> String {
    if count == 1 {
        "Clear annotation".into()
    } else {
        format!("Clear {count} annotations")
    }
}
