//! The single HTML file: style sheets, scripts and files inlined from a screen's folder, and what
//! isn't in the folder left as written.

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::*;

/// A tiny PNG's bytes (its signature and a stand-in for the rest).
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#;

/// A screen folder holding `files`.
fn screen(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    for (path, bytes) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("its folder");
        std::fs::write(path, bytes).expect("the file");
    }
    dir
}

fn png_data() -> String {
    format!("data:image/png;base64,{}", STANDARD.encode(PNG))
}

fn svg_data() -> String {
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(SVG))
}

fn single(folder: &Path) -> String {
    single_file(folder).expect("the page")
}

#[test]
fn a_linked_sheet_becomes_a_style_with_its_imports_and_urls_from_its_own_file() {
    let page = r#"<!DOCTYPE html><html><head>
<link rel="stylesheet" href="css/site.css" media="screen">
</head><body><h1>Hi</h1></body></html>"#;
    let dir = screen(&[
        ("index.html", page.as_bytes()),
        (
            "css/site.css",
            b"@import \"parts/type.css\";\nbody { background: url(../img/bg.png) }",
        ),
        (
            "css/parts/type.css",
            b"h1 { background-image: url('dot.svg#x') }",
        ),
        ("css/parts/dot.svg", SVG.as_bytes()),
        ("img/bg.png", PNG),
    ]);
    let out = single(dir.path());
    assert!(out.starts_with("<!DOCTYPE html>"), "{out}");
    assert!(!out.contains("<link"), "{out}");
    assert!(out.contains("<style media=\"screen\">"), "{out}");
    assert!(
        out.contains(&format!("background: url(\"{}\")", png_data())),
        "{out}"
    );
    assert!(
        out.contains(&format!("background-image: url(\"{}#x\")", svg_data())),
        "{out}"
    );
    assert!(
        out.find("h1 {").unwrap() < out.find("body {").unwrap(),
        "{out}"
    );
}

#[test]
fn images_media_icons_and_style_urls_become_data_urls() {
    let page = r#"<html><head><link rel="icon" href="img/a.png">
<style>.x { background: url(img/a.png) }</style></head><body>
<img src="img/a.png" srcset="img/a.png 1x, img/missing.png 2x, https://cdn.example/a.png 3x">
<picture><source srcset="img/a.png" type="image/png"></picture>
<div style="background: url('img/a.png')">x</div>
<video poster="img/a.png" src="clip.mp4"></video>
<input type="image" src="img/a.png"><input type="text" src="img/a.png">
<svg><image href="icon.svg"></image><use xlink:href="icon.svg#home"></use></svg>
</body></html>"#;
    let dir = screen(&[
        ("index.html", page.as_bytes()),
        ("img/a.png", PNG),
        ("icon.svg", SVG.as_bytes()),
    ]);
    let out = single(dir.path());
    let png = png_data();
    assert!(
        !out.starts_with("<!DOCTYPE"),
        "a page without a doctype gets none: {out}"
    );
    assert!(
        out.contains(&format!("<link rel=\"icon\" href=\"{png}\">")),
        "{out}"
    );
    assert!(
        out.contains(&format!(".x {{ background: url(\"{png}\") }}")),
        "{out}"
    );
    assert!(out.contains(&format!("<img src=\"{png}\"")), "{out}");
    assert!(
        out.contains(&format!(
            "srcset=\"{png} 1x, img/missing.png 2x, https://cdn.example/a.png 3x\""
        )),
        "{out}"
    );
    assert!(out.contains(&format!("<source srcset=\"{png}\"")), "{out}");
    assert!(
        out.contains(&format!("style=\"background: url(&quot;{png}&quot;)\"")),
        "{out}"
    );
    assert!(
        out.contains(&format!("<video poster=\"{png}\" src=\"clip.mp4\">")),
        "{out}"
    );
    assert!(
        out.contains(&format!("<input type=\"image\" src=\"{png}\">")),
        "{out}"
    );
    assert!(
        out.contains("<input type=\"text\" src=\"img/a.png\">"),
        "{out}"
    );
    assert!(
        out.contains(&format!("<image href=\"{}\">", svg_data())),
        "{out}"
    );
    assert!(
        out.contains(&format!("<use xlink:href=\"{}#home\">", svg_data())),
        "{out}"
    );
}

#[test]
fn local_scripts_go_inline_and_deferred_ones_move_to_the_end() {
    let page = r#"<!doctype html><html><head>
<script src="app.js" defer></script>
<script type="module" src="mod.js"></script>
<script src="https://cdn.example/lib.js"></script>
</head><body><p>Hi</p></body></html>"#;
    let dir = screen(&[
        ("index.html", page.as_bytes()),
        ("app.js", b"document.body.append('</script><b>');"),
        ("mod.js", b"export const x = 1;"),
    ]);
    let out = single(dir.path());
    assert!(
        out.contains("<script type=\"module\">export const x = 1;</script>"),
        "{out}"
    );
    assert!(
        out.contains("<script src=\"https://cdn.example/lib.js\"></script>"),
        "{out}"
    );
    let moved = "<script>document.body.append('<\\/script><b>');</script></body>";
    assert!(out.contains(moved), "{out}");
    assert!(!out.contains("defer"), "{out}");
    assert!(!out.contains("app.js"), "{out}");
}

