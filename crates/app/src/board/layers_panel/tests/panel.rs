//! The panel in a window: where it shows and how it goes, the filter, a hover, the way a
//! selection opens in the tree, a drag with the mouse, and its icons.

use std::collections::HashSet;

use figcraft_doc::NodeId;
use gpui_kit::{
    AssetSource as _, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};
use sawit_board::layers::{DropPos, LayerIcon};
use serde_json::json;

use super::super::{ICONS, LayerDrag, NO_LAYERS, NO_MATCHES, WIDTH, layer_icon};
use super::{
    change, children, command, design_file, drawn, hovered, middle, open, open_in_window, row,
    selection, selector, shown_and_locked, tool,
};
use crate::board::tests::{Scratch, click, focus_canvas, run_op};
use crate::board::{design_panel, top_bar};

/// Spec D11: the panel shows at first beside a design file, not beside a board.
#[gpui_kit::test]
fn the_panel_shows_beside_a_design_file_and_not_a_board(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-defaults");
    let design = tmp.0.join("design.sawit");
    design_file(&design);
    let view = open(cx, &design);
    assert!(view.read_with(cx, |view, _| view.show_layers));
    let board = open(cx, &tmp.0.join("canvas.sawit"));
    assert!(!board.read_with(cx, |view, _| view.show_layers));
}

/// Rulings P4-C10 and P4-C16: `view.layers` (⌥1) and `view.leftSidebar` (⇧⌘\), from an
/// agent's command op, show and hide the panel at once.
#[gpui_kit::test]
fn view_layers_and_the_left_sidebar_show_and_hide_the_panel(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-op");
    let view = open(cx, &tmp.0.join("canvas.sawit"));
    let shown = |cx: &TestAppContext| view.read_with(cx, |view, _| view.show_layers);
    let reply = run_op(&view, command("view.layers"), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(shown(cx));
    run_op(&view, command("view.leftSidebar"), cx);
    assert!(!shown(cx));
    run_op(&view, command("view.leftSidebar"), cx);
    assert!(shown(cx));
    run_op(&view, command("view.layers"), cx);
    assert!(!shown(cx));
    assert!(view.read_with(cx, |view, _| {
        view.board().unwrap().session.ui_requests.is_empty()
    }));
}

/// Ruling P4-C8: the panel goes with the UI, and comes back with it. Ruling P4-C14: the canvas,
/// with its toolbar, is laid out beside it, not under it.
#[gpui_kit::test]
fn the_layers_panel_goes_with_the_ui_and_comes_back(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-ui");
    let path = tmp.0.join("design.sawit");
    design_file(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    let canvas = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| {
            let bounds = view.bounds;
            (f32::from(bounds.origin.x), f32::from(bounds.size.width))
        })
    };
    assert!(drawn(&mut cx, "board-layers-panel"));
    assert_eq!(canvas(&cx), (WIDTH, 1000. - WIDTH - design_panel::WIDTH));
    let toolbar = cx.debug_bounds("board-toolbar").unwrap();
    assert!(f32::from(toolbar.origin.x) > WIDTH, "{toolbar:?}");
    change(&view, &mut cx, |view, cx| {
        view.choose("view.toggleUi", json!({}), cx)
    });
    assert!(!drawn(&mut cx, "board-layers-panel"));
    assert!(view.read_with(&cx, |view, _| view.show_layers));
    assert_eq!(canvas(&cx), (0., 1000.));
    change(&view, &mut cx, |view, cx| {
        view.choose("view.toggleUi", json!({}), cx)
    });
    assert!(drawn(&mut cx, "board-layers-panel"));
    assert_eq!(canvas(&cx).0, WIDTH);
}

