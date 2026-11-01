//! The pick the page's overlay sends, as it deserializes.

use super::*;

#[test]
fn picked_payload_deserializes() {
    let json = r##"{"selector":"#a","label":"button#a","tag":"button","id":"a","classes":[],"text":"Go","rect":{"x":1,"y":2,"width":3,"height":4},"styles":[["display","flex"]],"source":{"file":"src/A.tsx","line":7,"framework":"React","component":"A"},"html":"<button id=\"a\">Go</button>","off_screen":false}"##;
    let d: ElementDetails = serde_json::from_str(json).unwrap();
    assert_eq!(d.styles[0], ("display".to_string(), "flex".to_string()));
    assert_eq!(d.source.unwrap().line, Some(7));
    assert!(!d.off_screen);
}

/// What the overlay now sends for a pick (`browser_overlay.js` `details`), field for field.
#[test]
fn full_picked_payload_deserializes() {
    let json = r##"{"selector":"#shop-now","label":"button#shop-now.btn","tag":"button","id":"shop-now",
            "classes":["btn","btn-primary"],"text":"Shop the harvest","rect":{"x":32,"y":412,"width":180,"height":40},
            "styles":[["display","inline-block"]],"source":null,"html":"<button>","off_screen":false,
            "path":"body > main > button#shop-now","attributes":[["data-testid","hero-cta"],["type","submit"]],
            "role":"button","name":"Shop the harvest","page_rect":{"x":32,"y":1212,"width":180,"height":40},
            "viewport":[1180,760],"context":{"parent":{"label":"div.actions","text":null},
            "before":null,"after":{"label":"button.btn.btn-ghost","text":"Read the farm notes"}}}"##;
    let d: ElementDetails = serde_json::from_str(json).unwrap();
    assert_eq!(d.attributes[0], ("data-testid".into(), "hero-cta".into()));
    assert_eq!(d.role.as_deref(), Some("button"));
    assert_eq!(d.page_rect.as_ref().map(|r| r.y), Some(1212.));
    assert_eq!(d.viewport, Some((1180, 760)));
    let cx = d.context.unwrap();
    assert_eq!(cx.parent.unwrap().label, "div.actions");
    assert!(cx.before.is_none());
    assert_eq!(
        cx.after.unwrap().text.as_deref(),
        Some("Read the farm notes")
    );
}
