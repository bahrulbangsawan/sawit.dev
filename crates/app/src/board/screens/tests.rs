//! Send to Agent's command (its folder and prompt: `sawit_board::handoff`'s tests), Preview's sizes, a texture's PNG, and which screens a
//! change on disk touches.

use std::path::{Path, PathBuf};

use gpui_kit::RenderImage;
use sawit_core::terminal::AgentKind;

use super::handoff::agent_command;
use super::pictures::png_of;
use super::preview::{BAR, BEZEL, PAD, Preview, Size, Zoom, custom_size};
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
    let mut p = Preview::new(figcraft_doc::NodeId(1), "Sign in".into(), (390, 844), None);
    p.size = size;
    p.rotated = rotated;
    p
}

/// Preview lays the page out at the size it was made at, the window's, a device's or a custom
/// one, on its side when turned, scaled down to fit under its bar, never up.
#[test]
fn preview_sizes_fit_under_its_bar() {
    let canvas = (1200.0, 900.0);
    let area = (1200.0 - 2.0 * PAD, 900.0 - BAR - 2.0 * PAD);
    assert_eq!(preview(Size::Made, false).css_size(area), (390, 844));
    assert_eq!(preview(Size::Made, true).css_size(area), (844, 390));
    assert_eq!(
        preview(Size::Window, true).css_size(area),
        (area.0 as u32, area.1 as u32)
    );
    assert_eq!(
        preview(Size::Custom(800, 600), true).css_size(area),
        (600, 800)
    );
    let desktop = sawit_core::browser::DEVICES
        .iter()
        .position(|d| d.name == "Desktop")
        .unwrap();
    assert_eq!(
        preview(Size::Device(desktop), false).css_size(area),
        (1440, 900)
    );
    let (frame, scale) = preview(Size::Made, false).place((1200.0, 1100.0));
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

/// 100% shows one CSS px a point when the width fits, the room's height for a taller page; a
/// device frame fits phones and tablets only, and takes its bezel from the room.
#[test]
fn preview_zoom_and_device_frame() {
    let canvas = (1200.0, 900.0);
    let mut phone = preview(Size::Made, false);
    phone.zoom = Zoom::Actual;
    assert!(phone.actual_fits(canvas));
    let (frame, scale) = phone.place(canvas);
    assert_eq!((frame.width, scale), (390.0, 1.0));
    assert!(frame.height <= 900.0 - BAR - 2.0 * PAD);
    let desktop = sawit_core::browser::DEVICES
        .iter()
        .position(|d| d.name == "Desktop")
        .unwrap();
    let mut wide = preview(Size::Device(desktop), false);
    wide.zoom = Zoom::Actual;
    assert!(!wide.actual_fits(canvas), "1440 doesn't fit 1200");
    assert!(wide.place(canvas).1 < 1.0, "it fits instead");
    assert!(!wide.frame_fits());
    phone.framed = true;
    phone.zoom = Zoom::Fit;
    assert!(phone.frame_shown());
    let (framed, _) = phone.place(canvas);
    assert!(framed.y >= BAR + PAD + BEZEL);
    assert!(framed.height <= 900.0 - BAR - 2.0 * PAD - 2.0 * BEZEL);
    assert_eq!(custom_size(Some(1024), Some(768)), Ok((1024, 768)));
    assert!(custom_size(Some(10), Some(768)).is_err());
    assert!(custom_size(None, Some(768)).is_err());
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

// ---- B13: the screen, artboard and flow bars, and Send to Agent's menu -----------------------

mod bars {
    use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
    use gpui_kit::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, WeakEntity, px};
    use sawit_core::terminal::AgentKind;
    use serde_json::{Value, json};

    use crate::board::BoardView;
    use crate::board::screens::bar::{AGENTS_HEADER, LOOKING, NONE_FOUND, agents_menu};
    use crate::board::tests::{Scratch, chrome_canvas_sized, run_op};

    /// The bar's padding and border at each end.
    const ENDS: f32 = 4.;

    fn frame(view: &Entity<BoardView>, x: i32, name: &str, cx: &mut VisualTestContext) -> Value {
        let op = json!({
            "op": "command", "command": "frame.create",
            "params": {"x": x, "y": 0, "width": 120, "height": 160, "name": name,
                       "props": {"fill": "#ffffff"}},
        });
        let done = run_op(view, op, cx);
        done["result"]["id"].clone()
    }

    fn select(view: &Entity<BoardView>, ids: Vec<Value>, cx: &mut VisualTestContext) {
        run_op(
            view,
            json!({"op": "command", "command": "select.set", "params": {"ids": ids}}),
            cx,
        );
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// The bar `id`'s bounds and its buttons' (`buttons`, in order).
    fn bar(
        cx: &mut VisualTestContext,
        id: &'static str,
        buttons: &[&'static str],
    ) -> (Bounds<Pixels>, Vec<Bounds<Pixels>>) {
        let bar = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("{id} isn't drawn"));
        let buttons = buttons
            .iter()
            .map(|b: &&'static str| {
                cx.debug_bounds(b)
                    .unwrap_or_else(|| panic!("{b} isn't drawn"))
            })
            .collect();
        (bar, buttons)
    }

    /// No blank space at either end: the first button starts 4 px in, the last ends 4 px in.
    fn fits(bar: Bounds<Pixels>, buttons: &[Bounds<Pixels>]) {
        let first = buttons.first().unwrap();
        let last = buttons.last().unwrap();
        assert!(
            (first.left() - bar.left() - px(ENDS)).abs() < px(0.5),
            "{bar:?} {first:?}"
        );
        assert!(
            (bar.right() - last.right() - px(ENDS)).abs() < px(0.5),
            "{bar:?} {last:?}"
        );
    }

    /// The artboard bar and the flow bar are as wide as their buttons; in a narrow window the
    /// artboard bar's buttons show their icons alone, 28 px square, and still fit.
    #[gpui_kit::test]
    fn the_frame_bars_fit_their_buttons(cx: &mut TestAppContext) {
        let tmp = Scratch::new("b13-frame-bars");
        let (view, mut cx) = chrome_canvas_sized(cx, &tmp.0.join("canvas.sawit"), (1000., 700.));
        let a = frame(&view, -200, "Settings", &mut cx);
        let b = frame(&view, 0, "Profile", &mut cx);
        select(&view, vec![a.clone()], &mut cx);
        let (bounds, buttons) = bar(
            &mut cx,
            "frame-bar",
            &["frame-bar-export", "frame-bar-send"],
        );
        fits(bounds, &buttons);
        assert_eq!(bounds.size.height, px(36.));
        assert!(
            buttons[1].size.width > px(60.),
            "Send to Agent shows its words"
        );
        select(&view, vec![a, b], &mut cx);
        let (bounds, buttons) = bar(&mut cx, "frame-bar", &["frame-bar-send"]);
        fits(bounds, &buttons);
        assert!(
            cx.debug_bounds("frame-bar-export").is_none(),
            "a flow has no Export"
        );
    }

    #[gpui_kit::test]
    fn the_frame_bar_collapses_to_icons_on_a_narrow_canvas(cx: &mut TestAppContext) {
        let tmp = Scratch::new("b13-narrow");
        let (view, mut cx) = chrome_canvas_sized(cx, &tmp.0.join("canvas.sawit"), (150., 700.));
        let a = frame(&view, -20, "Settings", &mut cx);
        select(&view, vec![a], &mut cx);
        let (bounds, buttons) = bar(
            &mut cx,
            "frame-bar",
            &["frame-bar-export", "frame-bar-send"],
        );
        fits(bounds, &buttons);
        for b in &buttons {
            assert_eq!(b.size.width, px(28.), "icons alone: {b:?}");
        }
    }

    fn items(
        agents: Option<Vec<AgentKind>>,
        view: WeakEntity<BoardView>,
        cx: &mut VisualTestContext,
    ) -> Vec<(String, bool, bool)> {
        let menu = cx.update(|window, cx| {
            PopupMenu::build(window, cx, move |menu, _, _| {
                agents_menu(
                    menu,
                    &[],
                    &agents,
                    Some("~/p/.nyawit/handoff/x".into()),
                    &view,
                )
            })
        });
        menu.read_with(cx, |menu, _| {
            menu.items()
                .iter()
                .filter_map(|item| match item {
                    PopupMenuItem::Item {
                        label, disabled, ..
                    } => Some((label.to_string(), item.has_picture(), *disabled)),
                    PopupMenuItem::Label(label) => Some((format!("# {label}"), true, true)),
                    _ => None,
                })
                .collect()
        })
    }

    /// Send to Agent's menu: a header and each agent with its logo; a spinner row while looking;
    /// with none, a way to get one. Copy Prompt and Reveal in Finder, with icons, always.
    #[gpui_kit::test]
    fn the_send_menu_lists_agents_with_logos(cx: &mut TestAppContext) {
        let tmp = Scratch::new("b13-send-menu");
        let (view, mut cx) = chrome_canvas_sized(cx, &tmp.0.join("canvas.sawit"), (800., 600.));
        let weak = view.downgrade();
        let found = items(
            Some(vec![AgentKind::ClaudeCode, AgentKind::Codex]),
            weak.clone(),
            &mut cx,
        );
        let names: Vec<&str> = found.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                &format!("# {AGENTS_HEADER}") as &str,
                "Claude Code",
                "Codex",
                "Copy Prompt",
                "Reveal in Finder"
            ]
        );
        assert!(found.iter().all(|(_, pictured, _)| *pictured), "{found:?}");
        let looking = items(None, weak.clone(), &mut cx);
        assert_eq!(looking[0], (LOOKING.to_string(), true, true));
        let none = items(Some(vec![]), weak, &mut cx);
        let names: Vec<&str> = none.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                NONE_FOUND,
                "Get a Coding Agent…",
                "Copy Prompt",
                "Reveal in Finder"
            ]
        );
        assert!(!none[2].2, "Copy Prompt works with no agent");
    }
}