/// Ruling P4-C13: the top bar's toggle sits after the zoom menu, before the Design panel's,
/// and shows and hides the panel.
#[gpui_kit::test]
fn the_top_bar_toggle_shows_and_hides_the_panel(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-top-bar");
    let path = tmp.0.join("design.sawit");
    design_file(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    let left = |cx: &mut VisualTestContext, selector| {
        f32::from(cx.debug_bounds(selector).unwrap().origin.x)
    };
    let (zoom, toggle, design) = (
        left(&mut cx, "board-zoom"),
        left(&mut cx, "board-layers"),
        left(&mut cx, "board-design"),
    );
    assert!(zoom < toggle && toggle < design, "{zoom} {toggle} {design}");
    click(&mut cx, "board-layers");
    cx.run_until_parked();
    assert!(!view.read_with(&cx, |view, _| view.show_layers));
    assert!(!drawn(&mut cx, "board-layers-panel"));
    click(&mut cx, "board-layers");
    cx.run_until_parked();
    assert!(drawn(&mut cx, "board-layers-panel"));
}

/// Ruling P4-C5 and the filter ruling of P4.T1's review: a filter shows what it finds open
/// without adding to the layers the user opened, its arrows do nothing, a filter that finds
/// nothing says so, and clearing it gives the tree back as it was.
#[gpui_kit::test]
fn the_filter_leaves_the_open_layers_alone(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-filter");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    change(&view, &mut cx, |view, _| {
        view.expanded = HashSet::from([t.f])
    });
    let expanded = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.expanded.clone());
    assert!(drawn(&mut cx, row(t.a)) && !drawn(&mut cx, row(t.c)));

    change(&view, &mut cx, |view, _| view.layer_filter = "c".into());
    // C, under F and G, both open: A and B aren't listed.
    assert!(drawn(&mut cx, row(t.f)) && drawn(&mut cx, row(t.g)) && drawn(&mut cx, row(t.c)));
    assert!(!drawn(&mut cx, row(t.a)) && !drawn(&mut cx, row(t.b)));
    assert_eq!(expanded(&cx), HashSet::from([t.f]));
    // G's arrow does nothing under the filter.
    click(&mut cx, selector(format!("layers-arrow-{}", t.g.0)));
    cx.run_until_parked();
    assert_eq!(expanded(&cx), HashSet::from([t.f]));
    assert!(drawn(&mut cx, row(t.c)));

    // Nothing found: the list says so, and shows no row of the tree before it.
    change(&view, &mut cx, |view, _| view.layer_filter = "zzz".into());
    assert!(drawn(&mut cx, "layers-empty"));
    assert!(!drawn(&mut cx, row(t.f)));
    assert_eq!(NO_MATCHES, "No matching layers");

    change(&view, &mut cx, |view, _| view.layer_filter.clear());
    assert_eq!(expanded(&cx), HashSet::from([t.f]));
    assert!(drawn(&mut cx, row(t.a)) && drawn(&mut cx, row(t.g)));
    assert!(!drawn(&mut cx, row(t.c)));
}

/// Ruling P4-C7: a key typed in the filter goes to the filter, and the canvas's keys come back
/// with the keyboard.
#[gpui_kit::test]
fn typing_in_the_filter_leaves_the_canvas_keys_alone(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-filter-keys");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    // With the rectangle tool, "v" on the canvas would choose Move.
    change(&view, &mut cx, |view, cx| {
        view.choose("tool.rectangle", json!({}), cx)
    });
    click(&mut cx, "layers-filter");
    cx.simulate_keystrokes("v");
    cx.run_until_parked();
    assert_eq!(tool(&view, &cx), "rectangle");
    assert_eq!(
        view.read_with(&cx, |view, _| view.layer_filter.clone()),
        "v"
    );
    assert!(!drawn(&mut cx, row(t.f)));
    // Back on the canvas, "v" is its key again.
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("v");
    assert_eq!(tool(&view, &cx), "move");
}

