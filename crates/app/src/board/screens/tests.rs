//! Send to Agent's command (its folder and prompt: `sawit_board::handoff`'s tests), Preview's sizes, a texture's PNG, and which screens a
//! change on disk touches.

use std::path::{Path, PathBuf};

use gpui_kit::RenderImage;
use sawit_core::terminal::AgentKind;

use super::handoff::agent_command;
use super::pictures::png_of;
use super::preview::{BAR, Preview, Size};
use crate::board::web::screens_of;

/// Send to Agent starts the agent's CLI with the prompt file as its first message: the command
/// it would run, checked without running it.
#[test]
fn send_to_agent_runs_the_agents_cli_with_the_prompt() {
    let file = Path::new("/p/.nyawit/handoff/sign-in/prompt.md");
    let read = "\"$(cat -- /p/.nyawit/handoff/sign-in/prompt.md)\"";
    assert_eq!(
        agent_command(AgentKind::ClaudeCode, file).unwrap(),
        format!("claude {read}")
    );
    assert_eq!(
        agent_command(AgentKind::Codex, file).unwrap(),
        format!("codex {read}")
    );
    assert_eq!(
        agent_command(AgentKind::GeminiCli, file).unwrap(),
        format!("gemini -i {read}")
    );
    assert_eq!(agent_command(AgentKind::Amp, file), None);
}

fn preview(size: Size, rotated: bool) -> Preview {
    Preview {
        frame: figcraft_doc::NodeId(1),
        title: "Sign in".into(),
        made: (390, 844),
        size,
        rotated,
        engine: None,
        placed: None,
    }
}

/// Preview lays the page out at the size it was made at, the window's, or a device's, on its
/// side when turned, scaled down to fit under its bar, never up.
#[test]
fn preview_sizes_fit_under_its_bar() {
    let canvas = (1200.0, 900.0);
    let area = (1200.0 - 32.0, 900.0 - BAR - 32.0);
    assert_eq!(preview(Size::Made, false).css_size(area), (390, 844));
    assert_eq!(preview(Size::Made, true).css_size(area), (844, 390));
    assert_eq!(preview(Size::Window, true).css_size(area), (1168, 824));
    let desktop = sawit_core::browser::DEVICES.len() - 1;
    assert_eq!(
        preview(Size::Device(desktop), false).css_size(area),
        (1440, 900)
    );
    let (frame, scale) = preview(Size::Made, false).place((1200.0, 1000.0));
    assert_eq!(
        (frame.width, frame.height, scale),
        (390.0, 844.0, 1.0),
        "room for it"
    );
    assert!(frame.y >= BAR, "under the bar");
    let (frame, scale) = preview(Size::Made, false).place(canvas);
    assert!(
        scale < 1.0 && frame.height <= area.1,
        "scaled down to fit, never up"
    );
    let (frame, scale) = preview(Size::Device(desktop), false).place(canvas);
    assert!(scale < 1.0 && frame.width <= area.0 && frame.height <= area.1);
}

/// A texture's BGRA bytes come out as a PNG of the same pixels.
#[test]
fn a_textures_bytes_make_a_png() {
    let mut bgra = image::RgbaImage::new(2, 1);
    // Blue, green, red, alpha: an opaque orange, and a half-covered white.
    bgra.put_pixel(0, 0, image::Rgba([0x33, 0x66, 0xFF, 0xFF]));
    bgra.put_pixel(1, 0, image::Rgba([0x80, 0x80, 0x80, 0x80]));
    let texture = RenderImage::new([image::Frame::new(bgra)]);
    let png = png_of(&texture).unwrap();
    let back = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(back.get_pixel(0, 0).0, [0xFF, 0x66, 0x33, 0xFF]);
    assert_eq!(back.get_pixel(1, 0).0, [0xFF, 0xFF, 0xFF, 0x80]);
}

/// A change on disk names the screen whose folder it is in; anything else in the screens folder
/// names none.
#[test]
fn changes_on_disk_name_their_screens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let paths = [
        root.join("sign-in/index.html"),
        root.join("sign-in/css/a.css"),
        root.join("pricing/img/x.png"),
        root.join(".DS_Store"),
        PathBuf::from("/elsewhere/sign-in/index.html"),
    ];
    assert_eq!(screens_of(dir.path(), &paths), ["pricing", "sign-in"]);
}
