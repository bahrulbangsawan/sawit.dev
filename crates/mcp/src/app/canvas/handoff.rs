//! canvas_handoff: Send to Agent from MCP. The same folder the app writes (`.nyawit/handoff/
//! <name>/`, [`sawit_board::handoff`]) for a screen, an artboard or a flow of them, answered with
//! its files and prompt, so an agent can hand a design to another agent, or to itself in a new
//! session. With `agent`, and the app running, that agent's CLI opens in a new terminal tab in
//! the project with the prompt as its first message; the tab doesn't take the keyboard.

use sawit_board::{screens, store};
use sawit_core::app_control::Command;
use sawit_core::terminal::AgentKind;
use serde_json::{Value, json};

use super::CanvasHandoffArgs;
use super::files::canvas_file;
use crate::app::pretty;
use crate::{Sawit, fail, ok};

impl Sawit {
    pub(super) async fn canvas_handoff_tool(
        &self,
        a: CanvasHandoffArgs,
    ) -> rmcp::model::CallToolResult {
        let agent = match a.agent.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            None => None,
            Some(wanted) => {
                let lower = wanted.to_lowercase();
                let found = AgentKind::all().find(|k| {
                    k.id() == lower || k.name().to_lowercase() == lower || k.executable() == lower
                });
                match found {
                    Some(kind) => Some(kind),
                    None => {
                        let known: Vec<&str> = AgentKind::all()
                            .filter(|k| sawit_core::terminal::prompt_args(k.id(), false).is_some())
                            .map(|k| k.id())
                            .collect();
                        return fail(format!("Unknown agent “{wanted}”: {}", known.join(", ")));
                    }
                }
            }
        };
        let file = match canvas_file(a.file.as_deref()) {
            Ok(file) => file,
            Err(e) => return fail(e),
        };
        let canvas = match file {
            Some(file) => file,
            None => match self.canvas_board_file(a.project.as_deref()) {
                Ok(file) => file,
                Err(e) => return fail(e),
            },
        };
        let dir = screens::screens_dir(&store::bound_file(&canvas));
        let ids = match a.id {
            Some(Value::Array(ids)) => Value::Array(ids),
            Some(Value::String(id)) => json!([id]),
            Some(other) => json!([other.to_string()]),
            None => json!([]),
        };
        let op =
            json!({"op": "handoff", "ids": ids, "work": a.work, "screen": a.screen, "dir": dir});
        let named =
            !ids.as_array().is_none_or(Vec::is_empty) || a.work.is_some() || a.screen.is_some();
        if !named {
            return fail(
                "canvas_handoff: name what to hand off: `id` (one or more frames), `work` or `screen`",
            );
        }
        let mut done = match self.canvas(a.project.clone(), a.file, op).await {
            Ok(done) => done,
            Err(e) => return fail(e),
        };
        if let Some(kind) = agent {
            let prompt = done["prompt_file"].as_str().unwrap_or_default();
            let Some(command) =
                sawit_core::terminal::start_with_prompt(kind, std::path::Path::new(prompt), false)
            else {
                return fail(format!("Sawit can't start {} with a prompt", kind.name()));
            };
            done["command"] = json!(command);
            let cwd = crate::agent_project(a.project.as_deref());
            let run = Command::TerminalRun {
                command: command.clone(),
                cwd,
                new_tab: true,
            };
            match self.app_call(a.project, run).await {
                Ok(started) => done["terminal"] = started,
                Err(e) => {
                    done["note"] = json!(format!(
                        "{} wasn't opened ({e}): run `command` in the project to start it",
                        kind.name()
                    ));
                }
            }
        }
        ok(pretty(&done))
    }
}
