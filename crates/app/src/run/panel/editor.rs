//! The Run panel's form to add or edit a saved command: its name, one or more command lines, and
//! whether they run in order or all at once.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::scripts::{RunMode, SavedCommand, clean_commands};

use super::RunPanel;

/// The add / edit form for a saved command.
pub(super) struct Editor {
    /// The command being edited; `None` for a new one.
    id: Option<String>,
    name: Entity<InputState>,
    lines: Vec<Entity<InputState>>,
    mode: RunMode,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl RunPanel {
    pub(super) fn open_editor(
        &mut self,
        id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = id.as_ref().and_then(|id| {
            let root = self.root.as_ref()?;
            self.store.read(cx).saved.get(root, id).cloned()
        });
        let name = existing
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Name, e.g. Start everything")
                .default_value(name)
        });
        let mode = existing.as_ref().map(|c| c.mode).unwrap_or_default();
        let mut editor = Editor {
            id: existing.as_ref().map(|c| c.id.clone()),
            name: name.clone(),
            lines: Vec::new(),
            mode,
            error: None,
            _subscriptions: vec![self.save_on_enter(&name, window, cx)],
        };
        let commands = existing.map(|c| c.commands).unwrap_or_default();
        for command in if commands.is_empty() {
            vec![String::new()]
        } else {
            commands
        } {
            self.push_line(&mut editor, command, window, cx);
        }
        name.update(cx, |s, cx| s.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    fn save_on_enter(
        &self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.save_editor(window, cx);
            }
        })
    }

    fn push_line(
        &self,
        editor: &mut Editor,
        command: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let n = editor.lines.len() + 1;
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if n == 1 {
                    "Command, e.g. pnpm dev".to_string()
                } else {
                    format!("Command {n}")
                })
                .default_value(command)
        });
        editor
            ._subscriptions
            .push(self.save_on_enter(&input, window, cx));
        editor.lines.push(input.clone());
        input
    }

    fn add_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut editor) = self.editor.take() else {
            return;
        };
        let input = self.push_line(&mut editor, String::new(), window, cx);
        input.update(cx, |s, cx| s.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    fn save_editor(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(root), Some(editor)) = (self.root.clone(), self.editor.as_mut()) else {
            return;
        };
        let name = editor.name.read(cx).value().trim().to_string();
        let commands = clean_commands(
            editor
                .lines
                .iter()
                .map(|l| l.read(cx).value().to_string())
                .collect(),
        );
        if commands.is_empty() {
            editor.error = Some("Add at least one command.".into());
            cx.notify();
            return;
        }
        let name = if name.is_empty() {
            commands[0].clone()
        } else {
            name
        };
        let mut command = SavedCommand::new(&name, commands, editor.mode);
        if let Some(id) = &editor.id {
            command.id = id.clone();
        }
        self.store.update(cx, |s, cx| s.upsert(&root, command, cx));
        self.editor = None;
        cx.notify();
    }

    fn delete_saved(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.store.update(cx, |s, cx| s.remove(&root, id, cx));
        self.editor = None;
        cx.notify();
    }

    pub(super) fn render_editor(&self, editor: &Editor, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let lines = editor.lines.len();
        let mode = editor.mode;
        v_flex()
            .m_2()
            .p_2()
            .gap_1p5()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(div().text_xs().font_semibold().text_color(muted).child(
                if editor.id.is_some() {
                    "EDIT COMMAND"
                } else {
                    "NEW COMMAND"
                },
            ))
            .child(Input::new(&editor.name).small())
            .children(editor.lines.iter().enumerate().map(|(i, line)| {
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .flex_none()
                            .w(px(14.))
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{}", i + 1)),
                    )
                    .child(div().flex_1().min_w_0().child(Input::new(line).small()))
                    .child(
                        Button::new(("run-line-remove", i))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip("Remove This Command")
                            .disabled(lines == 1)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(editor) = this.editor.as_mut()
                                    && editor.lines.len() > 1
                                {
                                    editor.lines.remove(i);
                                    cx.notify();
                                }
                            })),
                    )
            }))
            .child(
                Button::new("run-line-add")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .label("Add Another Command")
                    .on_click(cx.listener(|this, _, window, cx| this.add_line(window, cx))),
            )
            .when(lines > 1, |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .child(div().text_xs().text_color(muted).child("Run"))
                        .child(
                            Button::new("run-mode-sequence")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::ListOrdered)
                                .label("In Order")
                                .selected(mode == RunMode::Sequence)
                                .tooltip("One after another in one terminal (stops if one fails)")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(e) = this.editor.as_mut() {
                                        e.mode = RunMode::Sequence;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("run-mode-parallel")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Layers)
                                .label("All at Once")
                                .selected(mode == RunMode::Parallel)
                                .tooltip("At the same time, each in its own terminal")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(e) = this.editor.as_mut() {
                                        e.mode = RunMode::Parallel;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(editor.error.clone(), |el, e| {
                el.child(div().text_xs().text_color(cx.theme().danger).child(e))
            })
            .child(
                h_flex()
                    .gap_1()
                    .when_some(editor.id.clone(), |el, id| {
                        el.child(
                            Button::new("run-editor-delete")
                                .ghost()
                                .danger()
                                .xsmall()
                                .icon(Lucide::Trash)
                                .label("Delete")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.delete_saved(&id, cx)),
                                ),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("run-editor-cancel")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.editor = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("run-editor-save")
                            .primary()
                            .xsmall()
                            .label("Save")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_editor(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
