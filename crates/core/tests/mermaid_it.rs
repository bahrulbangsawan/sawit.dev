//! Every Mermaid diagram type renders, flattens to plain SVG, and parses with
//! the usvg version GPUI draws images with.
//!
//! `SAWIT_MERMAID_PNG=dir cargo test -p sawit-core --test mermaid_it` also
//! writes each diagram as PNG for a look.

use sawit_core::mermaid::{Palette, fences, render};

const TYPES: &str = include_str!("fixtures/mermaid-types.md");

#[test]
fn every_diagram_type_renders_for_usvg() {
    let blocks = fences(TYPES);
    assert_eq!(blocks.len(), 33);
    let palette = Palette {
        background: "#282c33".into(),
        foreground: "#dce0e5".into(),
        ..Palette::default()
    };
    let png_dir = std::env::var_os("SAWIT_MERMAID_PNG").map(std::path::PathBuf::from);
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    let options = resvg::usvg::Options {
        fontdb: std::sync::Arc::new(fontdb),
        ..Default::default()
    };
    let mut kinds = std::collections::BTreeSet::new();
    for (ix, block) in blocks.iter().enumerate() {
        let diagram = render(&block.source, &palette)
            .unwrap_or_else(|e| panic!("block {ix} failed: {e}\n{}", block.source));
        assert!(!diagram.svg.contains("var("), "block {ix} kept a var()");
        assert!(
            !diagram.svg.contains("color-mix("),
            "block {ix} kept a color-mix()"
        );
        let tree = resvg::usvg::Tree::from_str(&diagram.svg, &options)
            .unwrap_or_else(|e| panic!("block {ix} ({}) doesn't parse: {e}", diagram.kind));
        assert!(tree.size().width() > 0.0);
        kinds.insert(diagram.kind.clone());
        if let Some(dir) = &png_dir {
            let size = tree.size().to_int_size();
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
            resvg::render(&tree, Default::default(), &mut pixmap.as_mut());
            pixmap
                .save_png(dir.join(format!("{ix:02}-{}.png", diagram.kind)))
                .unwrap();
        }
    }
    assert_eq!(kinds.len(), 33, "{kinds:?}");
}
