//! Attributes as props: React's names, booleans, numbers, form defaults, styles, SVG, and what is
//! left out.

use super::{convert_page, jsx};

#[test]
fn html_attributes_take_reacts_names() {
    let out = jsx(concat!(
        r#"<label for="email" class="field">Email</label>"#,
        r#"<input id="email" readonly maxlength="40" minlength="2" autocomplete="off" autofocus "#,
        r#"spellcheck="false" inputmode="email" enterkeyhint="go" tabindex="-1">"#,
        r#"<form enctype="multipart/form-data" novalidate accept-charset="utf-8"></form>"#,
        r#"<iframe frameborder="0" allowfullscreen referrerpolicy="no-referrer"></iframe>"#,
        r##"<img crossorigin="anonymous" usemap="#m" alt="">"##,
        r#"<time datetime="2024-01-01">Jan</time><a hreflang="en">en</a>"#,
        r#"<button formaction="/go" formmethod="post" formnovalidate>Go</button>"#,
        r#"<video autoplay playsinline muted loop controls></video>"#,
        r#"<div contenteditable="">x</div>"#,
    ));
    for expected in [
        r#"<label htmlFor="email" className="field">Email</label>"#,
        r#"<input id="email" readOnly maxLength={40} minLength={2} autoComplete="off" autoFocus spellCheck="false" inputMode="email" enterKeyHint="go" tabIndex={-1} />"#,
        r#"<form encType="multipart/form-data" noValidate acceptCharset="utf-8" />"#,
        r#"<iframe frameBorder="0" allowFullScreen referrerPolicy="no-referrer" />"#,
        r##"<img crossOrigin="anonymous" useMap="#m" alt="" />"##,
        r#"<time dateTime="2024-01-01">Jan</time>"#,
        r#"<a hrefLang="en">en</a>"#,
        r#"<button formAction="/go" formMethod="post" formNoValidate>Go</button>"#,
        r#"<video autoPlay playsInline muted loop controls />"#,
        r#"<div contentEditable suppressContentEditableWarning>x</div>"#,
    ] {
        assert!(out.contains(expected), "{expected}\nin\n{out}");
    }
}

#[test]
fn table_cells_get_number_spans() {
    let out = jsx(
        r#"<table><tr><td colspan="2" rowspan="3">x</td><td colspan="wide">y</td></tr></table>"#,
    );
    assert_eq!(
        out,
        "<table>\n  <tbody>\n    <tr>\n      <td colSpan={2} rowSpan={3}>x</td>\n      \
         <td colSpan=\"wide\">y</td>\n    </tr>\n  </tbody>\n</table>"
    );
}

#[test]
fn data_aria_and_unknown_attributes_pass_through() {
    let out = jsx(
        r#"<div data-user-id="7" aria-label="Menu" x-data="{ open: false }" foo="bar" @click="go()" :class="c">m</div>"#,
    );
    assert_eq!(
        out,
        r#"<div data-user-id="7" aria-label="Menu" x-data="{ open: false }" foo="bar" {...{ "@click": "go()" }} {...{ ":class": "c" }}>m</div>"#
    );
}

