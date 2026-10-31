//! A field that records keystrokes: while it has the keyboard, every key pressed is written down
//! instead of doing what it is bound to (⌘W does not close the tab), up to [`MAX_KEYS`] for a
//! chord. The keys it shows (a binding's, when the form opens on one) stay until the first key
//! pressed after it gets the keyboard, which starts a new recording; a click starts it again.
//!
//! Plain ⎋ and ↩ are the field's own: they end the recording ([`Done`]) and are never recorded.
//! A field that stops being drawn while it has the keyboard (its tab switched away) lets go of
//! it, so that the window's keys work again.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::names;

/// The most keys a chord may have.
pub const MAX_KEYS: usize = 3;

/// The keys that only modify another: pressed alone they are not recorded.
const MODIFIER_KEYS: &[&str] = &[
    "shift", "control", "ctrl", "alt", "option", "cmd", "platform", "function", "fn",
];

/// Says the keys changed.
pub struct Changed;

/// Says the recording is over, by a key of the field's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    /// A plain ⎋: the field's owner takes the keyboard back.
    Stopped,
    /// A plain ↩: the keys are the ones wanted.
    Confirmed,
}

pub struct KeystrokeRecorder {
    focus: FocusHandle,
    keys: Vec<Keystroke>,
    /// The next key starts a new recording instead of adding to the chord: the keys shown were
    /// set from outside, or the field has just got the keyboard.
    fresh: bool,
    /// Takes the keys from the bindings while the field has the keyboard; starts a new
    /// recording when it gets the keyboard; lets go of it when the field is no longer drawn.
    _subscriptions: [Subscription; 3],
    placeholder: SharedString,
}

impl EventEmitter<Changed> for KeystrokeRecorder {}
impl EventEmitter<Done> for KeystrokeRecorder {}

/// The recorders' keyboards, for [`is_recorder`].
#[derive(Default)]
struct Recorders(Vec<WeakFocusHandle>);

impl Global for Recorders {}

/// Whether `focus` is a recorder's: a field that takes every key while it has the keyboard, so
/// that only a click gives it the keyboard back once it went off screen (the workspace's
/// [`crate::workspace::KeyboardHome`] doesn't): given back, a ⌃Tab cycle would stop at it, and
/// ⌘W be recorded instead of closing the tab.
pub fn is_recorder(focus: &FocusHandle, cx: &App) -> bool {
    cx.try_global::<Recorders>().is_some_and(|recorders| {
        recorders
            .0
            .iter()
            .any(|r| r.upgrade().as_ref() == Some(focus))
    })
}

impl KeystrokeRecorder {
    pub fn new(
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let recorders = cx.default_global::<Recorders>();
        recorders.0.retain(|r| r.upgrade().is_some());
        recorders.0.push(focus.downgrade());
        let this = cx.weak_entity();
        let watched = focus.clone();
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            if !watched.is_focused(window) {
                return;
            }
            let keystroke = event.keystroke.clone();
            if MODIFIER_KEYS.contains(&keystroke.key.as_str()) {
                return;
            }
            cx.stop_propagation();
            let done = match keystroke.key.as_str() {
                _ if keystroke.modifiers.modified() => None,
                "escape" => Some(Done::Stopped),
                "enter" => Some(Done::Confirmed),
                _ => None,
            };
            this.update(cx, |this, cx| match done {
                Some(done) => cx.emit(done),
                None => this.record(keystroke, cx),
            })
            .ok();
        });
        let focus_watch = cx.on_focus(&focus, window, |this, _, _| this.fresh = true);
        // The keyboard is still the field's when it is no longer drawn (its tab was switched
        // away): it would take every key of the window, unseen. The window's view takes it back
        // once nothing holds it. (A window that is no longer the active one keeps it.)
        let blur_watch = cx.on_blur(&focus, window, |this, window, cx| {
            if this.focus.is_focused(window) && window.is_window_active() {
                window.blur(cx);
            }
        });
        Self {
            focus,
            keys: Vec::new(),
            fresh: true,
            _subscriptions: [intercept, focus_watch, blur_watch],
            placeholder: placeholder.into(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The keys, as the keymap file writes them (`cmd-k e`).
    pub fn keystrokes(&self) -> String {
        self.keys
            .iter()
            .map(|k| k.unparse(OperatingSystem::Mac))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Shows `text`'s keys (as the keymap file writes them), if they can be read, until the first
    /// key pressed replaces them.
    pub fn set_keystrokes(&mut self, text: &str, cx: &mut Context<Self>) {
        self.keys = text
            .split_whitespace()
            .filter_map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).ok())
            .collect();
        self.fresh = true;
        cx.emit(Changed);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.keys.clear();
        cx.emit(Changed);
        cx.notify();
    }

    /// A key was pressed: it starts a new recording when the field has just got the keyboard
    /// (or the chord is full), else it adds to the chord.
    fn record(&mut self, mut keystroke: Keystroke, cx: &mut Context<Self>) {
        // The keymap matches the key, not the character it types.
        keystroke.key_char = None;
        if std::mem::take(&mut self.fresh) || self.keys.len() >= MAX_KEYS {
            self.keys.clear();
        }
        self.keys.push(keystroke);
        cx.emit(Changed);
        cx.notify();
    }
}

impl Focusable for KeystrokeRecorder {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for KeystrokeRecorder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let recording = self.focus.is_focused(window);
        let theme = cx.theme();
        let text = if self.keys.is_empty() {
            None
        } else {
            Some(names::display_typed(&self.keys))
        };
        h_flex()
            .id("keystroke-recorder")
            .track_focus(&self.focus)
            .h_8()
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(if recording { theme.ring } else { theme.input })
            .bg(theme.background)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                this.fresh = true;
                window.focus(&this.focus, cx);
                cx.notify();
            }))
            // The keys are taken before the bindings run; this keeps them from the views around.
            .on_key_down(|_, _, cx| cx.stop_propagation())
            .child(
                Icon::new(Lucide::Keyboard)
                    .small()
                    .text_color(if recording {
                        theme.ring
                    } else {
                        theme.muted_foreground
                    }),
            )
            .child(div().flex_1().text_sm().map(|el| match &text {
                Some(text) => el.font_family("monospace").child(text.clone()),
                None => el.text_color(theme.muted_foreground).child(if recording {
                    "Press the keys…".into()
                } else {
                    self.placeholder.clone()
                }),
            }))
            .when(recording, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Recording"),
                )
            })
            .when(!self.keys.is_empty(), |el| {
                el.child(
                    Button::new("clear-keys")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .tooltip("Clear")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.clear(cx);
                            window.focus(&this.focus, cx);
                        })),
                )
            })
    }
}
