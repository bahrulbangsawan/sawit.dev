//! The panel and its sections: where it shows and how it goes, the Auto layout and Appearance
//! sections' edits, the header, and the icons and pictures it shows.

use figcraft_doc::NodeId;
use gpui_kit::{AssetSource as _, TestAppContext, VisualTestContext};
use sawit_board::design::layout::align_cell;
use sawit_board::design::{DesignSelection, Mixed};
use serde_json::json;

use super::super::{ICONS, NOTHING_SELECTED, WIDTH, appearance, header_text, position, typography};
use super::{
    Design, Scratch, click, design_file, drawn, edit_text, editing, field_text, frame_with_a_child,
    gesture, open, open_in_window, panel_value, rectangle, select, undo_steps,
};
use crate::board::{layers_panel, top_bar};

/// Ruling P5-C18, and the defaults of spec D11: the panel shows beside a design file, goes
/// with the UI, and comes back with it.
#[gpui_kit::test]
fn the_design_panel_goes_with_the_ui_and_comes_back(cx: &mut TestAppContext) {
    let tmp = Scratch::new("panel-ui");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let shown = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.show_design);
    let canvas_width =
        |cx: &VisualTestContext| view.read_with(cx, |view, _| f32::from(view.bounds.size.width));
    assert!(shown(&cx));
    assert!(drawn(&mut cx, "board-design-panel"));
    // The canvas is the window less both panels, which a design file opens with.
    let beside_panels = 900. - WIDTH - layers_panel::WIDTH;
    assert_eq!(canvas_width(&cx), beside_panels);
    let toggle_ui = |cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| view.choose("view.toggleUi", json!({}), cx));
        cx.run_until_parked();
    };
    toggle_ui(&mut cx);
    assert!(!drawn(&mut cx, "board-design-panel"));
    assert!(shown(&cx));
    assert_eq!(canvas_width(&cx), 900.);
    toggle_ui(&mut cx);
    assert!(drawn(&mut cx, "board-design-panel"));
    assert_eq!(canvas_width(&cx), beside_panels);
}

/// `view.design` shows and hides the panel: from an agent's command, and from the top bar's
/// toggle. A board opens without it (spec D11).
#[gpui_kit::test]
fn view_design_shows_and_hides_the_panel(cx: &mut TestAppContext) {
    let tmp = Scratch::new("panel-toggle");
    let (view, mut cx) = open_in_window(cx, &tmp.0.join("canvas.sawit"), &[]);
    assert!(!view.read_with(&cx, |view, _| view.show_design));
    assert!(!drawn(&mut cx, "board-design-panel"));
    let op = json!({"op": "command", "command": "view.design"});
    let reply = view.update(&mut cx, |view, cx| view.run_op(&op, cx));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.run_until_parked();
    assert!(drawn(&mut cx, "board-design-panel"));
    // With nothing selected, it says so, and has no field.
    assert!(drawn(&mut cx, "design-nothing-selected"));
    assert!(!drawn(&mut cx, "design-section-position"));
    click(&mut cx, "board-design");
    assert!(!drawn(&mut cx, "board-design-panel"));
    click(&mut cx, "board-design");
    assert!(drawn(&mut cx, "board-design-panel"));
    assert_eq!(NOTHING_SELECTED, "Select a layer to see its design.");
}

