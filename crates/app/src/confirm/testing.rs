//! For tests elsewhere: the confirm question up in a window, and answering it by its button, as
//! `simulate_prompt_answer` does for `window.prompt`.

use gpui_kit::*;

use super::ConfirmView;

/// Every question asked, by window (tests only).
#[derive(Default)]
pub struct Asked(pub Vec<(AnyWindowHandle, WeakEntity<ConfirmView>)>);

impl Global for Asked {}

/// The questions still waiting, oldest first.
fn waiting(cx: &mut TestAppContext) -> Vec<(AnyWindowHandle, Entity<ConfirmView>)> {
    cx.update(|cx| {
        cx.try_global::<Asked>()
            .map(|asked| {
                asked
                    .0
                    .iter()
                    .filter_map(|(w, v)| Some((*w, v.upgrade()?)))
                    .filter(|(_, v)| v.read(cx).waiting())
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The title and detail of the question waiting, if one is.
pub fn pending(cx: &mut TestAppContext) -> Option<(String, String)> {
    let (_, view) = waiting(cx).pop()?;
    cx.update(|cx| {
        let c = &view.read(cx).confirm;
        Some((
            c.title.to_string(),
            c.detail.as_ref().map(|d| d.to_string()).unwrap_or_default(),
        ))
    })
}

/// Whether a question is waiting.
pub fn has_pending(cx: &mut TestAppContext) -> bool {
    !waiting(cx).is_empty()
}

/// Presses `button` of the question waiting: its verb, its second verb, or anything else for
/// Cancel. Panics when no question waits.
pub fn answer(cx: &mut TestAppContext, button: &str) {
    let (window, view) = waiting(cx).pop().expect("no confirm question is up");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |v, cx| {
            if v.confirm.verb.as_ref() == button {
                v.go(0, window, cx)
            } else if v.confirm.second.as_deref() == Some(button) {
                v.go(1, window, cx)
            } else {
                v.cancel(window, cx)
            }
        })
    })
    .unwrap();
    cx.run_until_parked();
}
