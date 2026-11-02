//! Send to Agent: a screen handed to a coding agent to build in the project's own stack. Its
//! files go to the project's `.nyawit/handoff/<screen>/` (the HTML in one file, the React
//! version, a PNG of it as the canvas shows it) with the prompt, `prompt.md`, which asks the agent
//! to build the screen in the project's stack following Sawit's design standard. Choosing an
//! agent opens its CLI in a new terminal tab in the project with that prompt as its first message
//! ([`crate::browser::open_command`]); Copy Prompt puts the prompt on the clipboard instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::screens::{self, export};
use sawit_core::terminal::AgentKind;

use super::pictures::png_of;
use crate::board::BoardView;

/// The design standard the prompt names: the skill Sawit ships, and the tool that reads it.
pub(super) const STANDARD: &str = "Sawit's canvas design standard (the sawit-canvas-design skill; \
     with Sawit's MCP server, canvas_design_guide)";

/// A screen's files, written for an agent.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Handoff {
    pub prompt: String,
    pub prompt_file: PathBuf,
}

/// What the workspace does with a screen sent to an agent: runs `command` in a new terminal tab
/// in `project`.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct SendToAgent {
    pub kind: AgentKind,
    pub project: Option<PathBuf>,
    pub command: String,
}

/// What the prompt says the screen's files are.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Attached {
    pub html: Option<PathBuf>,
    pub tsx: Option<PathBuf>,
    pub css: Option<PathBuf>,
    pub png: Option<PathBuf>,
    pub folder: PathBuf,
}

/// The prompt that hands screen `title` (folder name `name`) to a coding agent, with its files.
pub(super) fn prompt(title: &str, name: &str, files: &Attached) -> String {
    let line = |what: &str, path: &Option<PathBuf>| {
        path.as_ref()
            .map(|path| format!("- {what}: {}\n", path.display()))
            .unwrap_or_default()
    };
    let react = match (&files.tsx, &files.css) {
        (Some(tsx), Some(css)) => format!(
            "- The same screen as a React component, a mechanical conversion for reference: {} \
             with {}\n",
            tsx.display(),
            css.display()
        ),
        _ => String::new(),
    };
    format!(
        "Build the screen \"{title}\" in this project, in its own stack: its framework, its \
         components, its styles and tokens, and its conventions for pages and routes. Make it a \
         real, working screen of the app.\n\n\
         It was designed on Sawit's canvas as HTML (screen \"{name}\"):\n\
         {html}{react}{png}- Its source folder: {folder}\n\n\
         Match the design's layout, copy, colours, type and spacing. Reuse the project's existing \
         components and tokens where they fit rather than copying the HTML wholesale, wire up what \
         the screen implies (forms, links, states), and keep it accessible. Follow {STANDARD}: \
         read it before you start, and run its checklist before you say you're done.\n",
        html = line("The HTML, CSS and images in one file", &files.html),
        png = line("A picture of it as designed", &files.png),
        folder = files.folder.display(),
    )
}

/// The command line that starts `kind` in a terminal with the prompt in `file` as its first
/// message; `None` for a CLI Sawit can't start with one.
pub(super) fn agent_command(kind: AgentKind, file: &Path) -> Option<String> {
    crate::browser::open_command(kind, file)
}

/// Writes screen `name`'s handoff (its folder `folder`, its picture `png`) to `out`: the HTML in
/// one file, the React version, the PNG and the prompt. What can't be made is left out of the
/// prompt; only the prompt itself failing is an error.
pub(super) fn write_handoff(
    folder: &Path,
    name: &str,
    title: &str,
    out: &Path,
    png: Option<Vec<u8>>,
) -> Result<Handoff, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("Couldn't make {}: {e}", out.display()))?;
    let component = screens::component_name(name);
    let html = out.join(format!("{name}.html"));
    let tsx = out.join(format!("{component}.tsx"));
    let mut files = Attached {
        folder: folder.to_path_buf(),
        ..Attached::default()
    };
    if export::write_html(folder, &html).is_ok() {
        files.html = Some(html);
    }
    if export::write_react(folder, name, &tsx).is_ok() {
        files.css = Some(tsx.with_extension("css"));
        files.tsx = Some(tsx);
    }
    if let Some(png) = png {
        let path = out.join(format!("{name}.png"));
        if std::fs::write(&path, png).is_ok() {
            files.png = Some(path);
        }
    }
    let prompt = prompt(title, name, &files);
    let prompt_file = out.join("prompt.md");
    std::fs::write(&prompt_file, &prompt)
        .map_err(|e| format!("Couldn't write {}: {e}", prompt_file.display()))?;
    Ok(Handoff {
        prompt,
        prompt_file,
    })
}

impl BoardView {
    /// Where screen `name`'s handoff goes: the project's `.nyawit/handoff/<name>/`, else a
    /// `handoff` folder beside the screens folder.
    fn handoff_dir(&self, name: &str) -> PathBuf {
        let screens = self.screens_dir();
        let beside = screens.parent().unwrap_or(Path::new("."));
        beside.join("handoff").join(name)
    }

    /// Sends screen frame `frame` to coding agent `kind` (or, with none, copies the prompt): its
    /// files are written off the UI thread, then the agent's CLI opens in a new terminal tab in
    /// the project with the prompt.
    pub(in crate::board) fn send_screen(
        &mut self,
        frame: NodeId,
        kind: Option<AgentKind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(found) = self.screen_at(frame) else {
            return;
        };
        let folder = screens::folder(&self.screens_dir(), &found.data.screen);
        let out = self.handoff_dir(&found.data.screen);
        let picture: Option<Arc<RenderImage>> = self.screen_picture(frame);
        let (name, title) = (found.data.screen.clone(), found.title.clone());
        let project = self.canvas_project();
        let written = cx.background_spawn(async move {
            let png = picture.as_deref().and_then(png_of);
            write_handoff(&folder, &name, &title, &out, png)
        });
        cx.spawn_in(window, async move |this, cx| {
            let handoff = written.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let said = match (handoff, kind) {
                    (Err(error), _) => error,
                    (Ok(handoff), None) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(handoff.prompt));
                        "Copied the prompt, with the screen's files beside it".to_string()
                    }
                    (Ok(handoff), Some(kind)) => match agent_command(kind, &handoff.prompt_file) {
                        Some(command) => {
                            let send = SendToAgent {
                                kind,
                                project,
                                command,
                            };
                            open_agent(send, window, cx)
                        }
                        None => format!("Sawit can't start {} with a prompt", kind.name()),
                    },
                };
                this.show_toast(said, cx);
            });
        })
        .detach();
    }

    /// The screen whose frame is `frame`, as [`Self::selected_screen`] gives one.
    fn screen_at(&self, frame: NodeId) -> Option<super::Selected> {
        let ds = self.board()?.session.ds().ok()?;
        let node = ds.doc.node(frame)?;
        Some(super::Selected {
            frame,
            data: screens::screen_of(node)?,
            title: node.name.clone(),
        })
    }
}

/// Opens `send`'s agent in a new terminal tab of the window's workspace; what to say of it.
fn open_agent(send: SendToAgent, window: &mut Window, cx: &mut App) -> String {
    let Some(workspace) = crate::cli::workspace_for(window.window_handle(), cx) else {
        return "The window's terminal couldn't be found".into();
    };
    let opened = workspace.update(cx, |ws, cx| {
        ws.run_agent_in_new_terminal(send.project, &send.command, window, cx)
    });
    match opened {
        Ok(()) => format!(
            "Opened {} in a new terminal with the screen",
            send.kind.name()
        ),
        Err(error) => error,
    }
}
