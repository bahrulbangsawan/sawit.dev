//! The style sheet scanner: what is a reference and what isn't, rewriting in place, and local
//! `@import`s inlined.

use super::*;

fn hrefs(css: &str) -> Vec<String> {
    references(css).into_iter().map(|r| r.href).collect()
}

#[test]
fn finds_urls_unquoted_and_quoted() {
    let css = r#"a { background: url(a.png) } b { background: url( 'b c.png' ) }
        c { background: URL("d.png") no-repeat }"#;
    assert_eq!(hrefs(css), ["a.png", "b c.png", "d.png"]);
    let found = references("x{background:url(a.png)}");
    assert_eq!(found[0].span, 13..23);
    assert_eq!(found[0].kind, Kind::Url);
}

#[test]
fn skips_comments_strings_and_other_functions() {
    let css = r#"/* url(no1.png) */ a::before { content: "url(no2.png)"; }
        b::after { content: 'it\'s url(no3.png)' } c { mask: myurl(no4.png) }
        d { background: url(yes.png) }"#;
    assert_eq!(hrefs(css), ["yes.png"]);
}

#[test]
fn decodes_escapes() {
    assert_eq!(hrefs(r"a{background:url(a\ b.png)}"), ["a b.png"]);
    assert_eq!(
        hrefs(r#"a{background:url("q\"uote.png")}"#),
        ["q\"uote.png"]
    );
    assert_eq!(hrefs(r"a{background:url(\66 ont.woff)}"), ["font.woff"]);
}

#[test]
fn a_bad_url_is_no_reference() {
    assert!(hrefs("a{background:url(a b.png)} b{}").is_empty());
    assert!(hrefs("a{background:url(a\"b.png)}").is_empty());
    // What follows a bad URL is still read.
    assert_eq!(
        hrefs("a{background:url(a b)} b{background:url(c.png)}"),
        ["c.png"]
    );
}

#[test]
fn finds_image_set_strings() {
    let css = r#"a { background-image: image-set("a.png" 1x, url(b.png) 2x);
        b: -webkit-image-set('c.png' 1x); content: "not.png" }"#;
    let found = references(css);
    let kinds: Vec<_> = found
        .iter()
        .map(|r| (r.kind.clone(), r.href.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (Kind::ImageSet, "a.png"),
            (Kind::Url, "b.png"),
            (Kind::ImageSet, "c.png")
        ]
    );
}

#[test]
fn reads_import_rules() {
    let css = r#"@import "a.css";
@import url(b.css) screen;
@IMPORT url("c.css") layer(base) supports(display: grid) print and (min-width: 40em);
@import 'd.css'"#;
    let found = references(css);
    let imports: Vec<(&str, String)> = found
        .iter()
        .map(|r| match &r.kind {
            Kind::Import { conditions } => (r.href.as_str(), conditions.clone()),
            _ => panic!("not an import: {r:?}"),
        })
        .collect();
    assert_eq!(
        imports,
        [
            ("a.css", String::new()),
            ("b.css", "screen".into()),
            (
                "c.css",
                "layer(base) supports(display: grid) print and (min-width: 40em)".into()
            ),
            ("d.css", String::new()),
        ]
    );
    assert_eq!(&css[found[0].span.clone()], "@import \"a.css\";");
    assert_eq!(found[3].span.end, css.len());
    // A url() inside an @import isn't a reference of its own.
    assert!(!found.iter().any(|r| r.kind == Kind::Url));
}

#[test]
fn rewrites_urls_in_place() {
    let css = r#"a { background: url(a.png) } b { background: url("keep.png") }
        c { background: image-set("c.png" 1x) } @import "x.css";"#;
    let out = rewrite_urls(css, |href| match href {
        "a.png" => Some("data:image/png;base64,AAA".into()),
        "c.png" => Some("./assets/c\"q.png".into()),
        _ => None,
    });
    assert_eq!(
        out,
        r#"a { background: url("data:image/png;base64,AAA") } b { background: url("keep.png") }
        c { background: image-set("./assets/c\"q.png" 1x) } @import "x.css";"#
    );
}

