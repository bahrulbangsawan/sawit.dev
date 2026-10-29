//! Shared look for empty, loading and error states, so no panel is left as
//! a blank space or a lone line of grey text.
//!
//! - [`Placeholder`] fills an area: icon badge, title, detail, actions.
//! - [`Banner`] is a one-line notice above content that is still usable.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Muted,
    Danger,
}

/// Centered state for an area with nothing (or nothing good) to show.
#[derive(IntoElement)]
pub struct Placeholder {
    icon: Icon,
    title: SharedString,
    detail: Option<SharedString>,
    tone: Tone,
    compact: bool,
    copyable: bool,
    spinner: bool,
    inline: bool,
    actions: Vec<AnyElement>,
}

impl Placeholder {
    pub fn new(icon: impl Into<Icon>, title: impl Into<SharedString>) -> Self {
        Self {
            icon: icon.into(),
            title: title.into(),
            detail: None,
            tone: Tone::Muted,
            compact: false,
            copyable: false,
            spinner: false,
            inline: false,
            actions: Vec::new(),
        }
    }

    /// Something failed: red badge, and the detail (the message) can be copied.
    pub fn error(title: impl Into<SharedString>) -> Self {
        Self {
            tone: Tone::Danger,
            copyable: true,
            ..Self::new(IconName::CircleAlert, title)
        }
    }

    /// Work in progress (a spinner, not an icon).
    pub fn loading(title: impl Into<SharedString>) -> Self {
        Self {
            spinner: true,
            ..Self::new(IconName::LoaderCircle, title)
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Smaller badge and text, for sidebars.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Only as tall as its content, for a state above other content.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for Placeholder {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let danger = self.tone == Tone::Danger;
        let tint = if danger {
            theme.danger
        } else {
            theme.muted_foreground
        };
        let badge = if self.compact { px(32.) } else { px(44.) };
        let icon = if self.spinner {
            spinner::Spinner::new().color(tint).into_any_element()
        } else {
            let icon = self.icon.text_color(tint);
            if self.compact { icon.small() } else { icon }.into_any_element()
        };
        let copy = self.detail.clone().filter(|_| self.copyable);

        v_flex()
            .when(!self.inline, |el| el.size_full().min_h(px(120.)))
            .when(self.inline, |el| el.w_full().py_6())
            .items_center()
            .justify_center()
            .p_4()
            .gap_2()
            .child(
                div()
                    .size(badge)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(tint.opacity(0.12))
                    .child(icon),
            )
            .child(
                div()
                    .text_center()
                    .font_medium()
                    .when(self.compact, |el| el.text_xs())
                    .when(!self.compact, |el| el.text_sm())
                    .text_color(if danger { theme.danger } else { theme.foreground })
                    .child(self.title),
            )
            .when_some(self.detail, |el, detail| {
                el.child(
                    div()
                        .max_w(px(if danger { 520. } else { 360. }))
                        .max_h(px(160.))
                        .overflow_hidden()
                        .text_xs()
                        .text_center()
                        .whitespace_normal()
                        .text_color(theme.muted_foreground)
                        .when(danger, |el| el.font_family(theme.mono_font_family.clone()))
                        .child(detail),
                )
            })
            .when(!self.actions.is_empty() || copy.is_some(), |el| {
                el.child(
                    h_flex()
                        .pt_1()
                        .gap_2()
                        .flex_wrap()
                        .justify_center()
                        .children(self.actions)
                        .when_some(copy, |el, text| {
                            el.child(
                                Button::new("placeholder-copy")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Copy)
                                    .label("Copy Error")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            text.to_string(),
                                        ))
                                    }),
                            )
                        }),
                )
            })
    }
}

/// One-line notice (error or info) above content that still works, with an
/// optional dismiss button. Long messages wrap.
#[derive(IntoElement)]
pub struct Banner {
    id: ElementId,
    message: SharedString,
    tone: Tone,
    actions: Vec<AnyElement>,
    on_dismiss: Option<OnClick>,
}

impl Banner {
    pub fn error(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            tone: Tone::Danger,
            actions: Vec::new(),
            on_dismiss: None,
        }
    }

    pub fn info(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            tone: Tone::Muted,
            ..Self::error(id, message)
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Banner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (tint, icon) = match self.tone {
            Tone::Danger => (theme.danger, IconName::CircleAlert),
            Tone::Muted => (theme.muted_foreground, IconName::Info),
        };
        h_flex()
            .id(self.id)
            .items_start()
            .gap_1p5()
            .mx_2()
            .my_1()
            .px_2()
            .py_1()
            .rounded(theme.radius)
            .border_1()
            .border_color(tint.opacity(0.35))
            .bg(tint.opacity(0.08))
            .text_xs()
            .child(
                div()
                    .pt(px(1.))
                    .flex_none()
                    .child(Icon::new(icon).xsmall().text_color(tint)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_normal()
                    .text_color(if self.tone == Tone::Danger {
                        theme.danger
                    } else {
                        theme.foreground
                    })
                    .child(self.message),
            )
            .children(self.actions)
            .when_some(self.on_dismiss, |el, handler| {
                el.child(
                    Button::new("banner-dismiss")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("Dismiss")
                        .on_click(handler),
                )
            })
    }
}
