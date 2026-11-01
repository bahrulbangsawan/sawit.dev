//! The pictures of the canvas's choices, in one place: what every dropdown, flyout and menu of the
//! canvas draws beside an option's words, so that the same choice has the same picture wherever it
//! is offered (the toolbar's buttons and flyouts, the context bar, the Design panel, the top bar
//! and the layers panel). A choice is looked up by the id the engine knows it by: the param a pick
//! sets (`{"style": "elbow"}`, `{"endCap": "triangleArrow"}`), the engine's own enums (a board
//! shape, a stamp, a blend mode), the toolbar model's tool pictures (`ToolIcon`), the Design
//! panel's property values (`inside`, `hug`, `dropShadow`, `png`) or a command id (`view.zoomIn`).
//!
//! The pictures come from three places:
//! - Lucide icons from gpui-kit's set, where one shows the choice already (bundled in
//!   `crate::assets`, as Sawit's other icons are).
//! - Sawit's own drawings in `assets/canvas_icons/`, on Lucide's 24-unit box with its 2-unit
//!   stroke and round ends, in `currentColor` so that they take the theme's colour: connector
//!   styles, arrowheads, text sizes, stroke positions and effects.
//! - The engine's own geometry, drawn as the canvas draws it: a board shape's outline
//!   (`BoardShape::path`) and a stamp's mark (`Stamp::mark`) on its colour.
//!
//! A start cap is the end cap's drawing turned over left to right, so the start and end menus read
//! as the two ends of one line, and a vertical sizing is the horizontal one's turned upright or its
//! Lucide counterpart. A picture is never all a choice says: the menus keep its words beside it,
//! and an icon-only button names its choice in its tooltip and to VoiceOver (Sawit's rule that
//! state is never carried by an icon or colour alone).

use std::borrow::Cow;
use std::f32::consts::FRAC_PI_2;

use figcraft_doc::color::BlendMode;
use figcraft_doc::{BoardShape, Routing, Stamp};
use figcraft_engine::board::TEXT_SIZES;
use gpui_kit::assets::{IconName as Lucide, IconNamed};
use gpui_kit::component::Icon;
use gpui_kit::*;
use sawit_board::toolbar::ToolIcon;
use serde_json::Value;

/// Where the canvas's own pictures are served from, in the app's assets.
const PREFIX: &str = "canvas_icons/";

macro_rules! drawn {
    ($($name:literal),* $(,)?) => {
        /// Sawit's drawings of the canvas's choices, by name (`assets/canvas_icons/<name>.svg`).
        const DRAWN: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../../../assets/canvas_icons/", $name, ".svg")))),*
        ];
    };
}

drawn!(
    "route-straight",
    "route-elbow",
    "route-curved",
    "cap-none",
    "cap-line-arrow",
    "cap-triangle-arrow",
    "cap-reversed-triangle",
    "cap-circle",
    "cap-diamond",
    "text-size-s",
    "text-size-m",
    "text-size-l",
    "text-size-xl",
    "stroke-inside",
    "stroke-center",
    "stroke-outside",
    "effect-drop-shadow",
    "effect-inner-shadow",
    "effect-layer-blur",
    "effect-background-blur",
);

/// The picture of a choice, before it is turned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    /// A Lucide icon.
    Lucide(Lucide),
    /// One of Sawit's drawings ([`DRAWN`]), by name.
    Drawn(&'static str),
    /// A board shape's outline, as the engine draws it.
    Shape(BoardShape),
    /// A stamp's mark. [`OptionIcon::element`] draws it white on the stamp's colour, as the canvas
    /// does; as a plain icon it takes the text's colour.
    Stamp(Stamp),
}

impl IconNamed for Glyph {
    fn path(self) -> SharedString {
        match self {
            Glyph::Lucide(icon) => icon.path(),
            Glyph::Drawn(name) => format!("{PREFIX}{name}.svg").into(),
            Glyph::Shape(shape) => format!("{PREFIX}shape-{}.svg", shape.id()).into(),
            Glyph::Stamp(stamp) => format!("{PREFIX}stamp-{}.svg", stamp.id()).into(),
        }
    }
}

/// How a picture is turned before it is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Turn {
    #[default]
    Upright,
    /// Turned over left to right: a start cap, which is the end cap's picture facing the other
    /// way.
    Mirrored,
    /// Turned a quarter: a vertical sizing drawn from the horizontal one.
    Quarter,
}

/// The picture of one choice: a [`Glyph`], turned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OptionIcon {
    pub(crate) glyph: Glyph,
    pub(crate) turn: Turn,
}

