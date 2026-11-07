//! The browser's tab strip: each tab's title (cut short with its whole title in a tooltip) and its
//! close button, the tab on show marked, tabs dragged to reorder, and New Tab at the end.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{BrowserTabs, CloseTab, NewTab};

/// A favicon's side, the size of the strip's other icons.
const ICON_SIDE: f32 = 14.;
/// The strip's height.
const STRIP_HEIGHT: f32 = 30.;
/// A tab's width bounds.
const TAB_MIN: f32 = 96.;
const TAB_MAX: f32 = 200.;

/// A tab being dragged, by its place.
#[derive(Clone)]
struct DraggedTab {
    from: usize,
    title: SharedString,
}

/// What follows the pointer while a tab is dragged.
struct DragLabel(SharedString);

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_xs()
            .child(self.0.clone())
    }
}

/// The tab's icon: a spinner while the page loads, then its favicon, or a globe when it has none,
/// in a box of one size so the title doesn't move when it changes.
fn tab_icon(loading: bool, favicon: Option<Arc<RenderImage>>, muted: Hsla) -> AnyElement {
    let slot = div()
        .size(px(ICON_SIDE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();
    match (loading, favicon) {
        (false, Some(image)) => slot.child(
            img(image)
                .size(px(ICON_SIDE))
                .object_fit(ObjectFit::Contain),
        ),
        (loading, _) => slot.child(
            Icon::new(if loading {
                Lucide::LoaderCircle
            } else {
                Lucide::Globe
            })
            .xsmall()
            .text_color(muted),
        ),
    }
    .into_any_element()
}

impl BrowserTabs {
    pub(super) fn strip(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let v = tab.view.read(cx);
            let title: SharedString = match v.title.trim() {
                "" => match v.current_url().as_str() {
                    "about:blank" | "" => "New Tab".into(),
                    url => url.to_string().into(),
                },
                title => title.to_string().into(),
            };
            let active = ix == self.active;
            let loading = v.loading;
            let favicon = v.favicon();
            let tip = title.clone();
            let dragged = DraggedTab {
                from: ix,
                title: title.clone(),
            };
            h_flex()
                .id(("browser-tab", tab.id as usize))
                .debug_selector(move || format!("browser-tab-{ix}"))
                .role(Role::Tab)
                .aria_label(title.clone())
                .aria_selected(active)
                .flex_1()
                .min_w(px(TAB_MIN))
                .max_w(px(TAB_MAX))
                .h_full()
                .pl_2()
                .pr_0p5()
                .gap_1()
                .text_xs()
                .cursor_pointer()
                .border_r_1()
                .border_color(border)
                .when(active, |el| {
                    el.bg(cx.theme().tab_active)
                        .text_color(cx.theme().tab_active_foreground)
                })
                .when(!active, |el| {
                    el.bg(cx.theme().tab).text_color(cx.theme().tab_foreground)
                })
                .child(tab_icon(loading, favicon, muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title.clone()),
                )
                .child(
                    Button::new(("browser-tab-close", tab.id as usize))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .accessibility_label(format!("Close {title}"))
                        .tooltip_with_action("Close Tab", &CloseTab, Some("Browser"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(ix, window, cx);
                        })),
                )
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| this.activate(ix, cx)))
                .on_drag(dragged, |dragged: &DraggedTab, _, _, cx| {
                    cx.new(|_| DragLabel(dragged.title.clone()))
                })
                .drag_over::<DraggedTab>(|el, _, _, cx| el.bg(cx.theme().accent))
                .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                    this.move_tab(dragged.from, ix, cx);
                }))
        });
        h_flex()
            .id("browser-tabs")
            .role(Role::TabList)
            .aria_label("Browser Tabs")
            .h(px(STRIP_HEIGHT))
            .flex_none()
            .bg(cx.theme().tab_bar)
            .border_b_1()
            .border_color(border)
            .overflow_x_scroll()
            .children(tabs)
            .child(
                Button::new("browser-new-tab")
                    .ghost()
                    .xsmall()
                    .mx_1()
                    .icon(Lucide::Plus)
                    .accessibility_label("New Tab")
                    .tooltip_with_action("New Tab", &NewTab, Some("Browser"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let view = this.open_tab(None, true, window, cx);
                        view.update(cx, |v, cx| v.focus_address(window, cx));
                    })),
            )
    }
}
