//! The Colors tab: every colour in use, deduplicated by hex and grouped by role, with how the
//! page's text colours read on its background.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::site_design::color::{self, Rgba};
use sawit_core::site_design::scan::{ColorRole, Scan};

use super::DesignPanel;
use super::rows::{hex_and_alpha, swatch};
use crate::browser::panel;

/// The group order of the segment.
const ROLES: [ColorRole; 7] = [
    ColorRole::Text,
    ColorRole::Background,
    ColorRole::Border,
    ColorRole::Fill,
    ColorRole::Stroke,
    ColorRole::Shadow,
    ColorRole::Gradient,
];

/// How many text colours the contrast list checks.
const CONTRAST_PAIRS: usize = 4;
/// Below this ratio a text colour can't be text on the page background.
const ON_ANOTHER_SURFACE: f64 = 1.5;

/// The scan's colors by role (in [`ROLES`] order): the same color written differently is one
/// entry with the counts added, most used first. Values that aren't colors are left out.
pub(super) fn group_colors(scan: &Scan) -> Vec<(ColorRole, Vec<(Rgba, u32)>)> {
    ROLES
        .into_iter()
        .filter_map(|role| {
            let mut merged: Vec<(Rgba, u32)> = Vec::new();
            for c in scan.colors.iter().filter(|c| c.role == role) {
                let Some(rgba) = color::parse(&c.value) else {
                    continue;
                };
                match merged.iter_mut().find(|(r, _)| r.to_hex() == rgba.to_hex()) {
                    Some((_, n)) => *n += c.count,
                    None => merged.push((rgba, c.count)),
                }
            }
            merged.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_hex().cmp(&b.0.to_hex())));
            (!merged.is_empty()).then_some((role, merged))
        })
        .collect()
}

fn role_title(role: ColorRole) -> &'static str {
    match role {
        ColorRole::Text => "Text",
        ColorRole::Background => "Background",
        ColorRole::Border => "Border",
        ColorRole::Fill => "SVG fill",
        ColorRole::Stroke => "SVG stroke",
        ColorRole::Shadow => "Shadow",
        ColorRole::Gradient => "Gradient",
    }
}

/// `rgb(0, 113, 227)`, or `rgba(…, 0.5)` when translucent.
pub(super) fn rgb_css(c: &Rgba) -> String {
    if c.a < 1.0 {
        let a = (f64::from(c.a) * 100.).round() / 100.;
        format!("rgba({}, {}, {}, {a})", c.r, c.g, c.b)
    } else {
        format!("rgb({}, {}, {})", c.r, c.g, c.b)
    }
}

/// The Tailwind utility for the color in its role; none for shadows and gradients.
pub(super) fn role_class(role: ColorRole, hex: &str) -> Option<String> {
    let prefix = match role {
        ColorRole::Text => "text",
        ColorRole::Background => "bg",
        ColorRole::Border => "border",
        ColorRole::Fill => "fill",
        ColorRole::Stroke => "stroke",
        ColorRole::Shadow | ColorRole::Gradient => return None,
    };
    Some(format!("{prefix}-[{hex}]"))
}

/// The page's own custom property that holds this color, as a declaration. Only a variable the
/// page defines is offered.
pub(super) fn variable_line(scan: &Scan, c: &Rgba) -> Option<String> {
    let hex = c.to_hex();
    scan.variables
        .iter()
        .find(|(name, value)| {
            name.starts_with("--") && color::parse(value).is_some_and(|p| p.to_hex() == hex)
        })
        .map(|(name, value)| format!("{name}: {value};"))
}

/// One text colour on the page's background: the pair, its ratio and whether body text in it
/// passes AA.
#[derive(Debug, PartialEq)]
pub(super) struct Pair {
    pub text: Rgba,
    pub background: Rgba,
    pub ratio: f64,
}

/// The most used text colours on the page's background (white when the page sets none), the
/// way a reader meets them.
pub(super) fn contrast_pairs(scan: &Scan) -> Vec<Pair> {
    let background = scan
        .body_background
        .as_deref()
        .and_then(color::parse)
        .filter(|c| c.a > 0.)
        .unwrap_or(Rgba {
            r: 255,
            g: 255,
            b: 255,
            a: 1.,
        });
    let texts = group_colors(scan)
        .into_iter()
        .find(|(role, _)| *role == ColorRole::Text)
        .map(|(_, colors)| colors)
        .unwrap_or_default();
    texts
        .into_iter()
        .map(|(text, _)| Pair {
            text,
            background,
            ratio: color::contrast_ratio(text, background),
        })
        // A text colour this close to the background is set on another surface (light text
        // in a dark band): checked here it would read as a failure that isn't one.
        .filter(|p| p.ratio >= ON_ANOTHER_SURFACE)
        .take(CONTRAST_PAIRS)
        .collect()
}

