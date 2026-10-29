//! Buttons on code blocks in rendered previews (Markdown, HTML, changelog):
//! Copy on every block, plus Run on shell blocks (`bash`, `zsh`, …), which
//! types the commands into the bottom terminal of the same window.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::text::CodeBlock;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::{is_shell_lang, shell_commands};

use crate::cli::workspace_for;

/// For `TextView::code_block_actions`.
pub fn code_block_actions(block: &CodeBlock, _: &mut Window, cx: &mut App) -> AnyElement {
    let code = block.code();
    let lang = block.lang().filter(|l| !l.is_empty());
    let shell = lang.as_deref().is_some_and(is_shell_lang);
    h_flex()
        .gap_0p5()
        .items_center()
        .when_some(lang, |el, lang| {
            el.child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(lang),
            )
        })
        .when(shell, |el| {
            let commands = shell_commands(&code);
            el.child(
                Button::new("run")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Play)
                    .tooltip("Run in Terminal")
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(workspace) = workspace_for(window.window_handle(), cx) {
                            workspace.update(cx, |w, cx| w.run_in_terminal(&commands, window, cx));
                        }
                    }),
            )
        })
        .child(
            Button::new("copy")
                .ghost()
                .xsmall()
                .icon(Lucide::Copy)
                .tooltip("Copy")
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                }),
        )
        .into_any_element()
}
