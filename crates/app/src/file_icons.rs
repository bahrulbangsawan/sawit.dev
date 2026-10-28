//! Language logos for files (Material Icon Theme, MIT — see
//! `assets/file_icons/LICENSE-material-icon-theme`), embedded as small SVGs
//! and drawn in colour with `img()`.

use std::path::Path;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::*;

macro_rules! logos {
    ($($name:literal),* $(,)?) => {
        /// Every embedded logo: (name, SVG bytes).
        const LOGOS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../assets/file_icons/", $name, ".svg")))),*
        ];
    };
}

logos!(
    "astro",
    "audio",
    "c",
    "certificate",
    "console",
    "cpp",
    "csharp",
    "css",
    "dart",
    "database",
    "docker",
    "document",
    "elixir",
    "font",
    "git",
    "go",
    "graphql",
    "haskell",
    "html",
    "image",
    "java",
    "javascript",
    "json",
    "key",
    "kotlin",
    "less",
    "lock",
    "log",
    "lua",
    "makefile",
    "markdown",
    "nix",
    "nodejs",
    "pdf",
    "perl",
    "php",
    "powershell",
    "prisma",
    "proto",
    "python",
    "r",
    "react",
    "react_ts",
    "ruby",
    "rust",
    "sass",
    "scala",
    "settings",
    "svelte",
    "svg",
    "swift",
    "table",
    "terraform",
    "toml",
    "tsconfig",
    "tune",
    "typescript",
    "video",
    "vue",
    "xml",
    "yaml",
    "zig",
    "zip",
);

/// Asset path prefix served by `AppAssets`.
pub const PREFIX: &str = "file_icons/";

/// SVG bytes for an asset path like `file_icons/rust.svg`.
pub fn asset(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix(PREFIX)?.strip_suffix(".svg")?;
    LOGOS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// The logo for a file, if it has a recognisable language or kind.
pub fn logo_for(path: &Path) -> Option<&'static str> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let by_name = match name.as_str() {
        "dockerfile" | "containerfile" | ".dockerignore" => Some("docker"),
        "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml" => {
            Some("docker")
        }
        "makefile" | "gnumakefile" => Some("makefile"),
        "package.json" => Some("nodejs"),
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some("git"),
        "cargo.lock" | "bun.lock" | "bun.lockb" | "yarn.lock" | "pnpm-lock.yaml"
        | "package-lock.json" => Some("lock"),
        n if n.starts_with("tsconfig") && n.ends_with(".json") => Some("tsconfig"),
        n if n.starts_with(".env") => Some("tune"),
        _ => None,
    };
    if by_name.is_some() {
        return by_name;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "react_ts",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "react",
        "rs" => "rust",
        "py" | "pyi" => "python",
        "go" => "go",
        "sql" | "psql" | "db" | "sqlite" => "database",
        "json" | "jsonc" | "json5" => "json",
        "md" | "mdx" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "sass",
        "less" => "less",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "vue" => "vue",
        "svelte" => "svelte",
        "astro" => "astro",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "lua" => "lua",
        "zig" => "zig",
        "dart" => "dart",
        "scala" => "scala",
        "hs" => "haskell",
        "ex" | "exs" => "elixir",
        "nix" => "nix",
        "r" => "r",
        "pl" | "pm" => "perl",
        "ps1" => "powershell",
        "tf" | "tfvars" => "terraform",
        "proto" => "proto",
        "prisma" => "prisma",
        "graphql" | "gql" => "graphql",
        "sh" | "bash" | "zsh" | "fish" => "console",
        "xml" | "plist" => "xml",
        "ini" | "cfg" | "conf" => "settings",
        "lock" => "lock",
        "log" => "log",
        "csv" | "tsv" => "table",
        "pem" | "crt" | "cer" => "certificate",
        "key" => "key",
        "svg" => "svg",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "heic"
        | "heif" | "avif" => "image",
        "pdf" => "pdf",
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" => "video",
        "mp3" | "wav" | "m4a" | "flac" | "ogg" => "audio",
        "ttf" | "otf" | "woff" | "woff2" => "font",
        "zip" | "gz" | "tgz" | "tar" | "xz" | "7z" => "zip",
        "txt" | "rtf" => "document",
        _ => return None,
    })
}

/// The file's logo, or a plain file glyph when it has none.
pub fn file_icon(path: &Path, cx: &App) -> AnyElement {
    match logo_for(path) {
        Some(name) => img(SharedString::from(format!("{PREFIX}{name}.svg")))
            .size(px(14.))
            .flex_none()
            .into_any_element(),
        None => Icon::new(Lucide::File)
            .xsmall()
            .text_color(cx.theme().muted_foreground)
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::{LOGOS, PREFIX, asset, logo_for};
    use std::path::Path;

    #[test]
    fn languages_get_their_own_logo() {
        let logo = |p: &str| logo_for(Path::new(p));
        assert_eq!(logo("main.rs"), Some("rust"));
        assert_eq!(logo("App.tsx"), Some("react_ts"));
        assert_eq!(logo("index.TS"), Some("typescript"));
        assert_eq!(logo("app.py"), Some("python"));
        assert_eq!(logo("q.sql"), Some("database"));
        assert_eq!(logo("Dockerfile"), Some("docker"));
        assert_eq!(logo("compose.yaml"), Some("docker"));
        assert_eq!(logo("package.json"), Some("nodejs"));
        assert_eq!(logo("tsconfig.app.json"), Some("tsconfig"));
        assert_eq!(logo(".env.local"), Some("tune"));
        assert_eq!(logo("Cargo.lock"), Some("lock"));
        assert_eq!(logo("README.md"), Some("markdown"));
        assert_eq!(logo("photo.HEIC"), Some("image"));
        assert_eq!(logo("LICENSE"), None);
        assert_eq!(logo("x.unknownext"), None);
    }

    #[test]
    fn every_logo_used_is_embedded() {
        let samples = [
            "a.ts",
            "a.tsx",
            "a.js",
            "a.jsx",
            "a.rs",
            "a.py",
            "a.go",
            "a.sql",
            "a.json",
            "a.md",
            "a.html",
            "a.css",
            "a.scss",
            "a.less",
            "a.yml",
            "a.toml",
            "a.vue",
            "a.svelte",
            "a.astro",
            "a.java",
            "a.kt",
            "a.c",
            "a.cpp",
            "a.cs",
            "a.rb",
            "a.php",
            "a.swift",
            "a.lua",
            "a.zig",
            "a.dart",
            "a.scala",
            "a.hs",
            "a.ex",
            "a.nix",
            "a.r",
            "a.pl",
            "a.ps1",
            "a.tf",
            "a.proto",
            "a.prisma",
            "a.graphql",
            "a.sh",
            "a.xml",
            "a.ini",
            "a.lock",
            "a.log",
            "a.csv",
            "a.pem",
            "a.key",
            "a.svg",
            "a.png",
            "a.pdf",
            "a.mp4",
            "a.mp3",
            "a.ttf",
            "a.zip",
            "a.txt",
            "Dockerfile",
            "Makefile",
            "package.json",
            ".gitignore",
            "tsconfig.json",
            ".env",
        ];
        for s in samples {
            let name = logo_for(Path::new(s)).unwrap_or_else(|| panic!("{s} has no logo"));
            assert!(
                asset(&format!("{PREFIX}{name}.svg")).is_some(),
                "{s} → {name} not embedded"
            );
        }
        assert!(LOGOS.iter().all(|(_, svg)| svg.starts_with(b"<svg")));
    }
}
