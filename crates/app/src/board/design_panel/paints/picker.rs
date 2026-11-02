//! The colour pickers of the paint and shadow rows: the swatch that opens one, and while it is
//! open, the scrub of the list it colours, one undo step when it closes and none when Escape
//! closes it (ruling P5-C15).

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::effects::{EffectKind, effects, set_effect};
use sawit_board::design::paints::{parse_hex, set_solid};
use sawit_board::design::{DesignSelection, Edit};

use crate::board::BoardView;

/// What a colour picker colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::board::design_panel) enum ColourOf {
    /// Solid row `index` of list `key`, `fills` or `strokes`.
    Paint { key: &'static str, index: usize },
    /// The colour of shadow `index` of the effects.
    Shadow { index: usize },
}

impl ColourOf {
    /// The property a scrub of this colour changes.
    fn key(self) -> &'static str {
        match self {
            Self::Paint { key, .. } => key,
            Self::Shadow { .. } => "effects",
        }
    }

    /// The edit that gives this colour the hex `hex` and the opacity `opacity` on the layers of
    /// `sel`; `None` when they have no such solid paint or shadow.
    fn edit(self, sel: &DesignSelection, hex: &str, opacity: f64) -> Option<Edit> {
        match self {
            Self::Paint { key, index } => set_solid(sel, key, index, hex, opacity),
            Self::Shadow { index } => {
                let mut shadow = effects(sel)?.into_iter().nth(index)?;
                if !matches!(
                    shadow.kind,
                    EffectKind::DropShadow | EffectKind::InnerShadow
                ) {
                    return None;
                }
                shadow.hex = parse_hex(hex)?;
                shadow.opacity = opacity;
                Some(set_effect(sel, index, &shadow))
            }
        }
    }
}

/// A colour picker of the panel, and what it colours, kept from the first time it is drawn.
pub(in crate::board::design_panel) struct Picker {
    pub(super) state: Entity<ColorPickerState>,
    /// What it colours, as it was last drawn.
    of: ColourOf,
    _events: [Subscription; 2],
}

/// The swatch of colour `of` of row `id`, `hex` at `opacity`, drawn as `design-<id>-swatch`: a
/// click opens the colour picker. While the picker is open, the colours it goes through are one
/// scrub of `of`'s list ([`BoardView::picker_shown`]); while it is closed, it shows the layers'
/// colour.
pub(in crate::board::design_panel) fn colour_swatch(
    view: &mut BoardView,
    id: &str,
    of: ColourOf,
    hex: &str,
    opacity: f64,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let state = view.picker(id, of, window, cx);
    if view.design.colour.as_deref() == Some(id) {
        view.design.colour_drawn = true;
    }
    if let Some(colour) = colour_of(hex, opacity) {
        state.update(cx, |picker, cx| {
            if !picker.is_open() && picker.value() != Some(colour) {
                picker.set_value(colour, window, cx);
            }
        });
    }
    let selector = format!("design-{id}-swatch");
    div()
        .debug_selector(move || selector)
        .flex_shrink_0()
        .child(ColorPicker::new(&state).small())
        .into_any_element()
}