impl From<Lucide> for OptionIcon {
    fn from(icon: Lucide) -> Self {
        Glyph::Lucide(icon).into()
    }
}

impl From<Glyph> for OptionIcon {
    fn from(glyph: Glyph) -> Self {
        Self {
            glyph,
            turn: Turn::Upright,
        }
    }
}

impl OptionIcon {
    const fn drawn(name: &'static str) -> Self {
        Self {
            glyph: Glyph::Drawn(name),
            turn: Turn::Upright,
        }
    }

    const fn turned(self, turn: Turn) -> Self {
        Self { turn, ..self }
    }

    /// The picture as an icon in the text's colour, for a menu item or a button. A stamp is its
    /// mark alone here; [`Self::element`] draws its badge.
    pub(crate) fn icon(self) -> Icon {
        let icon = Icon::new(self.glyph);
        match self.turn {
            Turn::Upright => icon,
            Turn::Mirrored => icon.transform(Transformation::scale(size(-1., 1.))),
            Turn::Quarter => icon.rotate(radians(-FRAC_PI_2)),
        }
    }

    /// The picture `side` px square, as a menu row or a flyout draws it: a stamp as its badge, its
    /// white mark on its colour as the canvas draws it, and the others as [`Self::icon`].
    pub(crate) fn element(self, side: Pixels) -> AnyElement {
        match self.glyph {
            Glyph::Stamp(stamp) => {
                let [r, g, b, a] = stamp.color().to_rgba8();
                let colour = Rgba {
                    r: f32::from(r) / 255.,
                    g: f32::from(g) / 255.,
                    b: f32::from(b) / 255.,
                    a: f32::from(a) / 255.,
                };
                // The mark's picture is the middle three fifths of the badge ([`stamp_svg`]), so
                // the mark sits on the badge where the canvas draws it.
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(side)
                    .rounded_full()
                    .bg(colour)
                    .child(self.icon().size(side * 0.6).text_color(white()))
                    .into_any_element()
            }
            _ => self.icon().size(side).into_any_element(),
        }
    }
}

/// The side of a choice's picture beside its words in a menu.
pub(crate) const MENU_GLYPH: f32 = 14.;

/// A menu item's picture and words, as every menu of choices on the canvas draws them: the
/// picture, when the choice has one, then the words. The menu's check, for the choice made now,
/// stays before them.
pub(crate) fn menu_row(picture: Option<OptionIcon>, words: impl IntoElement) -> Div {
    gpui_kit::component::h_flex()
        .gap_2()
        .children(picture.map(|picture| picture.element(px(MENU_GLYPH))))
        .child(words)
}

/// The two ends of a connector, whose cap menus mirror each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Start,
    End,
}

/// The picture of the choice that sets the engine's param `key` to `value`, as the toolbar's
/// flyouts and the context bar offer them: a shape, a connector's style, a stamp, a start or end
/// cap, or a text size. `None` for a colour, whose swatch is its picture, and for a value the
/// engine doesn't have.
pub(crate) fn of_param(key: &str, value: &Value) -> Option<OptionIcon> {
    let id = value.as_str();
    match key {
        "shape" => BoardShape::from_id(id?).map(shape),
        "style" => Routing::from_id(id?).map(routing),
        "stamp" => Stamp::from_id(id?).map(stamp),
        "startCap" => cap(id?, End::Start),
        "endCap" => cap(id?, End::End),
        "fontSize" => {
            let size = value.as_f64()?;
            let named = TEXT_SIZES
                .iter()
                .find(|(_, named)| (named - size).abs() < 0.01);
            text_size(named?.0)
        }
        _ => None,
    }
}

/// [`of_param`] for an option's params, which set one param.
pub(crate) fn of_params(params: &Value) -> Option<OptionIcon> {
    let mut set = params.as_object()?.iter();
    match (set.next(), set.next()) {
        (Some((key, value)), None) => of_param(key, value),
        _ => None,
    }
}

/// A board shape: its outline.
pub(crate) fn shape(shape: BoardShape) -> OptionIcon {
    Glyph::Shape(shape).into()
}

/// A stamp: its badge.
pub(crate) fn stamp(stamp: Stamp) -> OptionIcon {
    Glyph::Stamp(stamp).into()
}

/// How a connector runs: a straight line, an elbow or a curve between two ends.
pub(crate) fn routing(routing: Routing) -> OptionIcon {
    OptionIcon::drawn(match routing {
        Routing::Straight => "route-straight",
        Routing::Elbow => "route-elbow",
        Routing::Curved => "route-curved",
    })
}

