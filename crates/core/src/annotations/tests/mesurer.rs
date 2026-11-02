//! mesurer's comments and markup in the notes: threads read from a workspace, Comment rows and the
//! page's markup as an agent gets them, and Sawit's changes to a workspace.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::annotations::mesurer::edit::{self, Taken};
use crate::annotations::mesurer::{element_of, threads_of};

/// A page's artifacts as mesurer saves them: three threads (one with a reply, one resolved, one
/// left in a 390×844 preview) and one of every other kind of markup.
fn artifacts() -> Value {
    let target = |selector: &str, tag: &str, extra: Value| {
        let mut t = json!({
            "selector": selector, "tagName": tag, "textSnippet": "Fresh from the farm",
            "htmlPreview": "<h2 class=\"title\">Fresh from the farm</h2>",
            "attributes": {"class": "title big", "id": "fresh"},
            "styles": "display: block\nfont: 700 32px Inter\ncolor: rgb(0, 0, 0)",
            "rect": {"left": 40, "top": 120, "width": 320, "height": 48},
            "documentPoint": {"x": 60, "y": 130},
            "url": "http://localhost:3000/shop", "viewport": {"width": 1280, "height": 800},
        });
        for (k, v) in extra.as_object().unwrap() {
            t[k] = v.clone();
        }
        t
    };
    let message = |text: &str, at: f64| json!({"id": format!("m{at}"), "role": "user", "text": text, "createdAt": at});
    json!({
        "guides": [{"id": "g1", "orientation": "vertical", "position": 120.4},
                   {"id": "g2", "orientation": "horizontal", "position": 300}],
        "selectedGuideIds": [],
        "arrows": [{"id": "a1", "start": {"x": 10, "y": 20}, "end": {"x": 300, "y": 400},
                    "control": {"x": 100, "y": 50}, "color": "#f00", "width": 2}],
        "selectedArrowIds": [],
        "penStrokes": [{"id": "p1", "kind": "marker", "color": "#ff0", "width": 6,
                        "points": [{"x": 10, "y": 20}, {"x": 130, "y": 100}]}],
        "textAnnotations": [{"id": "t1", "x": 120, "y": 340, "text": "Make `this` bigger\nnow"}],
        "measurements": [{"id": "m1", "rect": {"left": 10, "top": 20, "width": 120, "height": 40},
                          "normalizedRect": {"left": 10, "top": 20, "width": 120, "height": 40},
                          "deltaX": 0, "deltaY": 0}],
        "activeMeasurement": null,
        "heldDistances": [{"id": "d1", "horizontal": {"x1": 0, "x2": 24, "y": 5, "value": 24},
                           "vertical": null}],
        "layoutGuides": [{"id": "l1", "kind": "columns", "visible": true, "color": "#FF0000",
                          "opacity": 0.1, "count": 12, "size": 72, "gutter": 20, "offset": 0,
                          "align": "center"}],
        "comments": [
            {"id": "c1", "status": "open", "createdAt": 1, "updatedAt": 2,
             "target": target("section#three > h2", "h2", json!({})),
             "messages": [message("Tighten this heading", 1.0)]},
            {"id": "c2", "status": "open", "createdAt": 1, "updatedAt": 2,
             "target": target("a.cta", "a", json!({"attributes": {"href": "/buy"}})),
             "messages": [message("The button is too pale", 1.0), message("Use the brand green\nlike the logo", 2.0)]},
            {"id": "c3", "status": "resolved", "createdAt": 1, "updatedAt": 2,
             "target": target("footer", "footer", json!({})),
             "messages": [message("Done already", 1.0)]},
            {"id": "c4", "status": "open", "createdAt": 1, "updatedAt": 2,
             "target": target("nav > a", "a", json!({"preview": true, "viewport": {"width": 390, "height": 844}, "framePath": [],
                 "styles": "display: block\nwidth: 72.8px\nheight: 54px", "rect": {"left": 346, "top": 222, "width": 20, "height": 15}})),
             "messages": [message("The menu wraps on phones", 1.0)]},
            {"id": "", "status": "open", "target": {}, "messages": [message("no id", 1.0)]},
        ],
    })
}

fn comment_rows() -> Vec<Annotation> {
    threads_of(&artifacts(), "http_localhost_3000", "/shop")
        .into_iter()
        .enumerate()
        .map(|(i, (thread, element))| Annotation {
            n: i as u32 + 2,
            url: "http://localhost:3000/shop".into(),
            title: "Shop".into(),
            element,
            comment: thread.first().to_string(),
            intent: Intent::Fix,
            screenshot: None,
            include_screenshot: true,
            source: Source::Comment(thread),
        })
        .collect()
}

