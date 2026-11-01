//! Every diagram type Pele draws renders, flattens to plain SVG, and parses with
//! the usvg version GPUI draws images with.
//!
//! `SAWIT_MERMAID_PNG=dir cargo test -p sawit-core --test mermaid_it` also
//! writes each diagram as SVG and PNG for a look.

use std::path::{Path, PathBuf};

use sawit_core::mermaid::{Palette, TYPES, fences, render};

const TYPES_MD: &str = include_str!("fixtures/mermaid-types.md");

/// One fixture per keyword of [`TYPES`], which the canvas's tests draw too.
const KEYWORD_FIXTURES: &str = "../board/tests/fixtures/mermaid/types";

/// The app's dark theme, whose colours a flattened SVG must carry as well as the light ones.
fn dark() -> Palette {
    Palette {
        background: "#282c33".into(),
        foreground: "#dce0e5".into(),
        ..Palette::default()
    }
}

fn usvg_options() -> resvg::usvg::Options<'static> {
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    resvg::usvg::Options {
        fontdb: std::sync::Arc::new(fontdb),
        ..Default::default()
    }
}

/// Renders `source` with `palette`, checks the SVG is flat and that usvg reads it, and writes it
/// as SVG and PNG to `png_dir` when one is given. Pele's type for it.
fn draws(
    name: &str,
    source: &str,
    palette: &Palette,
    options: &resvg::usvg::Options,
    png_dir: Option<&Path>,
) -> String {
    let diagram =
        render(source, palette).unwrap_or_else(|e| panic!("{name} failed: {e}\n{source}"));
    let svg = &diagram.svg;
    assert!(!svg.contains("var("), "{name} kept a var()");
    assert!(!svg.contains("color-mix("), "{name} kept a color-mix()");
    assert!(
        diagram.width > 0.0 && diagram.height > 0.0,
        "{name} has no size"
    );
    let tree = resvg::usvg::Tree::from_str(svg, options)
        .unwrap_or_else(|e| panic!("{name} ({}) doesn't parse: {e}", diagram.kind));
    assert!(tree.size().width() > 0.0);
    // Something is drawn: a diagram whose every colour was lost would parse to an empty tree.
    assert!(tree.root().has_children(), "{name} draws nothing");
    if let Some(dir) = png_dir {
        let size = tree.size().to_int_size();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
        resvg::render(&tree, Default::default(), &mut pixmap.as_mut());
        pixmap.save_png(dir.join(format!("{name}.png"))).unwrap();
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
    diagram.kind
}

#[test]
fn every_diagram_type_renders_for_usvg() {
    let blocks = fences(TYPES_MD);
    assert_eq!(blocks.len(), 33);
    let png_dir = std::env::var_os("SAWIT_MERMAID_PNG").map(PathBuf::from);
    if let Some(dir) = &png_dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    let options = usvg_options();
    let mut kinds = std::collections::BTreeSet::new();
    for (ix, block) in blocks.iter().enumerate() {
        let name = format!("{ix:02}");
        let kind = draws(&name, &block.source, &dark(), &options, png_dir.as_deref());
        kinds.insert(kind);
    }
    // Every type Pele draws, once.
    let all: std::collections::BTreeSet<String> =
        TYPES.iter().map(|(_, kind)| kind.to_string()).collect();
    assert_eq!(kinds, all);
}

/// Each keyword a Mermaid source can start with (each C4 and railroad variant, each `-beta`
/// spelling) is detected as the type [`TYPES`] gives it, and draws in the light and the dark
/// palette alike.
#[test]
fn every_keyword_is_detected_and_drawn() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(KEYWORD_FIXTURES);
    let png_dir = std::env::var_os("SAWIT_MERMAID_PNG").map(|d| PathBuf::from(d).join("keywords"));
    if let Some(dir) = &png_dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    let options = usvg_options();
    for (keyword, kind) in TYPES {
        let file = dir.join(format!("{keyword}.mmd"));
        let source = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("{} can't be read: {e}", file.display()));
        let first = source
            .lines()
            .map(str::trim)
            .skip_while(|line| *line == "---")
            .find(|line| line.starts_with(keyword));
        assert!(first.is_some(), "{keyword}.mmd starts with {keyword}");
        let drawn = draws(
            keyword,
            &source,
            &Palette::default(),
            &options,
            png_dir.as_deref(),
        );
        assert_eq!(drawn, *kind, "{keyword}");
        let dark_name = format!("{keyword}-dark");
        draws(&dark_name, &source, &dark(), &options, png_dir.as_deref());
    }
    // No fixture is left over for a keyword that isn't one.
    let fixtures = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(fixtures, TYPES.len());
}