/// A connector cap by the name `board.setStyle` reads (`sawit_board::context_bar::CAPS`): the cap
/// on the end of a short line, which trails off in dots where the rest of the connector goes. An
/// end cap faces right and a start cap left.
pub(crate) fn cap(name: &str, end: End) -> Option<OptionIcon> {
    let drawn = OptionIcon::drawn(match name {
        "none" => "cap-none",
        "lineArrow" => "cap-line-arrow",
        "triangleArrow" => "cap-triangle-arrow",
        "reversedTriangle" => "cap-reversed-triangle",
        "circleFilled" => "cap-circle",
        "diamondFilled" => "cap-diamond",
        _ => return None,
    });
    Some(match end {
        End::Start => drawn.turned(Turn::Mirrored),
        End::End => drawn,
    })
}

/// A text size by its name in the engine's `TEXT_SIZES`: a letter A of that size.
pub(crate) fn text_size(name: &str) -> Option<OptionIcon> {
    Some(OptionIcon::drawn(match name {
        "S" => "text-size-s",
        "M" => "text-size-m",
        "L" => "text-size-l",
        "XL" => "text-size-xl",
        _ => return None,
    }))
}

/// A blend mode: the group it is in, as Figma's menu groups them (the separators of the Design
/// panel's menu). The modes of a group darken, lighten or add contrast alike, and the menu's words
/// tell them apart.
pub(crate) fn blend(mode: BlendMode) -> OptionIcon {
    use BlendMode::*;
    OptionIcon::from(match mode {
        PassThrough => Lucide::Layers,
        Normal => Lucide::Circle,
        Darken | Multiply | PlusDarker | ColorBurn => Lucide::Moon,
        Lighten | Screen | PlusLighter | ColorDodge => Lucide::Sun,
        Overlay | SoftLight | HardLight => Lucide::Contrast,
        Difference | Exclusion => Lucide::Diff,
        Hue | Saturation | Color | Luminosity => Lucide::Palette,
    })
}

/// Where a stroke sits, by the value of `strokeAlign`: inside the layer's dotted edge, on it, or
/// outside it.
pub(crate) fn stroke_align(align: &str) -> Option<OptionIcon> {
    Some(OptionIcon::drawn(match align {
        "inside" => "stroke-inside",
        "center" => "stroke-center",
        "outside" => "stroke-outside",
        _ => return None,
    }))
}

/// How a frame is sized along an axis, by the value of `sizingH` or `sizingV` (`vertical`): a
/// fixed measure, hugging its contents (arrows in), or filling its container (arrows out).
pub(crate) fn sizing(sizing: &str, vertical: bool) -> Option<OptionIcon> {
    let (across, down) = match sizing {
        "fixed" => {
            let fixed = OptionIcon::from(Lucide::RulerDimensionLine);
            (fixed, fixed.turned(Turn::Quarter))
        }
        "hug" => (Lucide::FoldHorizontal.into(), Lucide::FoldVertical.into()),
        "fill" => (
            Lucide::UnfoldHorizontal.into(),
            Lucide::UnfoldVertical.into(),
        ),
        _ => return None,
    };
    Some(if vertical { down } else { across })
}

/// An effect by its kind's tag: a shadow outside or inside a layer, or a blur of the layer or of
/// what is behind it.
pub(crate) fn effect(kind: &str) -> Option<OptionIcon> {
    Some(OptionIcon::drawn(match kind {
        "dropShadow" => "effect-drop-shadow",
        "innerShadow" => "effect-inner-shadow",
        "layerBlur" => "effect-layer-blur",
        "backgroundBlur" => "effect-background-blur",
        _ => return None,
    }))
}

/// An export format by its extension: a picture for PNG, a photo for JPG, a vector for SVG and a
/// document for PDF.
pub(crate) fn export_format(format: &str) -> Option<OptionIcon> {
    Some(OptionIcon::from(match format {
        "png" => Lucide::Image,
        "jpg" => Lucide::Camera,
        "svg" => Lucide::PenTool,
        "pdf" => Lucide::FileText,
        _ => return None,
    }))
}

/// A command of the top bar's zoom menu. The set zooms (50%, 100%, 200%) have none: their words
/// are a number, which says it better than a picture would.
pub(crate) fn zoom(command: &str) -> Option<OptionIcon> {
    Some(OptionIcon::from(match command {
        "view.zoomIn" => Lucide::ZoomIn,
        "view.zoomOut" => Lucide::ZoomOut,
        "view.zoomFit" => Lucide::Scan,
        "view.zoomSelection" => Lucide::ScanSearch,
        _ => return None,
    }))
}