impl BoardView {
    /// Colour picker `id`, made the first time it is drawn, which colours `of`: the colours it
    /// picks go to the layers ([`Self::colour_picked`]), and its opening and closing start and end
    /// their scrub ([`Self::picker_shown`]).
    fn picker(
        &mut self,
        id: &str,
        of: ColourOf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ColorPickerState> {
        if let Some(picker) = self.design.pickers.get_mut(id) {
            picker.of = of;
            return picker.state.clone();
        }
        let id = SharedString::from(id.to_string());
        let state = cx.new(|cx| ColorPickerState::new(window, cx));
        let picked = cx.subscribe_in(&state, window, {
            let id = id.clone();
            move |view, _, event: &ColorPickerEvent, _, cx| {
                if let ColorPickerEvent::Change(Some(colour)) = event {
                    view.colour_picked(&id, *colour, cx);
                }
            }
        });
        let shown = cx.observe_in(&state, window, {
            let id = id.clone();
            move |view, state, window, cx| {
                let open = state.read(cx).is_open();
                view.picker_shown(&id, open, window, cx);
            }
        });
        let picker = Picker {
            state: state.clone(),
            of,
            _events: [picked, shown],
        };
        self.design.pickers.insert(id, picker);
        state
    }

    /// Colour picker `id` is `open`, or closed. Opening it starts a scrub of the list it colours,
    /// so that every colour it goes through is one undo step (ruling P5-C15); closing it ends the
    /// scrub, kept. Escape ends it first, put back ([`Self::cancel_colour_scrub`]). A value typed
    /// in a field, which the click on the swatch took the keyboard from, is applied before the
    /// scrub begins, as an undo step of its own, and so is a text being edited on the canvas
    /// ([`BoardView::scrub_begin`]). When that leaves no layer selected (the text was left
    /// empty, and went), the picker closes again: there is nothing for it to colour.
    pub(super) fn picker_shown(
        &mut self,
        id: &str,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scrubbing = self.design.colour.as_deref() == Some(id);
        if open && !scrubbing {
            let Some(of) = self.design.pickers.get(id).map(|picker| picker.of) else {
                return;
            };
            self.commit_focused_field(window, cx);
            self.scrub_begin(of.key(), cx);
            if self.scrub.is_some() {
                self.design.colour = Some(id.to_string().into());
            } else if self.design_selection().is_none_or(|sel| sel.ids.is_empty())
                && let Some(picker) = self.design.pickers.get(id)
            {
                picker
                    .state
                    .update(cx, |picker, cx| picker.set_open(false, cx));
            }
        } else if !open && scrubbing {
            self.scrub_end(true, cx);
        }
    }

    /// Colour picker `id` picked `colour`: what it colours takes it, on the selected layers, as
    /// they are now (within the picker's scrub while it is open).
    fn colour_picked(&mut self, id: &str, colour: Hsla, cx: &mut Context<Self>) {
        let Some(of) = self.design.pickers.get(id).map(|picker| picker.of) else {
            return;
        };
        let (hex, opacity) = hex_and_opacity(colour);
        self.edit_layers(|sel| of.edit(sel, &hex, opacity), cx);
    }

    /// Escape closes the colour picker that is open: the colours it went through go back, and no
    /// undo step is made (ruling P5-C15). Without a colour picker open it does nothing.
    pub(in crate::board::design_panel) fn cancel_colour_scrub(&mut self, cx: &mut Context<Self>) {
        if self.design.colour.is_some() {
            self.scrub_end(false, cx);
        }
    }

    /// Ends the scrub of the colour picker that is open, kept, as one undo step, and closes the
    /// picker. Without a colour picker open it does nothing.
    ///
    /// Input on its way to the engine while a picker is open runs this first, so that it isn't
    /// taken into the colour's undo step: a press on the canvas, which closes the picker only once
    /// the press has reached the tools (a gesture it starts would find the colour's interaction in
    /// place of its own), and an agent's op ([`BoardView::run_op`]; an Escape in the picker would
    /// take back what the op did).
    pub(in crate::board) fn end_colour_scrub(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.design.colour.clone() else {
            return;
        };
        self.scrub_end(true, cx);
        if let Some(picker) = self.design.pickers.get(&id) {
            picker
                .state
                .update(cx, |picker, cx| picker.set_open(false, cx));
        }
    }

    /// Runs after the Design panel is drawn, or isn't because it is hidden: a colour scrub whose
    /// picker this frame didn't draw for the layers the scrub began on (other layers are
    /// selected, the panel is hidden) ends, kept, and its picker closes, once the frame is drawn
    /// ([`Self::end_colour_scrub`]). Nothing else would end it: every later edit would join its
    /// undo step, and the picker would show open again with its swatch.
    pub(in crate::board) fn settle_colour_scrub(&mut self, cx: &mut Context<Self>) {
        let drawn = std::mem::take(&mut self.design.colour_drawn);
        let Some(id) = self.design.colour.clone() else {
            return;
        };
        let shown = self.design_selection();
        let same_layers = match (&self.scrub, shown) {
            (Some(scrub), Some(shown)) => scrub.sel.ids == shown.ids,
            _ => false,
        };
        if drawn && same_layers {
            return;
        }
        // Not while the frame is drawn: the commit redraws the canvas.
        let this = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = this.update(cx, |view, cx| {
                if view.design.colour.as_ref() == Some(&id) {
                    view.end_colour_scrub(cx);
                }
            });
        });
    }
}

/// The hex and opacity a paint takes for `colour`: `#RRGGBB`, and its alpha to a hundredth.
pub(super) fn hex_and_opacity(colour: Hsla) -> (String, f64) {
    let rgba = colour.to_rgb();
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    let hex = format!(
        "#{:02X}{:02X}{:02X}",
        byte(rgba.r),
        byte(rgba.g),
        byte(rgba.b)
    );
    (hex, (f64::from(rgba.a) * 100.0).round() / 100.0)
}

/// The colour the picker holds for `hex` at `opacity`. `None` when `hex` is no colour.
pub(super) fn colour_of(hex: &str, opacity: f64) -> Option<Hsla> {
    let hex = parse_hex(hex)?;
    let channel = |at: usize| {
        let byte = u8::from_str_radix(hex.get(at..at + 2)?, 16).ok()?;
        Some(f32::from(byte) / 255.0)
    };
    let rgba = Rgba {
        r: channel(1)?,
        g: channel(3)?,
        b: channel(5)?,
        a: opacity.clamp(0.0, 1.0) as f32,
    };
    Some(rgba.into())
}