#[test]
fn what_isnt_in_the_folder_stays_as_written() {
    let outside = screen(&[("secret.png", PNG)]);
    let folder = outside.path().join("screen");
    let page = r##"<html><head>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter&display=swap">
<link rel="stylesheet" href="missing.css">
<style>@import url(https://cdn.example/x.css); .a { background: url(gone.png) }</style>
</head><body>
<img src="https://example.com/a.png"><img src="//cdn.example/b.png">
<img src="data:image/png;base64,AAAA"><img src="nope.png"><img src="../secret.png">
<a href="#top">top</a><svg><use href="#icon"></use></svg>
<div style="background: url(#frag)"></div>
</body></html>"##;
    std::fs::create_dir_all(&folder).expect("the folder");
    std::fs::write(folder.join("index.html"), page).expect("the page");
    let out = single(&folder);
    for kept in [
        "href=\"https://fonts.googleapis.com/css2?family=Inter&amp;display=swap\"",
        "<link rel=\"stylesheet\" href=\"missing.css\">",
        "@import url(https://cdn.example/x.css);",
        ".a { background: url(gone.png) }",
        "<img src=\"https://example.com/a.png\">",
        "<img src=\"//cdn.example/b.png\">",
        "<img src=\"data:image/png;base64,AAAA\">",
        "<img src=\"nope.png\">",
        "<img src=\"../secret.png\">",
        "<a href=\"#top\">",
        "<use href=\"#icon\">",
        "style=\"background: url(#frag)\"",
    ] {
        assert!(out.contains(kept), "{kept} in {out}");
    }
}

#[test]
fn no_index_is_an_error() {
    let dir = screen(&[("other.html", b"<p>x</p>")]);
    assert!(single_file(dir.path()).is_err());
}

#[test]
fn template_content_is_written_and_inlined() {
    let page = r#"<body><template id="t"><img src="a.png"><p>in</p></template></body>"#;
    let dir = screen(&[("index.html", page.as_bytes()), ("a.png", PNG)]);
    let out = single(dir.path());
    let template = format!(
        "<template id=\"t\"><img src=\"{}\"><p>in</p></template>",
        png_data()
    );
    assert!(out.contains(&template), "{out}");
}

#[test]
fn inline_html_reads_through_its_closure() {
    let read = |rel: &str| (rel == "s.css").then(|| b"p { color: red }".to_vec());
    let out = inline_html(
        r#"<link rel="stylesheet" href="./s.css?v=2"><p>x</p>"#,
        &read,
    );
    assert!(out.contains("<style>p { color: red }</style>"), "{out}");
    // An alternate sheet is off by default, so it stays a link.
    let alternate = r#"<link rel="alternate stylesheet" href="s.css">"#;
    assert!(inline_html(alternate, &read).contains("<link rel=\"alternate stylesheet\""));
}

#[test]
fn srcset_splits_as_html_does() {
    assert_eq!(
        srcset(" a.png 1x,b.png  2x , data:image/png;base64,AA,BB 3x,c.png,, d.png 100w"),
        [
            ("a.png".to_string(), "1x".to_string()),
            ("b.png".into(), "2x".into()),
            ("data:image/png;base64,AA,BB".into(), "3x".into()),
            ("c.png".into(), String::new()),
            ("d.png".into(), "100w".into()),
        ]
    );
    let out = rewrite_srcset("a.png 1x, b.png 2x", |u| (u == "b.png").then(|| "B".into()));
    assert_eq!(out.as_deref(), Some("a.png 1x, B 2x"));
    assert_eq!(rewrite_srcset("a.png 1x", |_| None), None);
}

#[test]
fn end_tags_in_raw_text_are_escaped() {
    assert_eq!(
        escape_end_tag("a</SCRIPT>b</script", "script"),
        "a<\\/SCRIPT>b<\\/script"
    );
    assert_eq!(escape_end_tag("x</style>", "style"), "x<\\/style>");
}

#[test]
fn svg_styles_are_inlined_and_bodyless_pages_keep_their_scripts() {
    let read = |rel: &str| match rel {
        "a.png" => Some(PNG.to_vec()),
        "a.js" => Some(b"go()".to_vec()),
        _ => None,
    };
    let out = inline_html("<svg><style>.s { fill: url(a.png) }</style></svg>", &read);
    assert!(
        out.contains(&format!(".s {{ fill: url(\"{}\") }}", png_data())),
        "{out}"
    );
    let frameset =
        r#"<html><head><script src="a.js" defer></script></head><frameset></frameset></html>"#;
    let out = inline_html(frameset, &read);
    assert!(
        out.contains("<head><script defer=\"\">go()</script></head>"),
        "{out}"
    );
}