pub(super) fn render(scan: &Scan, cx: &mut Context<DesignPanel>) -> impl IntoElement {
    let groups = group_colors(scan);
    if groups.is_empty() {
        return panel::empty(
            "No colors on this page",
            "Text, backgrounds, borders and SVG fills show here once the page sets them.",
            None,
            cx,
        )
        .into_any_element();
    }
    let weak = cx.entity().downgrade();
    let mut sections: Vec<AnyElement> = Vec::new();

    let pairs = contrast_pairs(scan);
    if !pairs.is_empty() {
        let rows = pairs.iter().map(|p| pair_row(p, cx)).collect::<Vec<_>>();
        sections.push(
            panel::section(
                "Main text colors on the page background",
                None,
                v_flex().gap_1().children(rows),
                cx,
            )
            .into_any_element(),
        );
    }

    for (gi, (role, colors)) in groups.into_iter().enumerate() {
        let count = colors.len();
        let rows: Vec<AnyElement> = colors
            .into_iter()
            .enumerate()
            .map(|(ci, (rgba, uses))| color_row(scan, role, (gi, ci), rgba, uses, &weak, cx))
            .collect();
        sections.push(
            panel::section(
                role_title(role),
                Some(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(count.to_string())
                        .into_any_element(),
                ),
                v_flex().children(rows),
                cx,
            )
            .into_any_element(),
        );
    }
    v_flex().gap_4().children(sections).into_any_element()
}

/// `Aa` in the text colour on the background, the ratio, and AA for body text.
fn pair_row(p: &Pair, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let pass = p.ratio >= 4.5;
    let (hex, alpha) = hex_and_alpha(&p.text);
    h_flex()
        .min_h(px(28.))
        .gap_2()
        .items_center()
        .text_xs()
        .child(
            div()
                .w(px(36.))
                .h(px(24.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(theme.border)
                .bg(gpui_kit::rgba(rgba_u32(&p.background)))
                .text_color(gpui_kit::rgba(rgba_u32(&p.text)))
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Aa"),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().font_family(theme.mono_font_family.clone()).child(hex))
                .children(alpha.map(|a| {
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("· {a}"))
                })),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(format!("{:.2}:1", (p.ratio * 100.).floor() / 100.)),
        )
        .child(
            div()
                .w(px(52.))
                .flex_none()
                .text_right()
                .text_color(if pass { theme.success } else { theme.danger })
                .child(if pass { "AA ✓" } else { "AA ✕" }),
        )
        .into_any_element()
}

/// A colour: its swatch, hex and opacity, how often it is used; click copies the hex, the
/// menu (on hover, or a right-click) copies it in other forms.
fn color_row(
    scan: &Scan,
    role: ColorRole,
    at: (usize, usize),
    rgba: Rgba,
    uses: u32,
    weak: &WeakEntity<DesignPanel>,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let (hex, alpha) = hex_and_alpha(&rgba);
    let css_hex = rgba.to_hex();
    let mut menu_items: Vec<(&'static str, String)> = vec![
        ("Copy hex", css_hex.clone()),
        ("Copy rgb()", rgb_css(&rgba)),
        ("Copy oklch()", rgba.oklch_css()),
    ];
    if let Some(line) = variable_line(scan, &rgba) {
        menu_items.push(("Copy CSS variable", line));
    }
    if let Some(class) = role_class(role, &css_hex) {
        menu_items.push(("Copy Tailwind class", class));
    }
    let group: SharedString = format!("design-color-{}-{}", at.0, at.1).into();
    let menu = |weak: WeakEntity<DesignPanel>, items: Vec<(&'static str, String)>| {
        move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let mut menu = menu;
            for (label, text) in items.clone() {
                let weak = weak.clone();
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |this, cx| this.copy(text.clone(), cx));
                }));
            }
            menu
        }
    };
    let copy_hex = css_hex.clone();
    h_flex()
        .id(group.clone())
        .group(group.clone())
        .min_h(px(28.))
        .px_1()
        .mx(px(-4.))
        .gap_2()
        .items_center()
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|el| el.bg(theme.secondary))
        .child(swatch(&rgba, px(20.), cx))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .items_baseline()
                .text_xs()
                .child(div().font_family(theme.mono_font_family.clone()).child(hex))
                .children(alpha.map(|a| div().text_color(muted).child(format!("· {a}")))),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(if uses == 1 {
                    "1 use".to_string()
                } else {
                    format!("{uses} uses")
                }),
        )
        .child(
            div()
                .flex_none()
                .invisible()
                .group_hover(group.clone(), |s| s.visible())
                // The menu's press isn't the row's click, which copies.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(SharedString::from(format!("{group}-more")))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Ellipsis)
                        .accessibility_label(format!("Copy {css_hex} as…"))
                        .dropdown_menu_with_anchor(
                            Anchor::TopRight,
                            menu(weak.clone(), menu_items.clone()),
                        ),
                ),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.copy(copy_hex.clone(), cx);
        }))
        .context_menu(menu(weak.clone(), menu_items))
        .into_any_element()
}

