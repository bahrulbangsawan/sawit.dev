//! Flattening, fences, error words, the bundle's version and Pele's limits, tested.

use std::time::Instant;

use super::flattening::mix_colors;
use super::*;

/// CANVAS-CORE-10: a render stopped for running past its time limit says so in words
/// [`took_too_long`] knows; another error doesn't.
#[test]
fn a_render_stopped_for_time_is_told_from_other_errors() {
    assert!(took_too_long(&format!(
        "The diagram can't be drawn: {TOOK_TOO_LONG}"
    )));
    assert!(!took_too_long("Parse error on line 2: unexpected token"));
    assert!(!took_too_long("The diagram renderer stopped."));
}

#[test]
fn flatten_resolves_vars_and_mixes() {
    let svg = r##"<svg class="pele" style="max-width:100%;--_bg:var(--pele-bg,#fff);--_a:var(--pele-surface-alt,#e6e6e6);--_r:var(--pele-radius,4px);color:var(--_fg)" fill="var(--_fg)"><rect style="rx:var(--_r)" fill="color-mix(in srgb,var(--_a) 50%,var(--_bg))"/><text>var(--kept) a;--b:c</text><path fill="var(--pele-series-1,#4c78a8)"/></svg>"##;
    let palette = Palette {
        background: "#000000".into(),
        surface_alt: "#ffffff".into(),
        ..Palette::default()
    };
    let out = flatten(svg, &palette);
    assert!(!out.contains("var(--_"), "{out}");
    assert!(!out.contains("color-mix"), "{out}");
    // `--_fg` isn't declared here and has no fallback.
    assert!(out.contains(r#"style="max-width:100%;color:""#), "{out}");
    assert!(out.contains(r#"style="rx:4px""#), "{out}");
    assert!(out.contains(r##"fill="#808080""##), "{out}");
    assert!(out.contains(r##"fill="#4c78a8""##), "{out}");
    // Text content is untouched.
    assert!(out.contains("<text>var(--kept) a;--b:c</text>"), "{out}");
    assert!(out.contains(r##"<rect width="100%" height="100%" fill="#000000"/>"##));
}

/// Pele 1.13's series palettes: a diagram's `themeVariables` palette is declared on the root
/// as `--pele-series-*`, which may name a theme variable (that then follows the palette given
/// here); and a label on a fill the diagram sets names `--pele-fg-on-dark` or `-light`, which
/// the palette leaves to Pele's own.
#[test]
fn flatten_bakes_series_palettes_and_text_on_fills() {
    let palette = Palette {
        accent: "#123456".into(),
        ..Palette::default()
    };
    let source = "---\nconfig:\n  themeVariables:\n    pie1: \"var(--pele-accent)\"\n    \
                      pie2: \"#205ea6\"\n---\npie\n  \"A\": 1\n  \"B\": 2\n  \"C\": 3\n";
    let d = render(source, &palette).unwrap();
    assert!(!d.svg.contains("var("), "{}", d.svg);
    assert!(
        d.svg.contains("#123456") && d.svg.contains("#205ea6"),
        "{}",
        d.svg
    );
    // The third slice keeps the theme's own third colour.
    assert!(d.svg.contains("#54a24b"), "{}", d.svg);
    let block = "block\n  a[\"A\"]\n  style a fill:#333,stroke:#000\n";
    let d = render(block, &palette).unwrap();
    assert!(!d.svg.contains("var("), "{}", d.svg);
    assert!(d.svg.contains(r##"fill="#fff""##), "{}", d.svg);
}

#[test]
fn mix_weights() {
    assert_eq!(
        mix_colors("color-mix(in srgb,#ffffff 25%,#000000)"),
        "#404040"
    );
    assert_eq!(mix_colors("color-mix(in srgb, #fff, #000 25%)"), "#bfbfbf");
    // Anything else is left for the renderer to ignore.
    assert_eq!(
        mix_colors("color-mix(in oklch,red,blue)"),
        "color-mix(in oklch,red,blue)"
    );
}

#[test]
fn finds_mermaid_fences() {
    let md = "# T\n\n```mermaid\ngraph LR\n  A-->B\n```\n\n````md\n```mermaid\nnot me\n```\n````\n\n  ~~~ Mermaid title\npie\n~~~\n\n```mermaid\nunclosed";
    let found = fences(md);
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].source, "graph LR\n  A-->B\n");
    assert_eq!(
        &md[found[0].range.clone()],
        "```mermaid\ngraph LR\n  A-->B\n```\n"
    );
    assert_eq!(found[1].source, "pie\n");
    assert_eq!(found[2].source, "unclosed");
}

#[test]
fn rewrites_fences() {
    let md = "a\n```mermaid\nx\n```\nb\n```mermaid\ny\n```\n";
    let found = fences(md);
    let diagram = Diagram {
        svg: String::new(),
        width: 1.,
        height: 1.,
        kind: "flowchart".into(),
    };
    let out = rewrite_markdown(md, &found, &[Ok(diagram), Err("bad ``` thing".into())]);
    assert_eq!(
        out,
        "a\n![Diagram](sawit-mermaid:0)\n\n[Save Diagram as SVG…](sawit-mermaid-save:0)\n\nb\n**The diagram can't be drawn:**\n\n````text\nbad ``` thing\n````\n\n"
    );
    assert_eq!(index_in("sawit-mermaid-save:12", SAVE_SCHEME), Some(12));
    assert_eq!(index_in("sawit-mermaid:x", IMAGE_SCHEME), None);
}

#[test]
fn renders_and_reports_errors() {
    let d = render("flowchart LR\n  A[Hello] --> B[World]", &Palette::default()).unwrap();
    assert_eq!(d.kind, "flowchart");
    assert!(d.width > 0. && d.height > 0.);
    assert!(d.svg.starts_with("<svg") && !d.svg.contains("var("));
    assert!(render("flowchart LR\n  A[unclosed --> B", &Palette::default()).is_err());
    assert!(render("notADiagram", &Palette::default()).is_err());
}

/// The bundle is the Pele that [`PELE_VERSION`] names, with its licence beside it.
#[test]
fn the_bundle_is_the_pele_version_named() {
    let banner = format!("/*! Pele {PELE_VERSION} | MIT License");
    assert!(PELE_JS.starts_with(&banner), "{}", &PELE_JS[..80]);
    let licence = include_str!("../../assets/pele/LICENSE");
    assert!(licence.starts_with("MIT License\n\nCopyright (c) 2026 kepano"));
}

/// A source whose type Pele doesn't draw is told so in Sawit's words: what it starts with,
/// and the types it can start with. ZenUML, a mermaid.js plugin, is named as such.
#[test]
fn an_unsupported_type_is_told_in_sawits_words() {
    let palette = Palette::default();
    let zen = render("zenuml\n  title Demo\n  Alice->John: Hello", &palette).unwrap_err();
    assert!(zen.starts_with("ZenUML diagrams can't be drawn"), "{zen}");
    assert!(zen.contains("Pele"), "{zen}");
    let other = render("---\ntitle: T\n---\n%% note\n\nnotADiagram\n  a", &palette);
    let other = other.unwrap_err();
    assert!(
        other.starts_with("“notADiagram” isn't a diagram type Sawit can draw"),
        "{other}"
    );
    for (keyword, _) in TYPES {
        assert!(other.contains(keyword), "{other} names {keyword}");
    }
    assert!(unsupported(" \n%%{init: {}}%%\n").starts_with("The diagram is empty"));
}

/// A diagram past one of Pele's limits is refused at once, saying it is too big and which
/// limit it passed, instead of holding the worker (open items: "a Mermaid diagram of
/// thousands of edges stalls the worker for seconds").
#[test]
fn a_diagram_past_a_limit_is_refused_at_once() {
    let palette = Palette::default();
    let mut many = String::from("flowchart LR\n");
    for i in 0..=MAX_EDGES {
        many.push_str(&format!("  a{i} --> b{i}\n"));
    }
    let started = Instant::now();
    let error = render(&many, &palette).unwrap_err();
    assert!(
        error.starts_with("The diagram is too big to draw: "),
        "{error}"
    );
    assert!(error.contains(&MAX_EDGES.to_string()), "{error}");
    assert!(
        started.elapsed() < RENDER_TIMEOUT / 2,
        "{:?}",
        started.elapsed()
    );
    let long = format!("flowchart LR\n  A[{}]", "x".repeat(SOURCE_LIMIT as usize));
    let error = render(&long, &palette).unwrap_err();
    assert!(
        error.starts_with("The diagram is too big to draw: "),
        "{error}"
    );
    // Neither is a render stopped for time, which the canvas would not try again.
    assert!(!took_too_long(&error));
}

/// How long QuickJS takes to lay out flowcharts of growing size, which [`MAX_EDGES`] is
/// chosen by: `cargo test -p sawit-core --release -- --ignored --nocapture edges_take`.
#[test]
#[ignore]
fn edges_take() {
    for edges in [250u32, 500, 1000, 1500, 2000, 3000] {
        let mut source = String::from("flowchart LR\n");
        let nodes = edges.div_ceil(2);
        for i in 0..edges {
            source.push_str(&format!("  n{} --> n{}\n", i % nodes, (i * 7 + 3) % nodes));
        }
        let started = Instant::now();
        let result = run(source);
        println!(
            "{edges} edges: {:?} {}",
            started.elapsed(),
            result
                .as_ref()
                .map_or_else(|e| e.clone(), |r| r.svg.len().to_string())
        );
    }
}