#[test]
fn quotes_what_it_writes() {
    assert_eq!(url_token(r#"a "b" \c"#), r#"url("a \"b\" \\c")"#);
    assert_eq!(quoted("a\nb"), "\"a\\a b\"");
}

#[test]
fn scanning_odd_text_never_panics() {
    let css = "a{b:url(é\\ü) /* ünended \"str 'x\\\n@import url( 'ö";
    for end in 0..=css.len() {
        if css.is_char_boundary(end) {
            let _ = references(&css[..end]);
            let _ = rewrite_urls(&css[..end], |_| Some("x".into()));
        }
    }
    assert_eq!(hrefs("a{b:url(ünï.png)}"), ["ünï.png"]);
}

/// Sheets by path, for `flatten`'s loader.
fn files(list: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let list: Vec<(String, String)> = list
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect();
    move |rel: &str| list.iter().find(|(p, _)| p == rel).map(|(_, t)| t.clone())
}

/// A `url` callback that writes each reference as `<sheet>|<href>`, to show which sheet it was
/// resolved from.
fn tag_urls(sheet: &str, href: &str) -> Option<String> {
    Some(format!("{sheet}|{href}"))
}

#[test]
fn flatten_inlines_local_imports_with_urls_from_their_own_sheet() {
    let load = files(&[
        (
            "css/base.css",
            "@import \"parts/type.css\";\nbody { background: url(../img/bg.png) }",
        ),
        (
            "css/parts/type.css",
            "@charset \"utf-8\";\nh1 { background: url(t.svg) }",
        ),
    ]);
    let css = "@import url(css/base.css);\n.x { background: url(img/x.png) }";
    let flat = flatten(css, "index.html", &load, &mut tag_urls);
    assert!(flat.imports.is_empty());
    assert_eq!(
        flat.body,
        "\nh1 { background: url(\"css/parts/type.css|t.svg\") }\nbody { background: \
         url(\"css/base.css|../img/bg.png\") }\n.x { background: url(\"index.html|img/x.png\") }"
    );
}

#[test]
fn flatten_hoists_remote_and_missing_imports() {
    let load = files(&[(
        "a.css",
        "@import url(https://fonts.example/a.css);\na { color: red }",
    )]);
    let css = "@import \"a.css\";\n@import url(\"https://cdn.example/b.css\") print;\n\
               @import \"missing.css\";\nb { color: blue }";
    let flat = flatten(css, "index.html", &load, &mut |_, _| None);
    assert_eq!(
        flat.imports,
        [
            "@import url(https://fonts.example/a.css);",
            "@import url(\"https://cdn.example/b.css\") print;",
            "@import \"missing.css\";",
        ]
    );
    assert!(
        flat.text()
            .starts_with("@import url(https://fonts.example/a.css);\n")
    );
    assert!(flat.body.contains("a { color: red }"));
}

#[test]
fn flatten_cuts_cycles_and_stops_deep_chains() {
    let load = files(&[
        ("a.css", "@import \"b.css\";\na { x: 1 }"),
        ("b.css", "@import \"a.css\";\nb { x: 2 }"),
    ]);
    let flat = flatten("@import \"a.css\";", "index.html", &load, &mut |_, _| None);
    assert_eq!(flat.body.matches("a { x: 1 }").count(), 1);
    assert_eq!(flat.body.matches("b { x: 2 }").count(), 1);
    assert!(flat.imports.is_empty());

    let chain: Vec<(String, String)> = (0..10)
        .map(|n| {
            (
                format!("s{n}.css"),
                format!("@import \"s{}.css\";\n.s{n} {{}}", n + 1),
            )
        })
        .collect();
    let load = move |rel: &str| chain.iter().find(|(p, _)| p == rel).map(|(_, t)| t.clone());
    let flat = flatten("@import \"s0.css\";", "index.html", &load, &mut |_, _| None);
    assert!(flat.body.contains(".s3 {}"));
    assert!(!flat.body.contains(".s9 {}"));
    assert_eq!(flat.imports.len(), 1);
}

#[test]
fn imported_sheets_keep_their_conditions() {
    let load = files(&[("p.css", "p { x: 1 }")]);
    let flat = flatten(
        "@import \"p.css\" print;",
        "index.html",
        &load,
        &mut |_, _| None,
    );
    assert_eq!(flat.body, "@media print {\np { x: 1 }\n}\n");
    assert_eq!(
        wrap("layer(base) supports(display: grid) screen", "a{}"),
        "@layer base {\n@supports (display: grid) {\n@media screen {\na{}\n}\n}\n}\n"
    );
    assert_eq!(wrap("layer", "a{}"), "@layer {\na{}\n}\n");
    assert_eq!(
        wrap("supports(not (display: grid))", "a{}"),
        "@supports not (display: grid) {\na{}\n}\n"
    );
    assert_eq!(wrap("", "a{}"), "a{}");
}
