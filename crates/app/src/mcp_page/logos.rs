//! Logos: each AI client's, and each catalog server's.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::mcp_catalog::Server;
use sawit_core::mcp_clients::Client;

/// The logo of a client: colour logos as images, one-colour ones tinted
/// with the theme, and a neutral glyph for clients without a free logo.
pub(crate) fn client_logo(client: Client, size: f32, cx: &App) -> AnyElement {
    let logo = match client {
        Client::ClaudeCode => Some(("claudecode-color", true)),
        Client::ClaudeDesktop => Some(("claude-color", true)),
        Client::Codex => Some(("codex-color", true)),
        Client::Gemini => Some(("geminicli-color", true)),
        Client::Qwen => Some(("qwen-color", true)),
        Client::Amp => Some(("amp-color", true)),
        Client::Kiro => Some(("kiro-color", true)),
        Client::Junie => Some(("junie-color", true)),
        Client::Antigravity => Some(("antigravity-color", true)),
        Client::Trae => Some(("trae-color", true)),
        Client::OpenCode => Some(("opencode", false)),
        Client::CopilotCli => Some(("githubcopilot", false)),
        Client::Cline => Some(("cline", false)),
        Client::Goose => Some(("goose", false)),
        Client::Cursor => Some(("cursor", false)),
        Client::Windsurf => Some(("windsurf", false)),
        Client::Zed => Some(("zed", false)),
        Client::Warp => Some(("warp", false)),
        Client::RooCode => Some(("roocode", false)),
        Client::KiloCode => Some(("kilocode", false)),
        Client::LmStudio => Some(("lmstudio", false)),
        _ => None,
    };
    logo_image(logo, Lucide::Bot, size, cx)
}

pub(super) fn server_logo(server: &Server, cx: &App) -> AnyElement {
    if server.id == sawit_core::mcp_clients::SERVER_NAME {
        return img(crate::assets::LOGO_MARK)
            .size(px(20.))
            .flex_none()
            .into_any_element();
    }
    let glyph = if server.is_remote() {
        Lucide::Globe
    } else {
        Lucide::SquareTerminal
    };
    logo_image(server.logo, glyph, 20., cx)
}

fn logo_image(logo: Option<(&str, bool)>, glyph: Lucide, size: f32, cx: &App) -> AnyElement {
    let Some((name, colour)) = logo else {
        // Same box as the logos, so names line up.
        return div()
            .size(px(size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(glyph)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .into_any_element();
    };
    let path: SharedString = format!("agent_logos/{name}.svg").into();
    if colour {
        img(path).size(px(size)).flex_none().into_any_element()
    } else {
        svg()
            .path(path)
            .size(px(size))
            .flex_none()
            .text_color(cx.theme().foreground)
            .into_any_element()
    }
}