#[test]
fn boolean_attributes_are_bare_whatever_their_value() {
    let out = jsx(
        r#"<details open><summary>s</summary></details><button disabled="disabled" hidden>b</button><div hidden="until-found">f</div>"#,
    );
    assert!(out.contains("<details open>"), "{out}");
    assert!(out.contains("<button disabled hidden>b</button>"), "{out}");
    assert!(
        out.contains(r#"<div hidden="until-found">f</div>"#),
        "{out}"
    );
}

#[test]
fn form_values_become_uncontrolled_defaults() {
    let out = jsx(concat!(
        r#"<input value="Ada"><input type="checkbox" name="ok" value="yes" checked>"#,
        r#"<input type="submit" value="Send"><input type="hidden" value="h">"#,
        "<textarea name=\"bio\">\nHello\n  world</textarea>",
        r#"<select name="s"><option value="a">A</option><option value="b" selected>B</option></select>"#,
        r#"<select multiple><optgroup label="g"><option selected> One  thing </option></optgroup><option value="2" selected>Two</option><option>3</option></select>"#,
    ));
    for expected in [
        r#"<input defaultValue="Ada" />"#,
        r#"<input type="checkbox" name="ok" value="yes" defaultChecked />"#,
        r#"<input type="submit" value="Send" />"#,
        r#"<input type="hidden" value="h" />"#,
        r#"<textarea name="bio" defaultValue={"Hello\n  world"} />"#,
        r#"<select name="s" defaultValue="b">"#,
        r#"<option value="b">B</option>"#,
        r#"<select multiple defaultValue={["One thing", "2"]}>"#,
        "<option>3</option>",
    ] {
        assert!(out.contains(expected), "{expected}\nin\n{out}");
    }
    assert!(!out.contains("selected"), "{out}");
}

#[test]
fn style_attributes_become_objects() {
    let page = r#"<html><body><p style="color: red; background-image: url(&quot;img/a.png&quot;); --brand: #f60; -webkit-line-clamp: 2; -ms-flex: 1; -moz-appearance: none; font-family: 'A;B', serif; margin: 0 ! IMPORTANT; /* gone */ color: blue">x</p><div style=" ; "></div></body></html>"#;
    let export = convert_page(page, &[("img/a.png", b"png")]);
    let out = super::inner(&export.tsx);
    assert_eq!(
        out,
        r##"<p style={{ backgroundImage: "url(\"./Screen-assets/img/a.png\")", "--brand": "#f60", WebkitLineClamp: "2", msFlex: "1", MozAppearance: "none", fontFamily: "'A;B', serif", margin: "0", color: "blue" } as CSSProperties}>x</p>
<div />"##
    );
    let imports = "import type { CSSProperties } from \"react\";\nimport \"./Screen.css\";";
    assert!(export.tsx.contains(imports), "{}", export.tsx);
    assert!(
        export
            .notes
            .iter()
            .any(|n| n.contains("!important") && n.contains("margin")),
        "{:?}",
        export.notes
    );
}

#[test]
fn svg_keeps_its_tags_and_camel_cases_its_attributes() {
    let out = jsx(concat!(
        r#"<svg viewbox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" class="icon" tabindex="0">"#,
        r##"<defs><lineargradient id="g"><stop offset="0" stop-color="#fff" stop-opacity="0.5"></stop></lineargradient>"##,
        r#"<clippath id="c"><rect width="1" height="1"></rect></clippath></defs>"#,
        r#"<path d="M0 0L1 1" stroke-width="2" fill-rule="evenodd" clip-path="url(#c)" stroke-linecap="round" style="fill: red"></path>"#,
        r##"<use xlink:href="#g" xml:space="preserve"></use>"##,
        r#"<foreignobject><div class="in">hi</div></foreignobject></svg>"#,
    ));
    let expected = r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" xmlnsXlink="http://www.w3.org/1999/xlink" className="icon" tabIndex={0}>
  <defs>
    <linearGradient id="g">
      <stop offset="0" stopColor="#fff" stopOpacity="0.5" />
    </linearGradient>
    <clipPath id="c">
      <rect width="1" height="1" />
    </clipPath>
  </defs>
  <path d="M0 0L1 1" strokeWidth="2" fillRule="evenodd" clipPath="url(#c)" strokeLinecap="round" style={{ fill: "red" }} />
  <use xlinkHref="#g" xmlSpace="preserve" />
  <foreignObject>
    <div className="in">hi</div>
  </foreignObject>
</svg>"##;
    assert_eq!(jsx_lines(&out), jsx_lines(expected));
}

fn jsx_lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

#[test]
fn event_handlers_and_javascript_urls_are_left_out_and_flagged() {
    let page = r#"<html><body onload="init()"><button class="save" onclick="save(); return false">Save</button><a href="javascript:void(0)" id="x">x</a><a href="/ok">ok</a></body></html>"#;
    let export = convert_page(page, &[]);
    // The body's only attribute was a handler, so no <div> wraps the component.
    assert!(export.tsx.contains("    <>\n"), "{}", export.tsx);
    assert!(
        export
            .tsx
            .contains(r#"<button className="save">Save</button>"#),
        "{}",
        export.tsx
    );
    assert!(export.tsx.contains(r#"<a id="x">x</a>"#), "{}", export.tsx);
    assert!(
        export.tsx.contains(r#"<a href="/ok">ok</a>"#),
        "{}",
        export.tsx
    );
    assert!(!export.tsx.contains("onclick=") && !export.tsx.contains("onClick"));
    let flagged = |what: &str| export.notes.iter().any(|note| note.contains(what));
    assert!(
        flagged("onclick on <button class=\"save\">"),
        "{:?}",
        export.notes
    );
    assert!(flagged("onload on <body>"), "{:?}", export.notes);
    assert!(
        flagged("javascript: URL in href on <a id=\"x\">"),
        "{:?}",
        export.notes
    );
    for note in &export.notes {
        assert!(export.tsx.contains(&format!("// {note}\n")), "{note}");
    }
    let end = &export.tsx[export.tsx.find("/*\nLeft out").expect("the comment")..];
    assert!(
        end.contains("onclick on <button class=\"save\">:\nsave(); return false"),
        "{end}"
    );
    assert!(end.contains("javascript:void(0)"), "{end}");
}

#[test]
fn values_that_need_it_are_string_literals() {
    let out = jsx("<p title='He said \"hi\" &amp; left' data-x=\"a\\b\" lang=\"en\">t</p>");
    assert_eq!(
        out,
        r#"<p title={"He said \"hi\" & left"} data-x={"a\\b"} lang="en">t</p>"#
    );
}
