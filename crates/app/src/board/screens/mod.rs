//! UI screens in the canvas's tab ([`sawit_board::screens`]): what agents ask of them while the
//! app runs ([`ops`]), the bar over a selected screen with Preview, the downloads and Send to
//! Agent ([`bar`]), the full-size Preview ([`preview`]), the handoff to a coding agent
//! ([`handoff`]), and the screens' pictures in exports ([`pictures`]). How a screen is rendered,
//! watched and made live is the web frames' ([`super::web`]).

#[cfg(test)]
mod agent_tests;
mod bar;
mod handoff;
mod ops;
mod pictures;
mod preview;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::screens::{self, ScreenData};
use sawit_core::terminal::AgentKind;

use super::BoardView;
pub(super) use bar::render_screen_bar;
pub(super) use preview::{Preview, render_preview};

/// The screens' side of the canvas's tab.
#[derive(Default)]
pub(in crate::board) struct ScreenUi {
    /// The screen shown full size, if one is.
    pub preview: Option<Preview>,
    /// The coding agents on this Mac that Send to Agent can start, once they are looked for.
    pub agents: Option<Vec<AgentKind>>,
    pub finding_agents: bool,
}

/// The one screen selected on the canvas: its frame, its plugin data and its frame's name.
#[derive(Clone, Debug)]
pub(in crate::board) struct Selected {
    pub frame: NodeId,
    pub data: ScreenData,
    pub title: String,
}

impl BoardView {
    /// The canvas's screens folder ([`screens::screens_dir`]).
    pub(in crate::board) fn screens_dir(&self) -> PathBuf {
        screens::screens_dir(&sawit_board::store::bound_file(self.path()))
    }

    /// The project the canvas is a project's canvas of (its `.nyawit` folder's parent).
    pub(in crate::board) fn canvas_project(&self) -> Option<PathBuf> {
        let dir = self.path().parent()?;
        let in_nyawit = dir
            .file_name()
            .is_some_and(|n| n == sawit_core::nyawit::DIR);
        in_nyawit.then(|| dir.parent().map(PathBuf::from)).flatten()
    }

    /// The screen selected alone on the canvas, when one is and it isn't live.
    pub(in crate::board) fn selected_screen(&self) -> Option<Selected> {
        let ds = self.board()?.session.ds().ok()?;
        let [frame] = ds.selection.as_slice() else {
            return None;
        };
        if self.web_live() == Some(*frame) {
            return None;
        }
        let node = ds.doc.node(*frame)?;
        let data = screens::screen_of(node)?;
        Some(Selected {
            frame: *frame,
            data,
            title: node.name.clone(),
        })
    }

    /// The latest picture of screen frame `frame`, if it has been rendered.
    pub(in crate::board) fn screen_picture(&self, frame: NodeId) -> Option<Arc<RenderImage>> {
        let web = self.web.as_ref()?;
        let texture = web.frames.get(&frame)?.texture.as_ref()?;
        Some(texture.image.clone())
    }
}
