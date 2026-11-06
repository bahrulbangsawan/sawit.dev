//! A screen's checks, from its HTML and CSS.

use super::super::html::check;

/// The checks of `html` at `width`, as `(rule, selector)`.
fn found(html: &str, width: f64) -> Vec<(String, String)> {
    let none = |_: &str| None;
    let findings = check(html, &none, width);
    findings
        .into_iter()
        .map(|f| (f.rule, f.selector.unwrap_or_default()))
        .collect()
}

/// A page that clears every check, with `css` added to its sheet and `body` as its main.
fn page(css: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html><head><style>
body {{ margin: 0; padding: 16px; font: 16px/1.5 system-ui; color: #111827; background: #ffffff; }}
h1 {{ font-size: 24px; margin: 0 0 8px; }}
label {{ display: block; margin-bottom: 4px; }}
input {{ font-size: 16px; min-height: 48px; padding: 0 12px; border: 1px solid #6b7280; }}
:focus-visible {{ outline: 2px solid #1d4ed8; outline-offset: 2px; }}
{css}
</style></head><body><main><h1>Settings</h1>
<label for="name">Name</label><input id="name" type="text" value="Ada">
{body}</main></body></html>"#
    )
}

/// Item 27's seed: a low-contrast note, 10 px legal text and a 30 px button on a phone screen
/// fail contrast, the text floor and the target size, and nothing else.
#[test]
fn a_low_contrast_note_small_print_and_a_short_button_are_the_three_failures() {
    let css = ".note { color: #aaaaaa; }
.legal { font-size: 10px; }
.save { height: 30px; padding: 0 16px; font-size: 16px; color: #ffffff; background: #1d4ed8; border: 0; }";
    let body = r#"<p class="note">Changes save to this device.</p>
<p class="legal">Terms apply.</p>
<button class="save" type="submit">Save changes</button>"#;
    assert_eq!(
        found(&page(css, body), 393.0),
        [
            ("col.10".to_string(), "p.note".to_string()),
            ("typ.11".to_string(), "p.legal".to_string()),
            ("a11y.2".to_string(), "button.save".to_string()),
        ]
    );
    // The page alone passes.
    assert_eq!(found(&page("", ""), 393.0), []);
}

/// A 30 px button is a fine target on a desktop frame (24 px there), not on a touch one (44).
#[test]
fn targets_are_44_on_touch_frames_and_24_elsewhere() {
    let css = ".go { height: 30px; padding: 0 16px; }";
    let body = r#"<button class="go">Continue</button>"#;
    assert_eq!(found(&page(css, body), 1440.0), []);
    assert_eq!(
        found(&page(css, body), 768.0),
        [("a11y.2".into(), "button.go".into())]
    );
    // A link in a sentence flows with it, and has no target of its own.
    let inline = r#"<p>Read the <a href="/terms">terms</a> first.</p>"#;
    assert_eq!(found(&page("", inline), 393.0), []);
}

/// Colours come through custom properties, shorthands and translucent fills as a browser
/// computes them.
#[test]
fn contrast_reads_tokens_and_the_fill_behind() {
    let css = ":root { --muted: #9ca3af; --card: #f9fafb; }
.card { background: var(--card); padding: 16px; }
.meta { color: var(--muted, #000); }
.dim { color: rgba(17, 24, 39, 0.4); }";
    let body =
        r#"<div class="card"><p class="meta">Updated today</p><p class="dim">Draft</p></div>"#;
    assert_eq!(
        found(&page(css, body), 1440.0),
        [
            ("col.10".into(), "p.meta".into()),
            ("col.10".into(), "p.dim".into())
        ]
    );
    // Large text needs 3:1: #767676 on white is 4.54, #949494 is 3.03.
    let large = "h2 { font-size: 24px; color: #949494; }";
    assert_eq!(found(&page(large, "<h2>Profile</h2>"), 1440.0), []);
}

/// Images without alt, fields without labels, icon-only controls without names, focus rings
/// taken away, spacing off the scale and text that can't fit its box.
#[test]
fn the_other_rules_are_checked() {
    let body = r#"<img src="a.png"><img src="b.png" alt="">
<input class="search" type="search" placeholder="Search">
<button class="icon"><svg width="16" height="16"></svg></button>"#;
    let css = ".icon { width: 48px; height: 48px; }";
    assert_eq!(
        found(&page(css, body), 1440.0),
        [
            ("a11y.7".into(), "main > img".into()),
            ("a11y.8".into(), "input.search".into()),
            ("a11y.7".into(), "button.icon".into()),
        ]
    );
    let css = "button { outline: none; min-height: 48px; min-width: 48px; } .row { gap: 7px; }
.tag { width: 60px; white-space: nowrap; }
.box { width: 100px; height: 24px; }";
    let body = r#"<div class="row"><button>Go</button></div><span class="tag">A very long tag name</span>
<div class="box">This sentence is much too long for its box</div>"#;
    let without_ring = page(css, body).replace(":focus-visible", ":hover");
    assert_eq!(
        found(&without_ring, 1440.0),
        [
            ("lay.18".into(), "span.tag".into()),
            ("lay.18".into(), "div.box".into()),
            ("spc.1".into(), ".row".into()),
            ("sta.8".into(), "button".into()),
            ("a11y.8".into(), ":focus-visible".into()),
        ]
    );
}