#[test]
fn threads_are_read_from_a_page_with_their_elements() {
    let threads = threads_of(&artifacts(), "http_localhost_3000", "/shop");
    let ids: Vec<&str> = threads.iter().map(|(t, _)| t.id.as_str()).collect();
    assert_eq!(
        ids,
        ["c1", "c2", "c3", "c4"],
        "one without an id is left out"
    );
    let (c2, element) = &threads[1];
    assert_eq!(c2.messages.len(), 2);
    assert_eq!(c2.messages[1].created_at, 2.0);
    assert_eq!(
        (c2.origin.as_str(), c2.page.as_str()),
        ("http_localhost_3000", "/shop")
    );
    assert!(c2.in_document());
    assert_eq!(element.selector, "a.cta");
    assert_eq!(element.label, "a");
    assert_eq!(element.attributes, [("href".into(), "/buy".into())]);
    let (c3, _) = &threads[2];
    assert!(c3.resolved());
    let (c4, _) = &threads[3];
    assert_eq!(c4.preview, Some((390, 844)));
    assert!(!c4.in_document());
    // Nothing there: no threads.
    assert!(threads_of(&Value::Null, "o", "/").is_empty());

    let element = element_of(&artifacts()["comments"][0]["target"]);
    assert_eq!(element.label, "h2#fresh.title.big");
    assert_eq!(element.id.as_deref(), Some("fresh"));
    assert_eq!(element.classes, ["title", "big"]);
    assert_eq!(element.styles[1], ("font".into(), "700 32px Inter".into()));
    assert_eq!((element.rect.x, element.rect.width), (40., 320.));
    assert_eq!(element.viewport, Some((1280, 800)));
    assert_eq!(element.html, None, "the page's markup stays out");
}

