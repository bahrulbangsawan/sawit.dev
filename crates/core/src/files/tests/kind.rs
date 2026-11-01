//! The viewer and the highlighter language for a file.

use std::path::Path;

use crate::files::{FileKind, classify, language_for, language_label};

#[test]
fn language_from_extension() {
    assert_eq!(language_for(Path::new("a/b.sql")), "sql");
    assert_eq!(language_for(Path::new("x.tsx")), "tsx");
    assert_eq!(language_for(Path::new("x.mjs")), "javascript");
    assert_eq!(language_for(Path::new("Cargo.toml")), "toml");
    assert_eq!(language_for(Path::new("README.md")), "markdown");
    assert_eq!(language_for(Path::new(".env")), "bash");
    assert_eq!(language_for(Path::new("noext")), "text");
}

#[test]
fn language_covers_common_project_files() {
    for (file, lang) in [
        ("Dockerfile", "dockerfile"),
        ("api.Dockerfile", "dockerfile"),
        ("Makefile", "make"),
        ("GNUmakefile", "make"),
        (".env.local", "bash"),
        (".zshrc", "bash"),
        ("x.TSX", "tsx"),
        ("x.vue", "vue"),
        ("x.svelte", "svelte"),
        ("x.kt", "kotlin"),
        ("x.java", "java"),
        ("x.c", "c"),
        ("x.hpp", "cpp"),
        ("x.cs", "csharp"),
        ("x.rb", "ruby"),
        ("x.php", "php"),
        ("x.swift", "swift"),
        ("x.lua", "lua"),
        ("x.zig", "zig"),
        ("x.scss", "scss"),
        ("x.less", "less"),
        ("x.graphql", "graphql"),
        ("x.xml", "xml"),
        ("x.svg", "xml"),
        ("x.ini", "ini"),
        ("x.diff", "diff"),
        ("x.prisma", "prisma"),
        ("tsconfig.json", "json"),
        ("x.jsonc", "json"),
        ("Cargo.lock", "toml"),
        ("bun.lock", "json"),
        ("yarn.lock", "yaml"),
    ] {
        assert_eq!(language_for(Path::new(file)), lang, "{file}");
    }
    // Unknown extensions are passed through for the highlighter to try.
    assert_eq!(language_for(Path::new("x.nim")), "nim");
}

#[test]
fn language_labels_for_the_status_line() {
    assert_eq!(language_label("tsx"), "TSX");
    assert_eq!(language_label("rust"), "Rust");
    assert_eq!(language_label("text"), "Text");
    assert_eq!(language_label("nim"), "nim");
}

#[test]
fn file_kind_picks_the_viewer() {
    use FileKind::*;
    for (file, kind) in [
        ("a.ts", Text),
        ("README.md", Markdown),
        ("x.MDX", Markdown),
        ("index.html", Html),
        ("logo.svg", Svg),
        ("flow.mmd", Mermaid),
        ("flow.Mermaid", Mermaid),
        ("a.png", Image),
        ("a.JPG", Image),
        ("a.webp", Image),
        ("a.gif", Image),
        ("a.heic", Image),
        ("doc.pdf", Pdf),
        ("clip.mp4", Video),
        ("clip.mov", Video),
        ("clip.webm", Video),
        ("noext", Text),
    ] {
        assert_eq!(classify(Path::new(file)), kind, "{file}");
    }
}

#[test]
fn classify_canvas_files_and_excalidraw() {
    use FileKind::*;
    for (file, kind) in [
        ("board.sawit", Canvas),
        ("/p/.nyawit/canvas.sawit", Canvas),
        ("Board.SAWIT", Canvas),
        ("sketch.excalidraw", Excalidraw),
        ("/p/.nyawit/canvas.excalidraw", Excalidraw),
        ("Sketch.Excalidraw", Excalidraw),
        // An Excalidraw library, and names without the extension, are something else.
        ("shapes.excalidrawlib", Text),
        ("sawit", Text),
        // Regression: the canvas engine's own extension is no canvas file, just an unknown one.
        ("board.figcraft", Text),
        ("canvas.sawit.bak", Text),
        (".excalidraw", Text),
    ] {
        assert_eq!(classify(Path::new(file)), kind, "{file}");
    }
}
