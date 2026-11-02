//! Running ops and commands on the canvas's document, an agent's control ops and the chrome's
//! commands alike, then carrying out what the engine asked of the canvas; and ending the text
//! being typed in before a command that would drop it.

use std::sync::Arc;

use figcraft_engine::UiRequest;
use gpui_kit::*;
use sawit_board::{BoardDoc, BoardError, input, ops, store};
use serde_json::{Value, json};

use super::{BoardView, layers_panel};

impl BoardView {
    /// Runs a control op, `{"op": "get" | "command" | ...}` (`sawit_board::ops::run`), on the
    /// document, and answers it as the control socket replies: `{ok: true, ...}` or
    /// `{ok: false, error}`.
    ///
    /// Then the canvas carries out what the op asked of it, as after the same input in the
    /// window, redraws, and saves whatever the op changed: an op arrives with no input in the
    /// window, perhaps while it is covered and its display link has stopped, so the save can't
    /// wait for the next paint.
    ///
    /// While the Actions palette, the shortcuts sheet or the context menu is open, a `key` op of
    /// Escape closes it, as the key does in the window, and the canvas never gets it ([`menus`]).
    ///
    /// An op other than a read first ends a scrub of the Design panel that is on, kept
    /// ([`BoardView::end_scrub`]): a colour picker's closes its picker, and a label's drag
    /// scrubs nothing more until its release. The op is then an undo step of its own, which an
    /// Escape in the picker or during the drag can't take back, and an undo op finds no scrub
    /// over the document it puts back. A read leaves the scrub on.
    ///
    /// An op that changes the document also ends the text being typed in on the canvas first,
    /// unless it is a command that acts on that text (`sawit_board::ops::run`, ruling
    /// R-TEXTEDIT): what was typed is an undo step of its own, and the next click keeps it.
    /// While the user holds a drag on the canvas, such an op is refused instead, "<op>: finish
    /// the drag on the canvas first", and changes nothing: the drag's next move would take it
    /// back. No scrub is on then for it to end: the press that began the drag closed a colour
    /// picker's, and a label's holds the same button. So is one while the Pen draws a path,
    /// "<op>: finish the path the Pen draws on the canvas first": it would join the path's undo
    /// step (CANVAS-CORE-8).
    ///
    /// An op that takes layers off the canvas, on any page (a `clear`, a `delete`, a `replace`, an
    /// engine command such as `edit.delete` or `page.delete`), has its save keep what the file
    /// holds as a backup first, the ones before it too (CANVAS-CORE-3): undo isn't saved, so after
    /// the tab closes that is all that is left of those layers. What the op left tells whether it
    /// took any off, and which backup it keeps: the board's for a clear or a load, a deletion's,
    /// apart, for the rest ([`store::backup_after`]). Before an op that can, the board as the
    /// canvas has it is handed to the writer, an edit made a moment before included, which the
    /// autosave would have saved [`SAVE_DELAY`] later: the backup is of it, not of the file as it
    /// was before that edit. The user's own deletions keep none: they have undo, and a backup at
    /// each ⌫ would soon push the ones an agent's ops kept out of the five there are.
    pub fn run_op(&mut self, op: &Value, cx: &mut Context<Self>) -> Value {
        self.run_op_with(op, |board| ops::run(board, op), cx)
    }

    /// Draws a `mermaid` op's diagram that was rendered and made into skeletons off the UI thread
    /// (`sawit_board::ops::prepare_diagram`), as [`Self::run_op`] runs the op: Pele takes up to
    /// its time limit, 5 s, over a big diagram, which the window would freeze for.
    pub fn run_prepared_op(
        &mut self,
        prepared: ops::PreparedDiagram,
        cx: &mut Context<Self>,
    ) -> Value {
        let op = json!({"op": "mermaid"});
        self.run_op_with(&op, |board| ops::run_prepared(board, prepared), cx)
    }