/// The layers panel's page menu: Rename page, Duplicate page and Delete page.
pub(crate) const RENAME_PAGE: Lucide = Lucide::Pencil;
pub(crate) const DUPLICATE_PAGE: Lucide = Lucide::CopyPlus;
pub(crate) const DELETE_PAGE: Lucide = Lucide::Trash;

/// A tool as the toolbar's model pictures it (`sawit_board::toolbar::ToolIcon`): its button, and
/// its row in the flyout of its group. Every tool's picture is a Lucide icon.
pub(crate) fn tool(icon: ToolIcon) -> Lucide {
    match icon {
        ToolIcon::Move => Lucide::MousePointer2,
        ToolIcon::Hand => Lucide::Hand,
        ToolIcon::Scale => Lucide::Scaling,
        ToolIcon::Frame => Lucide::Frame,
        ToolIcon::Section => Lucide::SquareDashed,
        ToolIcon::Slice => Lucide::Slice,
        ToolIcon::Rectangle => Lucide::Square,
        ToolIcon::Line => Lucide::Slash,
        ToolIcon::Arrow => Lucide::ArrowUpRight,
        ToolIcon::Ellipse => Lucide::Circle,
        ToolIcon::Polygon => Lucide::Triangle,
        ToolIcon::Star => Lucide::Star,
        ToolIcon::Pen => Lucide::PenTool,
        ToolIcon::Pencil => Lucide::Pencil,
        ToolIcon::Text => Lucide::Type,
        ToolIcon::TextOnPath => Lucide::Signature,
        ToolIcon::Marker => Lucide::PenLine,
        ToolIcon::Highlighter => Lucide::Highlighter,
        ToolIcon::Eraser => Lucide::Eraser,
        ToolIcon::Sticky => Lucide::StickyNote,
        ToolIcon::Shapes => Lucide::Shapes,
        ToolIcon::Connector => Lucide::Spline,
        ToolIcon::Stamp => Lucide::Stamp,
        ToolIcon::Image => Lucide::ImagePlus,
    }
}

/// The SVG of a picture the canvas serves under [`PREFIX`]: one of Sawit's drawings, a board
/// shape's outline or a stamp's mark. `None` for any other path.
pub(crate) fn asset(path: &str) -> Option<Cow<'static, [u8]>> {
    let name = path.strip_prefix(PREFIX)?.strip_suffix(".svg")?;
    if let Some((_, svg)) = DRAWN.iter().find(|(drawn, _)| *drawn == name) {
        return Some(Cow::Borrowed(svg));
    }
    if let Some(id) = name.strip_prefix("shape-") {
        return BoardShape::from_id(id).map(|shape| Cow::Owned(shape_svg(shape).into_bytes()));
    }
    let id = name.strip_prefix("stamp-")?;
    Stamp::from_id(id).map(|stamp| Cow::Owned(stamp_svg(stamp).into_bytes()))
}

/// How a line of a picture is drawn: in the text's colour, with round ends and joins.
const ROUND: &str = r#"stroke="currentColor" stroke-linecap="round" stroke-linejoin="round""#;

/// The start of a picture's SVG, 24 units square, showing `view_box` of its drawing.
fn svg_head(view_box: &str) -> String {
    let xmlns = "http://www.w3.org/2000/svg";
    format!(r#"<svg xmlns="{xmlns}" width="24" height="24" viewBox="{view_box}">"#)
}

/// A board shape's outline, from the engine's own path, in Lucide's box and stroke: 20 by 16, the
/// proportions of a shape on a board, in the middle of the 24-unit box.
fn shape_svg(shape: BoardShape) -> String {
    let outline = shape.path(20.0, 16.0).to_svg();
    let head = svg_head("0 0 24 24");
    let paint = format!(r#"fill="none" stroke-width="2" {ROUND}"#);
    format!(r#"{head}<path {paint} transform="translate(2 4)" d="{outline}"/></svg>"#)
}

/// A stamp's mark, from the engine's own parts on its 40-unit badge, as the canvas draws them:
/// filled, or stroked 3 units wide with round ends. The picture is the middle 24 units of the
/// badge, so the mark (the middle 20) fills it with a little room around.
fn stamp_svg(stamp: Stamp) -> String {
    let parts: String = (stamp.mark().iter())
        .map(|part| {
            let paint = if part.filled {
                r#"fill="currentColor""#.to_owned()
            } else {
                format!(r#"fill="none" stroke-width="3" {ROUND}"#)
            };
            format!(r#"<path {paint} d="{}"/>"#, part.d)
        })
        .collect();
    format!("{}{parts}</svg>", svg_head("8 8 24 24"))
}

#[cfg(test)]
mod tests {
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
}
