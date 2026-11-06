//! Send to Agent: screens and artboards handed to a coding agent to build in the project's own
//! stack, one frame or several as a flow. Their files go to the project's
//! `.nyawit/handoff/<name>/` ([`sawit_board::handoff`]: `design.png` at 2×, each frame's HTML and
//! React, `spec.md` and `prompt.md`). Choosing an agent opens its CLI in a new terminal tab in the
//! project with the prompt as its first message ([`crate::browser::open_command`]); Copy Prompt
//! puts the prompt on the clipboard; Reveal Handoff Folder shows the folder in the Finder. Each
//! says what it did in a toast, with the folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::handoff::{self, Handoff};
use sawit_core::terminal::AgentKind;

use super::pictures::png_of;
use crate::board::BoardView;

/// What Send to Agent does once the folder is written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::board) enum SendTo {
    Agent(AgentKind),
    CopyPrompt,
    Reveal,
}

/// What the workspace does with a design sent to an agent: runs `command` in a new terminal tab
/// in `project`.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct SendToAgent {
    pub kind: AgentKind,
    pub project: Option<PathBuf>,
    pub command: String,
}

/// The command line that starts `kind` in a terminal with the prompt in `file` as its first
/// message; `None` for a CLI Sawit can't start with one.
pub(super) fn agent_command(kind: AgentKind, file: &Path) -> Option<String> {
    crate::browser::open_command(kind, file)
}

impl BoardView {
    /// Hands `frames` (screens and artboards) off: their screens' pictures are made into PNGs off
    /// the UI thread, then the folder is written ([`handoff::write`]) and `to` carried out.
    pub(in crate::board) fn send_frames(
        &mut self,
        frames: Vec<NodeId>,
        to: SendTo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pictures: Vec<(NodeId, Arc<RenderImage>)> = (frames.iter())
            .filter_map(|f| Some((*f, self.screen_picture(*f)?)))
            .collect();
        let encoded = cx.background_spawn(async move {
            let pngs = pictures.iter().filter_map(|(f, p)| Some((*f, png_of(p)?)));
            pngs.collect::<Vec<_>>()
        });
        let project = self.canvas_project();
        cx.spawn_in(window, async move |this, cx| {
            let pictures = encoded.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let said = match this.write_handoff(&frames, &pictures) {
                    Err(error) => error,
                    Ok(done) => carry_out(done, to, project, window, cx),
                };
                this.show_toast(said, cx);
            });
        })
        .detach();
    }

    /// Writes the handoff of `frames` with the screens' `pictures`.
    fn write_handoff(
        &mut self,
        frames: &[NodeId],
        pictures: &[(NodeId, Vec<u8>)],
    ) -> Result<Handoff, String> {
        let dir = self.screens_dir();
        let board = self.board_mut().ok_or("The canvas isn't open")?;
        let doc = board.doc().map_err(|e| e.for_user())?;
        let targets = handoff::targets(&doc, frames)?;
        let out = handoff::folder(&dir, &handoff::name(&targets));
        handoff::write(board, &targets, &dir, &out, pictures)
    }
}

/// Does `to` with the written handoff `done`; what to say of it.
fn carry_out(
    done: Handoff,
    to: SendTo,
    project: Option<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) -> String {
    let folder = done.folder.display().to_string();
    match to {
        SendTo::CopyPrompt => {
            cx.write_to_clipboard(ClipboardItem::new_string(done.prompt));
            format!("Copied the prompt; the files are in {folder}")
        }
        SendTo::Reveal => {
            cx.reveal_path(&done.prompt_file);
            format!("Handoff written to {folder}")
        }
        SendTo::Agent(kind) => match agent_command(kind, &done.prompt_file) {
            Some(command) => {
                let send = SendToAgent {
                    kind,
                    project,
                    command,
                };
                open_agent(send, &folder, window, cx)
            }
            None => format!("Sawit can't start {} with a prompt", kind.name()),
        },
    }
}

/// Opens `send`'s agent in a new terminal tab of the window's workspace; what to say of it.
fn open_agent(send: SendToAgent, folder: &str, window: &mut Window, cx: &mut App) -> String {
    let Some(workspace) = crate::cli::workspace_for(window.window_handle(), cx) else {
        return "The window's terminal couldn't be found".into();
    };
    let opened = workspace.update(cx, |ws, cx| {
        ws.run_agent_in_new_terminal(send.project, &send.command, window, cx)
    });
    match opened {
        Ok(()) => format!("Opened {} with the handoff in {folder}", send.kind.name()),
        Err(error) => error,
    }
}