    /// [`Self::run_op`], with `run` running `op` on the document.
    fn run_op_with(
        &mut self,
        op: &Value,
        run: impl FnOnce(&mut BoardDoc) -> Value,
        cx: &mut Context<Self>,
    ) -> Value {
        if let Some(reply) = self.close_menus_for_op(op, cx) {
            return reply;
        }
        if !reads_only(op) {
            self.end_scrub(cx);
        }
        if can_take_layers_off(op) {
            // A refusal (images the file holds) is the autosave's to show.
            let _ = self.queue_save();
            // A load reads its file at once, and that may be the backup an agent's clear just
            // kept (`canvas_load` of `<file>.bak` right after `canvas_clear`): what is queued,
            // and the backup it keeps, goes to disk first, as with the app closed.
            if loads_a_file(op) {
                self.writer.flush();
            }
        }
        let before = self.document();
        let reply = match self.board_mut() {
            Some(board) => run(board),
            None => self.not_opened(),
        };
        if let (Some(before), Some(after)) = (&before, self.document())
            && !Arc::ptr_eq(before, &after)
        {
            let name = op.get("op").and_then(Value::as_str).unwrap_or_default();
            let backup = store::backup_after(name, before, &after);
            self.next_backup = self.next_backup.max(backup);
        }
        self.handle_ui_requests(Vec::new(), cx);
        self.autosave(cx);
        cx.notify();
        reply
    }

    /// Runs engine command `command` with `params`, as a click on the chrome does: a tool's
    /// button, a flyout's or the context bar's choice, an item of the zoom menu. It is the one way
    /// the chrome runs a command (ruling P3-C13), in the order FigCraft's own app runs one
    /// (ui-egui `app/commands.rs`): the engine runs it, the canvas carries out what the engine
    /// asked of it, then the engine's error shows as a toast, in plain words
    /// ([`Self::show_error`]; last, so that it is the toast that shows), and the canvas redraws.
    /// Any command goes through as it is given.
    ///
    /// A text being typed in on the canvas ends first, unless the command acts on it
    /// ([`Self::end_text_edit`], ruling R-TEXTEDIT).
    pub fn choose(&mut self, command: &str, params: Value, cx: &mut Context<Self>) {
        if self.end_text_edit(Some(command), cx) {
            self.run(|board| board.execute(command, &params), cx);
        }
    }

    /// Runs `f` on the document as [`Self::choose`] runs a command, with what follows it: the
    /// engine's requests, the error's toast and a redraw. What `f` gave, or `None` when it failed
    /// or no file is open.
    pub(super) fn run<T>(
        &mut self,
        f: impl FnOnce(&mut BoardDoc) -> Result<T, BoardError>,
        cx: &mut Context<Self>,
    ) -> Option<T> {
        let done = f(self.board_mut()?);
        self.handle_ui_requests(Vec::new(), cx);
        let done = match done {
            Ok(done) => Some(done),
            Err(error) => {
                self.show_error(&error, cx);
                None
            }
        };
        cx.notify();
        done
    }

    /// Says why `error` stopped what the user did, in a toast, in plain words: the engine's advice
    /// to agents ("pass `ids`") is for an agent's reply, never for the user
    /// (`BoardError::for_user`).
    pub(super) fn show_error(&mut self, error: &BoardError, cx: &mut Context<Self>) {
        self.show_toast(error.for_user(), cx);
    }

    /// Carries out what the engine asked of the canvas: `requests`, which an input already took
    /// out of the session, then any still waiting in `session.ui_requests`. It runs after every
    /// pointer event, key and op, as FigCraft's own canvas handles its requests after each input.
    ///
    /// `sawit_board::input::handle_requests` does the ones that need no view (`board.editText`
    /// starts typing in a sticky). `panel.design` (from `view.design`: ⌥8, the top bar's toggle,
    /// an agent's command) shows or hides the Design panel, and `panel.layers` (`view.layers`: ⌥1,
    /// the top bar's toggle) and `panel.leftSidebar` (`view.leftSidebar`: ⇧⌘\) the layers panel
    /// (ruling P4-C10). The layers panel ([`layers_panel`]) starts typing a layer's name for
    /// `layers.rename` (`object.rename` without a name: ⌘R, F2) and `canvas.renameLabel` (a
    /// double-click on a frame's name on the canvas), and closes its rows for
    /// `layers.collapseAll` (⌥L). `palette.actions` (`app.actions`: ⌘K, ⌘/) and
    /// `panel.shortcuts` (`help.shortcuts`: ⌃⇧?) open or close the Actions palette and the
    /// shortcuts sheet, and `contextMenu` (`view.contextMenu`: ⇧F10) opens the context menu under
    /// the selection ([`menus`]). A message the engine left for the user shows as a toast.
    pub(super) fn handle_ui_requests(
        &mut self,
        mut requests: Vec<UiRequest>,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.board_mut() else {
            return;
        };
        requests.append(&mut board.session.ui_requests);
        let rest = input::handle_requests(board, &requests);
        let message = board.session.message.take();
        for request in rest {
            match request.kind.as_str() {
                "panel.design" => self.show_design = !self.show_design,
                "panel.layers" | "panel.leftSidebar" => self.show_layers = !self.show_layers,
                "layers.rename" => self.start_rename(&layers_panel::ids_in(&request.data), cx),
                "canvas.renameLabel" => {
                    let id = request.data.get("id");
                    if let Some(id) = id.and_then(figcraft_engine::params::id_of) {
                        self.start_rename(&[id], cx);
                    }
                }
                "layers.collapseAll" => self.collapse_layers(),
                "palette.actions" => self.toggle_palette(cx),
                "panel.shortcuts" => self.toggle_sheet(cx),
                "contextMenu" => self.open_context_menu_from_keys(cx),
                kind => log::debug!("the canvas has no handler yet for request {kind}"),
            }
        }
        if let Some(message) = message {
            self.show_toast(message, cx);
        }
    }