/// Ruling P4-C12: hovering a row outlines its layer on the canvas, and leaving the rows ends
/// it. The row's eye and padlock show while it is hovered, or while the layer is hidden.
#[gpui_kit::test]
fn hovering_a_row_outlines_its_layer(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-hover");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    change(&view, &mut cx, |view, _| {
        view.expanded.insert(t.f);
    });
    let eye = |id: NodeId| selector(format!("layers-eye-{}", id.0));
    let none = Modifiers::default();
    assert!(!drawn(&mut cx, eye(t.a)));

    let over_a = middle(&mut cx, row(t.a));
    cx.simulate_mouse_move(over_a, None, none);
    cx.run_until_parked();
    assert_eq!(hovered(&view, &cx), Some(t.a));
    assert!(drawn(&mut cx, eye(t.a)));
    let over_b = middle(&mut cx, row(t.b));
    cx.simulate_mouse_move(over_b, None, none);
    cx.run_until_parked();
    assert_eq!(hovered(&view, &cx), Some(t.b));
    assert!(!drawn(&mut cx, eye(t.a)));
    // Onto the filter, under no row.
    let off = middle(&mut cx, "layers-filter");
    cx.simulate_mouse_move(off, None, none);
    cx.run_until_parked();
    assert_eq!(hovered(&view, &cx), None);

    // A hidden layer's eye shows without a hover, and a click on it shows the layer.
    change(&view, &mut cx, |view, cx| view.toggle_visible(t.a, cx));
    assert!(drawn(&mut cx, eye(t.a)));
    let selected = selection(&view, &cx);
    click(&mut cx, eye(t.a));
    cx.run_until_parked();
    assert_eq!(shown_and_locked(&view, t.a, &cx), (true, false));
    // The click was the eye's: the row's layer wasn't selected by it.
    assert_eq!(selection(&view, &cx), selected);
}

/// The reveal: a layer selected on the canvas opens the layers holding it in the tree, once,
/// so that a row closed by hand afterwards stays closed.
#[gpui_kit::test]
fn a_selection_made_on_the_canvas_opens_its_way_in_the_tree(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-reveal");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    assert!(!drawn(&mut cx, row(t.c)));
    change(&view, &mut cx, |view, _| {
        view.board_mut().unwrap().session.set_selection(vec![t.c]);
    });
    assert!(drawn(&mut cx, row(t.c)));
    assert_eq!(
        view.read_with(&cx, |view, _| view.expanded.clone()),
        HashSet::from([t.f, t.g])
    );
    // Closed by hand, G stays closed.
    click(&mut cx, selector(format!("layers-arrow-{}", t.g.0)));
    cx.run_until_parked();
    assert!(!drawn(&mut cx, row(t.c)));
    assert!(drawn(&mut cx, row(t.g)));
}

/// A drag of a row onto the top of another moves the layer there, with the mouse.
#[gpui_kit::test]
fn a_row_dragged_onto_the_top_of_another_goes_above_it(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-drag");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    change(&view, &mut cx, |view, _| {
        view.expanded.insert(t.f);
    });
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let from = middle(&mut cx, row(t.a));
    let b = cx.debug_bounds(row(t.b)).unwrap();
    let onto = point(b.center().x, b.origin.y + px(3.));
    cx.simulate_mouse_down(from, left, none);
    cx.simulate_mouse_move(from + point(px(0.), px(-6.)), left, none);
    cx.simulate_mouse_move(onto, left, none);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(&cx, |view, _| view.drag.clone()),
        Some(LayerDrag {
            ids: vec![t.a],
            over: Some((1, DropPos::Above)),
        })
    );
    cx.simulate_mouse_up(onto, left, none);
    cx.run_until_parked();
    assert_eq!(children(&view, t.f, &cx), [t.g, t.b, t.a]);
    assert!(view.read_with(&cx, |view, _| view.drag.is_none()));
}

/// Every icon the panel and its toggle show is one Sawit embeds (as ruling P5-C11 has it for
/// the Design panel).
#[test]
fn every_icon_of_the_layers_panel_is_embedded() {
    let kinds = [
        LayerIcon::Frame,
        LayerIcon::Section,
        LayerIcon::Group,
        LayerIcon::Component,
        LayerIcon::Instance,
        LayerIcon::Text,
        LayerIcon::Rectangle,
        LayerIcon::Ellipse,
        LayerIcon::Vector,
        LayerIcon::Line,
        LayerIcon::Image,
        LayerIcon::Sticky,
        LayerIcon::Shape,
        LayerIcon::Connector,
        LayerIcon::Ink,
        LayerIcon::Stamp,
        LayerIcon::Table,
        LayerIcon::Other,
    ];
    let icons = ICONS.iter().copied().chain(kinds.map(layer_icon));
    for icon in icons.chain([top_bar::LAYERS_ICON]) {
        let path = icon.path();
        let svg = crate::assets::AppAssets.load(&path).unwrap();
        assert!(svg.is_some(), "{path} isn't embedded");
    }
    assert_eq!(NO_LAYERS, "No layers on this page");
}
