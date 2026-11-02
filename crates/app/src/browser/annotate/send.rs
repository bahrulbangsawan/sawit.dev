//! Where the notes go: the agents running in the terminals (looked for again while the panel is
//! open), the agent CLIs Open agent can start, and the send button that offers them.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::{AgentKind, PaneId};

use super::{AnnotateEvent, AnnotatePanel};
use crate::browser::agents::{self, AgentSource};
use crate::terminal::RunningAgent;

/// How often the open panel looks again for the agents running in the terminals.
const AGENT_POLL: Duration = Duration::from_secs(1);

/// How wide an agent menu may be: it opens over the panel, never over the page, whose webview
/// draws above it.
const MENU_WIDTH: f32 = 236.;

/// What the send button does, from the agents running and those installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SendAction {
    /// Sends to the agent at this index; with more than one running, a menu lists them all.
    Send { default: usize },
    /// Starts this agent CLI in a new terminal; with more than one installed, a menu lists them.
    Open { kind: AgentKind },
    /// No agent runs and none was found to start, or the panel is still looking.
    NoAgent { looking: bool },
}

impl AnnotatePanel {
    /// Where the agents running in the terminals are found. While the panel is open it looks
    /// again every [`AGENT_POLL`], as an agent can start or end in a terminal at any time.
    pub fn set_agent_source(&mut self, source: AgentSource, cx: &mut Context<Self>) {
        self.source = Some(source);
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENT_POLL).await;
                let alive = this.update(cx, |this, cx| {
                    if this.shown {
                        this.refresh_agents(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
        self.refresh_agents(cx);
        self.find_installed(cx);
    }

    /// The panel opened or closed. Opened, it looks at once for the agents running, and for the
    /// agent CLIs installed since it last looked.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.shown = shown;
        if shown && self.source.is_some() {
            self.refresh_agents(cx);
            self.find_installed(cx);
        }
    }

    /// Looks again for the agents running in the terminals. A remembered target whose agent has
    /// ended is forgotten.
    pub fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        if let Some(source) = self.source.clone() {
            let agents = source(cx);
            self.set_agents(agents, cx);
        }
    }

    /// The agents running in the terminals.
    pub(super) fn set_agents(&mut self, agents: Vec<RunningAgent>, cx: &mut Context<Self>) {
        if let Some(pane) = self.last_target
            && !agents.iter().any(|a| a.pane == pane)
        {
            self.last_target = None;
        }
        if !agents::same_agents(&agents, &self.agents) {
            self.agents = agents;
            cx.notify();
        }
    }

    /// The agent CLIs Open agent can start (for tests; the panel looks for them itself).
    #[cfg(test)]
    pub(super) fn set_installed(&mut self, installed: Vec<AgentKind>, cx: &mut Context<Self>) {
        self.installed = Some(installed);
        cx.notify();
    }

    /// Looks for the agent CLIs on this Mac, off the UI thread.
    fn find_installed(&mut self, cx: &mut Context<Self>) {
        let found = crate::rt::spawn_blocking(cx, agents::installed_agents);
        cx.spawn(async move |this, cx| {
            if let Ok(found) = found.await {
                let _ = this.update(cx, |this, cx| {
                    if this.installed.as_ref() != Some(&found) {
                        this.installed = Some(found);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// What the send button does now.
    pub(super) fn send_action(&self) -> SendAction {
        if let Some(default) = agents::default_target(&self.agents, self.last_target) {
            return SendAction::Send { default };
        }
        match &self.installed {
            None => SendAction::NoAgent { looking: true },
            Some(found) => match agents::default_open(found, self.last_kind) {
                Some(kind) => SendAction::Open { kind },
                None => SendAction::NoAgent { looking: false },
            },
        }
    }

    /// Sends the notes to the agent in `pane`, which is remembered as the target while it runs.
    pub(super) fn send_to(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.iter().find(|a| a.pane == pane).cloned() else {
            return;
        };
        self.last_target = Some(pane);
        self.last_kind = Some(agent.kind);
        cx.emit(AnnotateEvent::SendTo(agent));
    }

    /// Starts `kind` in a new terminal with the notes, and starts it again next time.
    fn open_agent(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.last_kind = Some(kind);
        cx.emit(AnnotateEvent::Open(kind));
    }

    /// Why the send button can't be used now, said under it: there are no notes, or no agent
    /// to send them to.
    pub(super) fn send_blocked(&self, has_rows: bool) -> Option<SharedString> {
        if !has_rows {
            return Some("Pick an element and write a note to send it to an agent.".into());
        }
        match self.send_action() {
            SendAction::NoAgent { looking: true } => Some("Looking for agent CLIs…".into()),
            SendAction::NoAgent { looking: false } => Some(
                "No agent is running in a terminal, and no agent CLI was found. Install Claude \
                 Code, Codex or Gemini CLI from Dev Tools."
                    .into(),
            ),
            _ => None,
        }
    }

    /// The send button: Send to the agent running in a terminal, with a menu of the others when
    /// there are several; else Open, which starts one with the notes, with a menu of the agent
    /// CLIs installed when there are several. Each shows the agent's logo.
    pub(super) fn send_control(
        &self,
        has_rows: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let tint = theme.primary_foreground;
        let menu_tint = theme.popover_foreground;
        let action = self.send_action();
        let this = cx.entity().downgrade();
        let (mode, control): (&'static str, AnyElement) = match action {
            SendAction::Send { default } => {
                let agent = self.agents[default].clone();
                let name = agents::target_name(&self.agents, default);
                let pane = agent.pane;
                let primary = Button::new("annotate-send-primary")
                    .small()
                    .primary()
                    .flex_1()
                    .child(logo_slot(agent.kind, tint, !has_rows, window, cx))
                    .child(button_text(format!("Send to {}", agent.kind.name())))
                    .accessibility_label(format!("Send the notes to {name}"))
                    .tooltip(format!(
                        "Paste the notes into the chat of {name} and send them"
                    ))
                    .disabled(!has_rows)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.send_to(pane, cx);
                    }));
                if self.agents.len() < 2 {
                    ("annotate-send-one", primary.w_full().into_any_element())
                } else {
                    let entries: Rc<Vec<(RunningAgent, String, bool)>> = Rc::new(
                        self.agents
                            .iter()
                            .enumerate()
                            .map(|(ix, a)| {
                                (
                                    a.clone(),
                                    agents::target_name(&self.agents, ix),
                                    ix == default,
                                )
                            })
                            .collect(),
                    );
                    let control = DropdownButton::new("annotate-send-split")
                        .w_full()
                        .small()
                        .primary()
                        .button(primary)
                        .disabled(!has_rows)
                        .menu_accessibility_label("Choose the agent to send the notes to")
                        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                            let mut menu = menu.max_w(px(MENU_WIDTH)).label("Send the notes to");
                            for (agent, name, ticked) in entries.iter().cloned() {
                                let this = this.clone();
                                let pane = agent.pane;
                                menu = menu.item(
                                    PopupMenuItem::element(move |window, cx| {
                                        menu_entry(agent.kind, name.clone(), menu_tint, window, cx)
                                    })
                                    .checked(ticked)
                                    .on_click(
                                        move |_, _, cx| {
                                            let _ = this.update(cx, |p, cx| p.send_to(pane, cx));
                                        },
                                    ),
                                );
                            }
                            menu
                        });
                    ("annotate-send-split", control.into_any_element())
                }
            }
            SendAction::Open { kind } => {
                let name = kind.name();
                let primary = Button::new("annotate-open-primary")
                    .small()
                    .primary()
                    .flex_1()
                    .child(logo_slot(kind, tint, !has_rows, window, cx))
                    .child(button_text(format!("Open {name}")))
                    .accessibility_label(format!("Open {name} in a new terminal with the notes"))
                    .tooltip(format!(
                        "No agent is running in a terminal: start {name} in a new one, with the \
                         notes as its first message"
                    ))
                    .disabled(!has_rows)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_agent(kind, cx);
                    }));
                let installed = self.installed.clone().unwrap_or_default();
                if installed.len() < 2 {
                    ("annotate-open-one", primary.w_full().into_any_element())
                } else {
                    let control = DropdownButton::new("annotate-open-split")
                        .w_full()
                        .small()
                        .primary()
                        .button(primary)
                        .disabled(!has_rows)
                        .menu_accessibility_label("Choose the agent to open")
                        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                            let mut menu = menu.max_w(px(MENU_WIDTH)).label("Open with the notes");
                            for other in installed.iter().copied() {
                                let this = this.clone();
                                menu = menu.item(
                                    PopupMenuItem::element(move |window, cx| {
                                        menu_entry(
                                            other,
                                            other.name().to_string(),
                                            menu_tint,
                                            window,
                                            cx,
                                        )
                                    })
                                    .checked(other == kind)
                                    .on_click(
                                        move |_, _, cx| {
                                            let _ =
                                                this.update(cx, |p, cx| p.open_agent(other, cx));
                                        },
                                    ),
                                );
                            }
                            menu
                        });
                    ("annotate-open-split", control.into_any_element())
                }
            }
            SendAction::NoAgent { .. } => (
                "annotate-send-none",
                Button::new("annotate-open-none")
                    .small()
                    .primary()
                    .w_full()
                    .icon(Lucide::Bot)
                    .label("Open agent")
                    .disabled(true)
                    .into_any_element(),
            ),
        };
        div()
            .debug_selector(|| "annotate-send".into())
            .flex_1()
            .min_w_0()
            .child(
                h_flex()
                    .debug_selector(move || mode.into())
                    .w_full()
                    .child(control),
            )
    }
}

/// An agent's logo in a button, 14 px, as the terminal draws it, faded with the button when it
/// is disabled; a one-colour logo in the button's `tint`.
fn logo_slot(kind: AgentKind, tint: Hsla, disabled: bool, window: &Window, cx: &App) -> Div {
    div()
        .flex_none()
        .when(disabled, |el| el.opacity(0.5))
        .child(crate::terminal::agent_logo(kind, px(14.), tint, window, cx))
}

/// A button's label beside its logo, cut short when the panel is narrow.
fn button_text(text: String) -> impl IntoElement {
    div().min_w_0().truncate().child(text)
}

/// A menu line: the agent's logo and what tells it apart.
fn menu_entry(kind: AgentKind, name: String, tint: Hsla, window: &Window, cx: &App) -> Div {
    h_flex()
        .gap_2()
        .min_w_0()
        .items_center()
        .child(crate::terminal::agent_logo(kind, px(16.), tint, window, cx))
        .child(div().min_w_0().truncate().child(name))
}