/// Ruling P5.T3 (carried): one edit aligns frames that run both ways, each along its own
/// direction, and one undo puts both back.
#[gpui_kit::test]
fn aligning_frames_that_run_both_ways_is_one_undo_step(cx: &mut TestAppContext) {
    let tmp = Scratch::new("align-both-ways");
    let path = tmp.0.join("design.sawit");
    let mut design = Design::new();
    let (across, _) = frame_with_a_child(&mut design, 0.0);
    let (down, _) = frame_with_a_child(&mut design, 400.0);
    design.set(&[across], json!({"layout": "horizontal"}));
    design.set(&[down], json!({"layout": "vertical"}));
    design.save(&path);
    let ids = [across, down];
    let view = open(cx, &path, &ids);
    let aligned = |cx: &TestAppContext| {
        [across, down].map(|frame| {
            let main = panel_value(&view, &[frame], "mainAlign", cx);
            let cross = panel_value(&view, &[frame], "crossAlign", cx);
            (main, cross)
        })
    };
    let before = aligned(cx);
    let steps = undo_steps(&view, cx);
    let sel = view.read_with(cx, |view, _| {
        DesignSelection::read(&view.board().unwrap().doc().unwrap(), &ids)
    });
    view.update(cx, |view, cx| view.apply_edit(align_cell(&sel, 0, 2), cx));
    assert_eq!(view.read_with(cx, |view, _| view.toast.clone()), None);
    let same = |align: &str| Mixed::Same(json!(align));
    // The top right cell: along the columns for the horizontal frame, down the rows for the
    // vertical one.
    assert_eq!(
        aligned(cx),
        [(same("max"), same("min")), (same("min"), same("max"))]
    );
    assert_eq!(undo_steps(&view, cx), steps + 1);
    view.update(cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    assert_eq!(aligned(cx), before);
}

/// The Auto layout section: adding keeps the selection, the direction sets layout and wrap
/// together, a cell of the grid aligns, sizing comes from its menu, and removing takes the
/// layout off.
#[gpui_kit::test]
fn the_auto_layout_section_edits_the_frames(cx: &mut TestAppContext) {
    let tmp = Scratch::new("auto-layout");
    let path = tmp.0.join("design.sawit");
    let mut design = Design::new();
    let (plain, _) = frame_with_a_child(&mut design, 0.0);
    let (laid_out, _) = frame_with_a_child(&mut design, 400.0);
    design.set(&[laid_out], json!({"layout": "horizontal"}));
    design.save(&path);
    let ids = [plain, laid_out];
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    let value = |key: &str, cx: &VisualTestContext| panel_value(&view, &ids, key, cx);
    assert_eq!(value("layout", &cx), Mixed::Mixed);
    assert!(drawn(&mut cx, "design-layout-remove"));
    click(&mut cx, "design-layout-add");
    let added = panel_value(&view, &[plain], "layout", &cx);
    assert!(matches!(added, Mixed::Same(_)), "{added:?}");
    let selected = view.read_with(&cx, |view, _| view.board().unwrap().session.selection());
    assert_eq!(selected, ids);
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert!(!drawn(&mut cx, "design-layout-add"));

    click(&mut cx, "design-layout-vertical");
    assert_eq!(value("layout", &cx), Mixed::Same(json!("vertical")));
    click(&mut cx, "design-layout-wrap");
    assert_eq!(value("layout", &cx), Mixed::Same(json!("horizontal")));
    assert_eq!(value("wrap", &cx), Mixed::Same(json!(true)));
    click(&mut cx, "design-layout-vertical");
    assert_eq!(value("wrap", &cx), Mixed::Same(json!(false)));
    assert_eq!(undo_steps(&view, &cx), steps + 4);

    // Vertical: the bottom middle cell is the end of the column, centred across it.
    click(&mut cx, "design-align-cell-2-1");
    assert_eq!(value("mainAlign", &cx), Mixed::Same(json!("max")));
    assert_eq!(value("crossAlign", &cx), Mixed::Same(json!("center")));

    click(&mut cx, "design-sizingH");
    click(&mut cx, "design-sizingH-hug");
    assert_eq!(value("sizingH", &cx), Mixed::Same(json!("hug")));

    // Each side's padding on its own.
    click(&mut cx, "design-padding-sides");
    click(&mut cx, "design-field-paddingTop");
    cx.simulate_keystrokes("1 2 enter");
    assert_eq!(value("paddingTop", &cx), Mixed::Same(json!(12.0)));
    assert_eq!(field_text(&view, "padding", &cx), "Mixed");

    click(&mut cx, "design-layout-remove");
    assert_eq!(value("layout", &cx), Mixed::Absent);
    assert!(drawn(&mut cx, "design-layout-add"));
    assert!(!drawn(&mut cx, "design-align-grid"));
}

/// CANVAS-UI-5, ruling R-TEXTEDIT from the Auto layout section: Add auto layout while a text
/// is edited, with a frame among the selection, ends the editing first, as one undo step, and
/// is a step of its own. Run inside the open edit, it made no undo step, and the edit stayed
/// open over a selection it no longer had.
#[gpui_kit::test]
fn add_auto_layout_ends_the_text_being_edited_first(cx: &mut TestAppContext) {
    let tmp = Scratch::new("auto-layout-typing");
    let path = tmp.0.join("design.sawit");
    let mut design = Design::new();
    let (frame, _) = frame_with_a_child(&mut design, 0.0);
    let text = design.make(json!({"type": "text", "text": "Hi", "x": 400, "y": 0}));
    design.save(&path);
    let (view, mut cx) = open_in_window(cx, &path, &[text]);
    let steps = undo_steps(&view, &cx);
    edit_text(&view, text, &mut cx);
    cx.simulate_input("!");
    // The frame joins the selection while the text is still edited.
    select(&view, &[frame], &mut cx);
    assert!(editing(&view, &cx));
    click(&mut cx, "design-layout-add");
    assert!(!editing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    let added = panel_value(&view, &[frame], "layout", &cx);
    assert!(matches!(added, Mixed::Same(_)), "{added:?}");
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    let toast = view.read_with(&cx, |view, _| view.toast.clone());
    assert_eq!(toast, None);
}

/// The Appearance section: the eye (ruling P5-C17), each corner's radius on its own, and the
/// blend mode.
#[gpui_kit::test]
fn the_appearance_section_edits_the_layers(cx: &mut TestAppContext) {
    let tmp = Scratch::new("appearance");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0), rectangle(150.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let value = |key: &str, cx: &VisualTestContext| panel_value(&view, &ids, key, cx);
    click(&mut cx, "design-visible");
    assert_eq!(value("visible", &cx), Mixed::Same(json!(false)));
    click(&mut cx, "design-visible");
    assert_eq!(value("visible", &cx), Mixed::Same(json!(true)));
    // One hidden and one shown: the eye shows all.
    view.update(&mut cx, |view, cx| {
        let hide = json!({"ids": [ids[0]], "props": {"visible": false}});
        view.choose("node.set", hide, cx);
    });
    cx.run_until_parked();
    assert_eq!(value("visible", &cx), Mixed::Mixed);
    click(&mut cx, "design-visible");
    assert_eq!(value("visible", &cx), Mixed::Same(json!(true)));

    click(&mut cx, "design-corners");
    click(&mut cx, "design-field-topLeftRadius");
    cx.simulate_keystrokes("8 enter");
    assert_eq!(value("topLeftRadius", &cx), Mixed::Same(json!(8.0)));
    assert_eq!(value("topRightRadius", &cx), Mixed::Same(json!(0.0)));
    assert_eq!(field_text(&view, "cornerRadius", &cx), "Mixed");

    click(&mut cx, "design-blend");
    click(&mut cx, "design-blend-Multiply");
    assert_eq!(value("blend", &cx), Mixed::Same(json!("multiply")));
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

#[test]
fn the_eye_shows_what_a_click_does() {
    let (shown, _, hides) = appearance::visibility(&Mixed::Same(json!(true)));
    assert_eq!((shown, hides), (appearance::SHOWN_ICON, false));
    let (hidden, _, shows) = appearance::visibility(&Mixed::Same(json!(false)));
    assert_eq!((hidden, shows), (appearance::HIDDEN_ICON, true));
    let (mixed, _, shows_all) = appearance::visibility(&Mixed::Mixed);
    assert_eq!((mixed, shows_all), (appearance::MIXED_SHOWN_ICON, true));
}

/// Ruling P5-C16: the header is the kind and the name of one layer, or the count of several.
#[test]
fn the_header_names_the_kind_and_the_layer() {
    let mut design = Design::new();
    let card = design.make(json!({"type": "rectangle", "name": "Card", "width": 10, "height": 10}));
    let other = design.make(rectangle(50.0));
    let doc = design.0.ds().unwrap().doc.clone();
    let header = |ids: &[NodeId]| header_text(&DesignSelection::read(&doc, ids));
    assert_eq!(header(&[card]), ("Rectangle".into(), Some("Card".into())));
    assert_eq!(header(&[card, other]), ("2 layers".into(), None));
}

/// Ruling P5-C11: every icon the panel and its toggle show is one Sawit embeds.
#[test]
fn every_icon_of_the_design_panel_is_embedded() {
    let icons = ICONS.iter().copied().chain(position::align_icons());
    let icons = icons.chain(typography::icons());
    for icon in icons.chain([top_bar::DESIGN_ICON]) {
        let path = icon.path();
        let svg = crate::assets::AppAssets.load(&path).unwrap();
        assert!(svg.is_some(), "{path} isn't embedded");
    }
}

/// Every choice of the panel's menus has a picture Sawit embeds, so that a choice added without
/// one fails here: the blend modes, the stroke positions, the sizings of both axes, the effect
/// kinds and the export formats. The scales ("2×") and the font's styles ("Bold") are words
/// that are their own picture, and have none.
#[test]
fn every_choice_of_the_design_panels_menus_has_a_picture() {
    use figcraft_doc::color::BlendMode;
    use gpui_kit::assets::IconNamed as _;

    use super::super::{effects, export, layout, paints};
    use crate::board::option_icons::{self, OptionIcon};

    let embedded = |icon: Option<OptionIcon>, what: &str| {
        let icon = icon.unwrap_or_else(|| panic!("{what} has no picture"));
        let path = icon.glyph.path();
        let svg = crate::assets::AppAssets.load(&path).unwrap();
        assert!(svg.is_some(), "{what}: {path} isn't embedded");
    };
    for mode in BlendMode::ALL {
        embedded(Some(option_icons::blend(mode)), mode.label());
    }
    for (align, name) in paints::STROKE_ALIGNS {
        embedded(option_icons::stroke_align(align), name);
    }
    for (sizing, _, name) in layout::SIZINGS {
        embedded(option_icons::sizing(sizing, false), name);
        embedded(option_icons::sizing(sizing, true), name);
    }
    for (kind, name) in effects::KINDS {
        embedded(option_icons::effect(kind), name);
    }
    for (format, name) in export::FORMATS {
        embedded(option_icons::export_format(format), name);
    }
}