    /// Whether the canvas is editing a text: typing goes into it.
    pub(super) fn editing_text(&self) -> bool {
        self.board()
            .is_some_and(|board| board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()))
    }

    /// Ends the text being typed in on the canvas, if there is one, as a press outside the text
    /// ends it: what was typed becomes one undo step, a sticky's text gives the selection back to
    /// its sticky, and a new text left empty goes. `before` is the command the caller runs next.
    /// When that command acts on the text, the edit stays open: the Text menu's commands, and copy
    /// and paste (`sawit_board::input::end_text_edit_before`). `None` ends the edit whatever comes
    /// next, as a right-click does ([`events`]).
    ///
    /// It is ruling R-TEXTEDIT's one helper: the canvas ends typing through it before a command of
    /// its chrome, its menus and its layers panel ([`Self::choose`], [`Self::run_menu`], a row's
    /// selection) and before a right-click's selection. An agent's op ends it in the session, by
    /// the same rule (`sawit_board::ops::run`). A command that drops the edit without ending it
    /// (Select none, a page or a tool switch, deleting the text) left the engine's "Edit text"
    /// gesture open. The next click on a layer then cancelled the gesture, which took the typing
    /// back (Plan 6 final review C1).
    ///
    /// Whether the caller may go on: false when the edit couldn't end, which a toast then says.
    /// The caller then runs nothing, since its command could drop the edit without ending it.
    pub(super) fn end_text_edit(&mut self, before: Option<&str>, cx: &mut Context<Self>) -> bool {
        if !self.editing_text() {
            return true;
        }
        let Some(board) = self.board_mut() else {
            return true;
        };
        let ended = match before {
            Some(command) => input::end_text_edit_before(board, command),
            None => input::end_text_edit(board),
        };
        if let Err(error) = ended {
            self.show_error(&error, cx);
            return false;
        }
        cx.notify();
        true
    }

    /// The answer to an op when the file couldn't be opened: why, as `{ok: false, error}`.
    fn not_opened(&self) -> Value {
        let reason = self.board.as_ref().err().map_or("", |error| error.as_ref());
        json!({
            "ok": false,
            "error": format!(
                "The canvas file {} couldn't be opened: {reason}",
                self.path().display()
            ),
        })
    }
}

/// Whether control op `op` only reads the canvas, and changes nothing: `get` and `commands`
/// (`sawit_board::ops::run`).
fn reads_only(op: &Value) -> bool {
    matches!(
        op.get("op").and_then(Value::as_str),
        Some("get" | "commands")
    )
}

/// Whether `op`, an agent's, can take layers off the canvas: a `clear`, a `delete`, a `replace`,
/// or an engine command (`edit.delete`, `edit.cut`, `page.delete`, a `command.batch` of them...).
/// The canvas hands its board to the writer before one ([`BoardView::run_op`]), so that the backup
/// the op's save keeps is of the board the agent acted on. The input ops can too (⌫, the eraser),
/// but they are the live checks', not a tool's: what they take off is kept as the file had it.
fn can_take_layers_off(op: &Value) -> bool {
    matches!(
        op.get("op").and_then(Value::as_str),
        Some("clear" | "delete" | "replace" | "command")
    )
}

/// Whether `op` loads a file in place of the board: a `replace` with a `path`, which it reads
/// as it runs ([`BoardView::run_op`]).
fn loads_a_file(op: &Value) -> bool {
    op.get("op").and_then(Value::as_str) == Some("replace") && op.get("path").is_some()
}
