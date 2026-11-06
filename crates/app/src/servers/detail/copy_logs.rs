//! Copy Logs and Copy Errors on an instance's log (backlog item 1): the last
//! [`LOG_LINES`] lines, or only the error lines (`ERROR`, `FATAL`, `PANIC`) with a few around
//! each, as plain text, and a short note saying how many lines went.

use std::time::Duration;

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::copy_logs::{
    LOG_LINES, copied_note, errors_note, errors_only, last_lines, strip_ansi,
};

use super::ServerDetail;

/// How long the "Copied 42 lines" note stays.
const NOTE_FOR: Duration = Duration::from_secs(3);

/// What Copy Logs (`errors` false) or Copy Errors copies of `log`, and the note that says so.
pub(super) fn copied(log: &[String], errors: bool) -> (String, String) {
    let all = strip_ansi(&log.join("\n"));
    if errors {
        let text = errors_only(&all);
        if text.is_empty() {
            return (text, "No error lines in the log".into());
        }
        let note = errors_note(&text);
        return (text, note);
    }
    let text = last_lines(&all, LOG_LINES);
    let note = copied_note(&text);
    (text, note)
}

impl ServerDetail {
    fn copy_log(&mut self, errors: bool, cx: &mut Context<Self>) {
        let (text, note) = copied(&self.store.read(cx).log, errors);
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        self.store.update(cx, |s, cx| {
            s.notice = Some(note.clone());
            cx.notify();
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTE_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.store.update(cx, |s, cx| {
                    if s.notice.as_deref() == Some(note.as_str()) {
                        s.notice = None;
                        cx.notify();
                    }
                })
            });
        })
        .detach();
    }

    /// The log's buttons: Copy Logs and Copy Errors, off while the log is empty.
    pub(super) fn copy_buttons(&self, empty: bool, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(
                Button::new("copy-logs")
                    .ghost()
                    .xsmall()
                    .label("Copy Logs")
                    .tooltip(format!("Copy the Last {LOG_LINES} Lines"))
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| this.copy_log(false, cx))),
            )
            .child(
                Button::new("copy-errors")
                    .ghost()
                    .xsmall()
                    .label("Copy Errors")
                    .tooltip("Copy the ERROR, FATAL and PANIC Lines, with the Lines Around Them")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| this.copy_log(true, cx))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::copied;

    #[test]
    fn copy_logs_takes_the_tail_and_copy_errors_the_errors() {
        let log: Vec<String> = (0..250)
            .map(|n| match n {
                100 => "\u{1b}[31mERROR:  relation \"x\" does not exist\u{1b}[0m".to_string(),
                n => format!("LOG:  line {n}"),
            })
            .collect();
        let (all, note) = copied(&log, false);
        assert_eq!(all.lines().count(), 200);
        assert_eq!(note, "Copied 200 lines");
        assert!(all.starts_with("LOG:  line 50"));
        let (errors, note) = copied(&log, true);
        assert_eq!(
            errors,
            "LOG:  line 98\nLOG:  line 99\nERROR:  relation \"x\" does not exist\nLOG:  line 101\nLOG:  line 102"
        );
        assert_eq!(note, "Copied 5 lines, 1 error");
        let (none, note) = copied(&log[..10], true);
        assert!(none.is_empty());
        assert_eq!(note, "No error lines in the log");
    }
}
