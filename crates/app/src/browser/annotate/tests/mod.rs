//! The Annotate panel's tests, by what they test, with the notes, panel and agents they share.

mod labels;
mod rows;
mod send;

use super::AnnotatePanel;
use crate::terminal::RunningAgent;
use sawit_core::annotations::{Annotation, ElementDetails, Intent, Rect};
use sawit_core::terminal::AgentKind;

fn at(url: &str, n: u32) -> Annotation {
    Annotation {
        n,
        url: url.into(),
        title: String::new(),
        element: ElementDetails {
            selector: "a".into(),
            label: "a".into(),
            tag: "a".into(),
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            },
            ..ElementDetails::default()
        },
        comment: String::new(),
        intent: Intent::Fix,
        screenshot: None,
        include_screenshot: true,
    }
}

fn panel(
    cx: &mut gpui_kit::TestAppContext,
) -> (
    gpui_kit::Entity<AnnotatePanel>,
    &mut gpui_kit::VisualTestContext,
) {
    use gpui_kit::AppContext as _;
    cx.update(gpui_kit::init);
    let cx = cx.add_empty_window();
    let panel = cx.update(|window, cx| cx.new(|cx| AnnotatePanel::new(window, cx)));
    (panel, cx)
}

fn agent(pane: u64, kind: AgentKind, focused: bool) -> RunningAgent {
    RunningAgent {
        tab: gpui_kit::WeakEntity::new_invalid(),
        pane,
        kind,
        label: format!("project-{pane}").into(),
        focused,
    }
}
