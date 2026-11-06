//! Copying a command's output (backlog item 1): Copy Last Output and Copy Error put the output of
//! the last command typed at the prompt on the clipboard as plain text, wrapped rows joined, no
//! escape codes (a terminal's screen holds characters, never codes). The command is found where
//! Return was pressed at the shell's prompt ([`TerminalView::write`]); without one, the screen as
//! it shows is copied. Copy Error keeps only the error lines and a few around each
//! ([`sawit_core::copy_logs::errors_only`]).

use gpui_kit::*;
use sawit_core::copy_logs::{copied_note, errors_note, errors_only};

use super::TerminalView;

impl TerminalView {
    /// The output of the last command typed at the prompt: from the row after it to the prompt
    /// that followed (or to the end while it still runs); the screen when no command was seen.
    pub fn last_output(&self) -> String {
        match self.command_mark.get() {
            Some(mark) => {
                let end = if self.at_prompt() {
                    self.session.cursor_mark()
                } else {
                    usize::MAX
                };
                self.session.text_from(mark + 1, end)
            }
            None => self.session.screen_text(),
        }
    }

    /// Copies [`Self::last_output`] (or only its errors, with `errors`) and says how many lines.
    /// Copy Error with no error line in the output copies the whole output, and says so.
    pub fn copy_last_output(&mut self, errors: bool, cx: &mut Context<Self>) {
        let output = self.last_output();
        let errored = errors
            .then(|| errors_only(&output))
            .filter(|e| !e.is_empty());
        let (text, note) = match errored {
            Some(text) => {
                let note = errors_note(&text);
                (text, note)
            }
            None if errors => {
                let note = format!("No error lines: {}", copied_note(&output).to_lowercase());
                (output, note)
            }
            None => {
                let note = copied_note(&output);
                (output, note)
            }
        };
        if text.is_empty() {
            self.copy_saying(String::new(), "Nothing to copy".into(), cx);
            return;
        }
        self.copy_saying(text, note.into(), cx);
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use crate::terminal::testing::{init, open_panel, ready, wait_until};

    /// The last command's output is copied as plain text: from the row after the command to
    /// the prompt after it, colours gone; Copy Error keeps the error lines.
    #[gpui_kit::test]
    fn the_last_commands_output_is_copied_without_codes(cx: &mut TestAppContext) {
        cx.update(init);
        let (_, panel) = open_panel(cx);
        let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &view);
        let command = "printf 'one\\n\\033[31merror: two\\033[0m\\nthree\\n'";
        cx.update(|cx| {
            view.read(cx)
                .write(sawit_core::terminal::run_bytes(command))
        });
        wait_until(cx, "the command ran", |cx| {
            let v = view.read(cx);
            v.at_prompt() && v.text_tail(20).contains("three")
        });
        let out = cx.update(|cx| view.read(cx).last_output());
        assert_eq!(out, "one\nerror: two\nthree");
        cx.update(|cx| view.update(cx, |v, cx| v.copy_last_output(true, cx)));
        let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        assert_eq!(copied.as_deref(), Some("one\nerror: two\nthree"));
        assert_eq!(
            cx.update(|cx| view.read(cx).toast_text.to_string()),
            "Copied 3 lines, 1 error"
        );
    }
}
