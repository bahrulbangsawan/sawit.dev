//! Tests that every choice the canvas offers has a picture, embedded, and the same one wherever it
//! is offered.

use gpui_kit::AssetSource as _;
use sawit_board::context_bar::{CAPS, Control, options};
use sawit_board::toolbar::{Flyout, FlyoutItem, flyout};
use sawit_board::{BoardDoc, DocKind};
use serde_json::json;

// Named one by one: a glob would bring in gpui-kit's own `test` attribute, which takes the
// place of the standard one.
use super::{
    BlendMode, BoardShape, DELETE_PAGE, DRAWN, DUPLICATE_PAGE, End, IconNamed, OptionIcon,
    RENAME_PAGE, Stamp, ToolIcon, Turn, asset, blend, cap, of_param, of_params, shape_svg,
    stamp_svg, tool, zoom,
};

/// Whether the app's assets serve `icon`'s picture as an SVG.
fn embedded(icon: OptionIcon) -> bool {
    let path = icon.glyph.path();
    let svg = crate::assets::AppAssets.load(&path).unwrap();
    svg.is_some_and(|svg| svg.starts_with(b"<svg"))
}

/// Every flyout of both toolbars with its choices, as the model lists them on a new board.
fn flyouts() -> Vec<(Flyout, Vec<FlyoutItem>)> {
    let board = BoardDoc::new_board("Canvas").unwrap();
    let mut groups = sawit_board::toolbar::groups(DocKind::Board);
    groups.extend(sawit_board::toolbar::groups(DocKind::Design));
    let shown = groups.iter().filter_map(|group| group.flyout);
    shown.map(|f| (f, flyout(&board.session, f))).collect()
}

/// The picture the toolbar draws for `item`: a tool's, or a choice's by its param.
fn picture(item: &FlyoutItem) -> Option<OptionIcon> {
    match item.icon {
        Some(icon) => Some(tool(icon).into()),
        None => of_params(&item.params),
    }
}

/// Every control of the context bar that is a menu, with no value chosen.
const MENUS: [Control; 5] = [
    Control::Shape { current: None },
    Control::FontSize { current: None },
    Control::Routing { current: None },
    Control::StartCap { current: None },
    Control::EndCap { current: None },
];

#[test]
fn every_flyout_choice_has_a_picture() {
    let flyouts = flyouts();
    // The board's sticky colours, shapes, connector styles and stamps, and the tools of the
    // groups of both toolbars.
    assert!(flyouts.len() >= 8, "{flyouts:?}");
    for (f, items) in flyouts {
        assert!(!items.is_empty(), "{f:?}");
        for item in items {
            // A colour is its own swatch.
            if item.swatch.is_some() {
                continue;
            }
            let icon = picture(&item);
            let icon = icon.unwrap_or_else(|| panic!("{f:?} {} has no picture", item.label));
            assert!(embedded(icon), "{f:?} {}", item.label);
        }
    }
}

#[test]
fn every_context_bar_choice_has_a_picture() {
    for control in MENUS {
        let choices = options(&control);
        assert!(!choices.is_empty(), "{control:?}");
        for choice in choices {
            let icon = of_params(&choice.params);
            let icon = icon.unwrap_or_else(|| panic!("{control:?} {}", choice.label));
            assert!(embedded(icon), "{control:?} {}", choice.label);
        }
    }
}

#[test]
fn the_same_choice_has_the_same_picture_in_the_flyout_and_the_bar() {
    let pictures = |params: &[serde_json::Value]| {
        params
            .iter()
            .map(|p| of_params(p).unwrap())
            .collect::<Vec<_>>()
    };
    for (f, control) in [
        (Flyout::Shapes, Control::Shape { current: None }),
        (Flyout::ConnectorStyles, Control::Routing { current: None }),
    ] {
        let board = BoardDoc::new_board("Canvas").unwrap();
        let offered: Vec<_> = flyout(&board.session, f)
            .into_iter()
            .map(|i| i.params)
            .collect();
        let chosen: Vec<_> = options(&control).into_iter().map(|o| o.params).collect();
        assert_eq!(pictures(&offered), pictures(&chosen), "{f:?}");
    }
}