/// A page with a note, Comment rows (one message, one with a reply, a resolved one left out, one
/// in a Responsive preview) and every kind of markup, its elements named.
#[test]
fn comments_and_markup_go_to_the_agent_with_the_notes() {
    let mut items = vec![annotation()];
    items[0].url = "http://localhost:3000/shop".into();
    items.extend(comment_rows());
    let mut markup = PageMarkup::of(&artifacts(), true);
    let near = |selector: &str| {
        Some(Near {
            selector: selector.into(),
            label: String::new(),
        })
    };
    let found: HashMap<String, Option<Near>> = markup
        .points()
        .into_iter()
        .zip([near("h2.`title`"), near("a.cta"), None, near("div.card")])
        .map(|(p, n)| (p.key, n))
        .collect();
    markup.place(&found);
    markup.captures = vec!["/p/.nyawit/browser/captures/shot-1.png".into()];
    let group = PageGroup {
        page: PageInfo {
            title: "Shop".into(),
            url: "http://localhost:3000/shop".into(),
            viewport: (1280, 800),
            device: None,
        },
        items,
        markup: Some(markup),
    };
    let md = markdown(&[group], None);
    let want = "\
# Visual feedback — Shop
**URL:** http://localhost:3000/shop · **Route:** `/shop` · **Viewport:** 1280×800

## 1. `button.btn-primary` — fix
- **Comment:** The button should use the brand blue, not green.
- **Selector:** `main > div > button.btn-primary`
- **Element:** `<button class=\"btn-primary\">` \"Submit order\"
- **Source:** `src/components/Checkout.tsx:42` (React, <Checkout>)
- **Box:** 120×40 at (350, 680)
- **Styles:** display: flex; background-color: rgb(34, 197, 94); font-size: 14px
- **Screenshot:** /p/.nyawit/annotations/2026-10-29/localhost-5173-1.png

## 2. `h2#fresh.title.big` — Comment
- **Comment:** Tighten this heading
- **Selector:** `section#three > h2`
- **Element:** `<h2 id=\"fresh\" class=\"title big\">` \"Fresh from the farm\"
- **Box:** 320×48 at (40, 120)
- **Styles:** display: block; font: 700 32px Inter; color: rgb(0, 0, 0)

## 3. `a` — Comment
- **Comment:** The button is too pale
  - **Reply:** Use the brand green
    like the logo
- **Selector:** `a.cta`
- **Element:** `<a href=\"/buy\">` \"Fresh from the farm\"
- **Box:** 320×48 at (40, 120)
- **Styles:** display: block; font: 700 32px Inter; color: rgb(0, 0, 0)

## 5. `a#fresh.title.big` — Comment
- **Comment:** The menu wraps on phones
- **Selector:** `nav > a`
- **Element:** `<a id=\"fresh\" class=\"title big\">` \"Fresh from the farm\"
- **Box:** 73×54, in a 390×844 Responsive preview
- **Styles:** display: block; width: 72.8px; height: 54px

## Page markup (Mesurer)
Positions are CSS px from the page's top left; guides, from the viewport's.
- **Text notes (1):**
  - ``Make `this` bigger now`` at (120, 340) on `` h2.`title` ``
- **Arrows (1):**
  - from (10, 20) on `a.cta` to (300, 400) (curved)
- **Pen strokes (1):**
  - marker, 120×80 at (10, 20), around `div.card`
- **Pinned distances (1):**
  - 24 px across
- **Measurements (1):**
  - 120×40 at (10, 20)
- **Guides (2):** vertical at x 120, horizontal at y 300
- **Layout guides (1):**
  - 12 columns, 72 px wide, 20 px gutter, 0 px offset, align center
- **X-ray:** on (every element's box outlined)
- **Captures (1):**
  - /p/.nyawit/browser/captures/shot-1.png
";
    assert_eq!(md, want);
    assert!(
        !md.contains("Done already"),
        "a resolved thread is left out"
    );
    assert!(
        !md.contains("<h2 class"),
        "a comment's HTML preview stays out"
    );
}

/// A page whose markup holds nothing has no section; the stretch columns name no width.
#[test]
fn markup_with_nothing_drawn_has_no_section() {
    let empty = PageMarkup::of(&json!({"guides": []}), false);
    assert!(empty.is_empty());
    assert!(empty.points().is_empty());
    let group = PageGroup {
        markup: Some(empty),
        ..group(vec![annotation()])
    };
    assert!(!markdown(&[group], None).contains("Page markup"));
    let stretch = PageMarkup::of(
        &json!({"layoutGuides": [{"kind": "rows", "count": 4, "size": 72, "gutter": 8,
                                  "offset": 16, "align": "stretch", "visible": false},
                                 {"kind": "grid", "size": 8}]}),
        false,
    );
    let md = stretch.markdown();
    assert!(
        md.contains("  - 4 rows, 8 px gutter, 16 px offset, align stretch (hidden)\n"),
        "{md}"
    );
    assert!(md.contains("  - grid, 8 px squares\n"), "{md}");
}

/// An arrow is curved when its control point is off its line: mesurer keeps the midpoint of a
/// straight one.
#[test]
fn a_straight_arrow_isnt_called_curved() {
    let arrow = |control: Value| {
        json!({"arrows": [{"id": "a", "start": {"x": 0, "y": 0}, "end": {"x": 100, "y": 100},
            "control": control, "color": "#f00", "width": 2}]})
    };
    let curved = |page: Value| PageMarkup::of(&page, false).arrows[0].curved;
    assert!(!curved(arrow(json!({"x": 50, "y": 50}))));
    assert!(!curved(arrow(json!({"x": 51, "y": 50}))), "within a pixel");
    assert!(curved(arrow(json!({"x": 80, "y": 20}))));
    assert!(!curved(arrow(Value::Null)));
}

/// Points move with their marks: an answer kept for where a mark was isn't used where it is now.
#[test]
fn a_moved_mark_is_named_again() {
    let mut markup = PageMarkup::of(&artifacts(), false);
    let keys: Vec<String> = markup.points().into_iter().map(|p| p.key).collect();
    assert_eq!(keys[0], "text:t1@120,340");
    assert_eq!(keys[3], "stroke:p1@70,60");
    let mut found = HashMap::new();
    found.insert(
        "text:t1@999,340".to_string(),
        Some(Near {
            selector: "body".into(),
            label: String::new(),
        }),
    );
    markup.place(&found);
    assert_eq!(markup.texts[0].near, None);
}

#[test]
fn a_thread_taken_out_goes_back_where_it_was() {
    let mut envelope = json!({"enabled": true, "toolMode": "none", "rulersVisible": false,
        "guideOrientation": "vertical", "pages": {"/shop": artifacts()}});
    let taken = edit::take_thread(&mut envelope, "/shop", "c2").expect("there");
    assert_eq!(taken.index, 1);
    let ids = |e: &Value| -> Vec<String> {
        e["pages"]["/shop"]["comments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids(&envelope), ["c1", "c3", "c4", ""]);
    assert_eq!(edit::take_thread(&mut envelope, "/shop", "c2"), None);
    edit::put_thread(&mut envelope, "/shop", taken.clone());
    assert_eq!(ids(&envelope), ["c1", "c2", "c3", "c4", ""]);
    // Put back twice: once.
    edit::put_thread(&mut envelope, "/shop", taken);
    assert_eq!(ids(&envelope).len(), 5);
    // To a page that has nothing now: a page mesurer reads, with the thread.
    let mut fresh = Value::Null;
    edit::put_thread(
        &mut fresh,
        "/new",
        Taken {
            index: 4,
            thread: json!({"id": "x"}),
        },
    );
    assert_eq!(fresh["pages"]["/new"]["comments"][0]["id"], "x");
    assert_eq!(fresh["pages"]["/new"]["guides"], json!([]));
    assert_eq!(fresh["pages"]["/new"]["activeMeasurement"], Value::Null);
    assert_eq!(fresh["enabled"], true);
    assert_eq!(fresh["toolMode"], "none");
}

#[test]
fn set_replaces_guides_and_layout_guides_and_switches_tools() {
    let mut envelope = edit::new_envelope();
    let args = json!({"xray": true, "tool": "Inspect", "minimized": false,
        "guides": [{"orientation": "horizontal", "position": 64}],
        "layout_guides": [{"kind": "columns", "count": 12, "size": 72, "gutter": 20}]});
    let set = edit::set(&mut envelope, "/", args.as_object().unwrap()).unwrap();
    assert_eq!(
        set,
        [
            "xray",
            "minimized",
            "tool select",
            "1 guides",
            "1 layout guides"
        ]
    );
    assert_eq!(envelope["xrayVisible"], true);
    assert_eq!(envelope["toolMode"], "select");
    let page = &envelope["pages"]["/"];
    assert_eq!(page["guides"][0]["orientation"], "horizontal");
    assert!(
        page["guides"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("sawit-")
    );
    let grid = &page["layoutGuides"][0];
    assert_eq!(
        (
            &grid["count"],
            &grid["size"],
            &grid["gutter"],
            &grid["align"],
            &grid["visible"]
        ),
        (
            &json!(12.0),
            &json!(72.0),
            &json!(20.0),
            &json!("stretch"),
            &json!(true)
        )
    );
    assert_eq!(
        (&grid["color"], &grid["opacity"]),
        (&json!("#FF0000"), &json!(0.1))
    );
    // mesurer's limits.
    let wide = json!({"layout_guides": [{"kind": "grid", "count": 99, "size": -4}]});
    edit::set(&mut envelope, "/", wide.as_object().unwrap()).unwrap();
    let grid = &envelope["pages"]["/"]["layoutGuides"][0];
    assert_eq!((&grid["count"], &grid["size"]), (&json!(24.0), &json!(0.0)));

    for (bad, says) in [
        (json!({}), "Give set one of"),
        (json!({"comments": []}), "doesn't take “comments”"),
        (json!({"tool": "xray"}), "Unknown tool “xray”"),
        (json!({"xray": "yes"}), "xray is true or false"),
        (
            json!({"guides": [{"orientation": "diagonal", "position": 1}]}),
            "vertical or horizontal",
        ),
        (json!({"guides": [{"position": -1}]}), "needs a position"),
        (
            json!({"layout_guides": [{"kind": "hex"}]}),
            "columns, rows or grid",
        ),
        (
            json!({"layout_guides": [{"align": "left"}]}),
            "stretch, min, center or max",
        ),
    ] {
        let before = envelope.clone();
        let e = edit::set(&mut envelope, "/", bad.as_object().unwrap()).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
        assert_eq!(envelope, before, "{bad} changed nothing");
    }
}

#[test]
fn clear_takes_one_kind_or_all() {
    let mut envelope = json!({"pages": {"/shop": artifacts()}});
    let cleared = edit::clear(&mut envelope, "/shop", "drawings").unwrap();
    assert_eq!(cleared.drawings, 3);
    let page = &envelope["pages"]["/shop"];
    assert_eq!(page["arrows"], json!([]));
    assert_eq!(page["guides"].as_array().unwrap().len(), 2, "guides stay");
    let cleared = edit::clear(&mut envelope, "/shop", "all").unwrap();
    assert_eq!(
        (
            cleared.drawings,
            cleared.measurements,
            cleared.guides,
            cleared.layout_guides
        ),
        (0, 2, 2, 1)
    );
    assert_eq!(cleared.comments, ["c1", "c2", "c3", "c4"]);
    assert_eq!(envelope["pages"]["/shop"]["comments"], json!([]));
    assert_eq!(envelope["enabled"], true, "the chrome mesurer needs");
    assert!(
        edit::clear(&mut envelope, "/shop", "everything")
            .unwrap_err()
            .contains("drawings, measurements, guides, layout_guides, comments, all")
    );
}