/// `0xRRGGBBAA`, for GPUI.
pub(super) fn rgba_u32(c: &Rgba) -> u32 {
    let a = (c.a.clamp(0., 1.) * 255.).round() as u32;
    u32::from(c.r) << 24 | u32::from(c.g) << 16 | u32::from(c.b) << 8 | a
}

#[cfg(test)]
mod tests {
    use super::{contrast_pairs, group_colors, rgb_css, rgba_u32, role_class, variable_line};
    use sawit_core::site_design::color;
    use sawit_core::site_design::scan::{ColorRole, ColorUse, Scan};

    fn use_of(value: &str, role: ColorRole, count: u32) -> ColorUse {
        ColorUse {
            value: value.into(),
            role,
            count,
            on_button: 0,
            on_link: 0,
        }
    }

    #[test]
    fn groups_merge_duplicates_and_follow_role_order() {
        let scan = Scan {
            colors: vec![
                use_of("#111111", ColorRole::Shadow, 1),
                use_of("rgb(0, 113, 227)", ColorRole::Background, 4),
                use_of("#ffffff", ColorRole::Background, 9),
                use_of("#0071e3", ColorRole::Background, 3),
                use_of("#222", ColorRole::Stroke, 2),
                use_of("#333", ColorRole::Fill, 2),
                use_of("#000000", ColorRole::Text, 5),
                use_of("not a color", ColorRole::Border, 1),
            ],
            ..Scan::default()
        };
        let groups = group_colors(&scan);
        let roles: Vec<ColorRole> = groups.iter().map(|(r, _)| *r).collect();
        assert_eq!(
            roles,
            [
                ColorRole::Text,
                ColorRole::Background,
                ColorRole::Fill,
                ColorRole::Stroke,
                ColorRole::Shadow
            ]
        );
        let bg: Vec<(String, u32)> = groups[1].1.iter().map(|(c, n)| (c.to_hex(), *n)).collect();
        assert_eq!(bg, [("#ffffff".to_string(), 9), ("#0071e3".to_string(), 7)]);
    }

    #[test]
    fn copy_formats() {
        let c = color::parse("#0071e3").unwrap();
        assert_eq!(rgb_css(&c), "rgb(0, 113, 227)");
        assert_eq!(
            role_class(ColorRole::Text, "#0071e3").unwrap(),
            "text-[#0071e3]"
        );
        assert_eq!(role_class(ColorRole::Shadow, "#0071e3"), None);
        let scan = Scan {
            variables: vec![("--brand".into(), "rgb(0, 113, 227)".into())],
            ..Scan::default()
        };
        assert_eq!(
            variable_line(&scan, &c).as_deref(),
            Some("--brand: rgb(0, 113, 227);")
        );
        assert_eq!(rgba_u32(&c), 0x0071e3ff);
    }

    /// The most used text colours are checked on the page's background, most used first; a
    /// page with no background of its own reads on white.
    #[test]
    fn text_colours_are_checked_on_the_background() {
        let mut scan = Scan {
            colors: vec![
                use_of("#767676", ColorRole::Text, 3),
                use_of("#000000", ColorRole::Text, 9),
                use_of("#ffffff", ColorRole::Background, 9),
            ],
            ..Scan::default()
        };
        let pairs = contrast_pairs(&scan);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].text.to_hex(), "#000000");
        assert!((pairs[0].ratio - 21.).abs() < 0.01);
        assert!(
            pairs[1].ratio >= 4.5 && pairs[1].ratio < 4.6,
            "{}",
            pairs[1].ratio
        );
        // On a black page, black text must sit on another surface: it isn't listed.
        scan.body_background = Some("#000000".into());
        let on_black = contrast_pairs(&scan);
        assert_eq!(on_black.len(), 1);
        assert_eq!(on_black[0].text.to_hex(), "#767676");
    }
}