#[test]
fn a_start_cap_is_the_end_cap_turned_over() {
    for name in CAPS {
        let start = cap(name, End::Start).unwrap();
        let end = cap(name, End::End).unwrap();
        assert_eq!(start.glyph, end.glyph, "{name}");
        assert_eq!(
            (start.turn, end.turn),
            (Turn::Mirrored, Turn::Upright),
            "{name}"
        );
    }
    // As the context bar's params say which end they set.
    let start = of_param("startCap", &json!("lineArrow")).unwrap();
    assert_eq!(start.turn, Turn::Mirrored);
    assert_eq!(
        of_param("endCap", &json!("lineArrow")).unwrap().turn,
        Turn::Upright
    );
}

#[test]
fn choices_tell_apart_within_their_menu() {
    // Each choice of a toolbar or context bar menu has a picture of its own.
    for (f, items) in flyouts() {
        let icons: Vec<_> = items.iter().filter_map(picture).collect();
        for (i, a) in icons.iter().enumerate() {
            assert!(!icons[i + 1..].contains(a), "{f:?} repeats {a:?}");
        }
    }
    for control in MENUS {
        let icons: Vec<_> = (options(&control).iter())
            .map(|o| of_params(&o.params).unwrap())
            .collect();
        for (i, a) in icons.iter().enumerate() {
            assert!(!icons[i + 1..].contains(a), "{control:?} repeats {a:?}");
        }
    }
}

#[test]
fn an_unknown_choice_has_no_picture() {
    assert_eq!(of_param("shape", &json!("blob")), None);
    assert_eq!(of_param("endCap", &json!("round")), None);
    assert_eq!(of_param("fontSize", &json!(13)), None);
    assert_eq!(of_param("color", &json!("blue")), None);
    assert_eq!(
        of_params(&json!({"style": "elbow", "shape": "ellipse"})),
        None
    );
    assert_eq!(asset("canvas_icons/shape-blob.svg"), None);
    assert_eq!(asset("canvas_icons/nothing.svg"), None);
}

#[test]
fn every_drawing_is_lucides_box_and_stroke() {
    for (name, svg) in DRAWN {
        let svg = std::str::from_utf8(svg).unwrap();
        for attribute in [
            r#"viewBox="0 0 24 24""#,
            r#"stroke="currentColor""#,
            r#"stroke-width="2""#,
            r#"stroke-linecap="round""#,
            r#"stroke-linejoin="round""#,
        ] {
            assert!(svg.contains(attribute), "{name} lacks {attribute}");
        }
    }
}

#[test]
fn shapes_and_stamps_are_drawn_from_the_engine() {
    let outline = shape_svg(BoardShape::Diamond);
    assert!(outline.contains(&BoardShape::Diamond.path(20.0, 16.0).to_svg()));
    let mark = stamp_svg(Stamp::Check);
    assert!(mark.contains(Stamp::Check.mark()[0].d));
}

#[test]
fn every_tool_has_a_picture() {
    // Every tool of both toolbars, with the groups' flyouts, which list them all.
    let mut icons = Vec::new();
    for kind in [DocKind::Board, DocKind::Design] {
        let groups = sawit_board::toolbar::groups(kind);
        icons.extend(
            groups
                .into_iter()
                .flat_map(|group| group.tools)
                .map(|tool| tool.icon),
        );
    }
    assert!(icons.len() > 20, "{icons:?}");
    icons.push(ToolIcon::Image);
    for icon in icons {
        assert!(embedded(tool(icon).into()), "{icon:?}");
    }
    for icon in [RENAME_PAGE, DUPLICATE_PAGE, DELETE_PAGE] {
        assert!(embedded(icon.into()), "{icon:?}");
    }
}

#[test]
fn every_zoom_command_but_the_set_ones_has_a_picture() {
    for command in [
        "view.zoomIn",
        "view.zoomOut",
        "view.zoomFit",
        "view.zoomSelection",
    ] {
        assert!(zoom(command).is_some_and(embedded), "{command}");
    }
    assert_eq!(zoom("view.zoom100"), None);
}

#[test]
fn every_blend_mode_has_a_picture() {
    for mode in BlendMode::ALL {
        assert!(embedded(blend(mode)), "{mode:?}");
    }
}
