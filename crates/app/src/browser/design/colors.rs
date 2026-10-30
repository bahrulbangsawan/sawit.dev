//! The Colors segment: every color in use, deduplicated by hex and grouped by role.

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;
use sawit_core::site_design::color::{self, Rgba};
use sawit_core::site_design::scan::{ColorRole, Scan};

use super::DesignPanel;

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

pub(super) fn render(scan: &Scan, cx: &mut Context<DesignPanel>) -> impl IntoElement {
    let groups = group_colors(scan);
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    if groups.is_empty() {
        return div()
            .text_sm()
            .text_color(muted)
            .child("No colors found on this page.")
            .into_any_element();
    }
    let weak = cx.entity().downgrade();
    let mut sections: Vec<AnyElement> = Vec::new();
    for (gi, (role, colors)) in groups.into_iter().enumerate() {
        let rows = colors.into_iter().enumerate().map(|(ci, (rgba, count))| {
            let hex = rgba.to_hex();
            let mut menu_items: Vec<(&'static str, String)> = vec![
                ("Copy rgb()", rgb_css(&rgba)),
                ("Copy oklch()", rgba.oklch_css()),
            ];
            if let Some(line) = variable_line(scan, &rgba) {
                menu_items.push(("Copy CSS variable", line));
            }
            if let Some(class) = role_class(role, &hex) {
                menu_items.push(("Copy Tailwind class", class));
            }
            let weak = weak.clone();
            let copy_hex = hex.clone();
            let fill = rgba_u32(&rgba);
            h_flex()
                .id(SharedString::from(format!("design-color-{gi}-{ci}")))
                .gap_2()
                .px_1()
                .py_0p5()
                .items_center()
                .rounded(theme.radius)
                .cursor_pointer()
                .hover(|el| el.bg(theme.accent))
                .child(
                    div()
                        .size_4()
                        .flex_none()
                        .rounded(px(3.))
                        .border_1()
                        .border_color(border)
                        .bg(gpui_kit::rgba(fill)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .child(hex),
                )
                .child(div().text_xs().text_color(muted).child(count.to_string()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.copy(copy_hex.clone(), cx);
                }))
                .context_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (label, text) in menu_items.clone() {
                        let weak = weak.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| this.copy(text.clone(), cx));
                        }));
                    }
                    menu
                })
        });
        sections.push(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .pb_0p5()
                        .child(role_title(role)),
                )
                .children(rows)
                .into_any_element(),
        );
    }
    v_flex()
        .gap_3()
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("Click copies the hex; right-click for rgb(), oklch() and more."),
        )
        .children(sections)
        .into_any_element()
}

/// `0xRRGGBBAA`, for GPUI.
pub(super) fn rgba_u32(c: &Rgba) -> u32 {
    let a = (c.a.clamp(0., 1.) * 255.).round() as u32;
    u32::from(c.r) << 24 | u32::from(c.g) << 16 | u32::from(c.b) << 8 | a
}

#[cfg(test)]
mod tests {
    use super::{group_colors, rgb_css, rgba_u32, role_class, variable_line};
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
}
