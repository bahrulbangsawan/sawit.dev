//! Tests of the annotations: the shared fixtures here, the tests by topic in the submodules.

mod markdown;
mod naming;
mod payload;

use super::*;

fn element() -> ElementDetails {
    ElementDetails {
        selector: "main > div > button.btn-primary".into(),
        label: "button.btn-primary".into(),
        tag: "button".into(),
        id: None,
        classes: vec!["btn-primary".into()],
        text: Some("Submit order".into()),
        rect: Rect {
            x: 350.,
            y: 680.,
            width: 120.,
            height: 40.,
        },
        styles: vec![
            ("display".into(), "flex".into()),
            ("background-color".into(), "rgb(34, 197, 94)".into()),
            ("font-size".into(), "14px".into()),
        ],
        source: Some(SourceHint {
            file: "src/components/Checkout.tsx".into(),
            line: Some(42),
            framework: "React".into(),
            component: Some("Checkout".into()),
        }),
        html: None,
        off_screen: false,
        ..ElementDetails::default()
    }
}

fn annotation() -> Annotation {
    Annotation {
        n: 1,
        url: "http://localhost:5173/cart".into(),
        title: "Cart".into(),
        element: element(),
        comment: "The button should use the brand blue, not green.".into(),
        intent: Intent::Fix,
        screenshot: Some("/p/.nyawit/annotations/2026-10-29/localhost-5173-1.png".into()),
        include_screenshot: true,
    }
}

fn page() -> PageInfo {
    PageInfo {
        title: "Cart".into(),
        url: "http://localhost:5173/cart".into(),
        viewport: (393, 852),
        device: Some("iPhone 15".into()),
    }
}

fn group(items: Vec<Annotation>) -> PageGroup {
    PageGroup {
        page: page(),
        items,
    }
}

/// A lone annotation with nothing optional filled in.
fn bare() -> Annotation {
    let mut a = annotation();
    a.comment = String::new();
    a.element.text = None;
    a.element.source = None;
    a.screenshot = None;
    a
}
