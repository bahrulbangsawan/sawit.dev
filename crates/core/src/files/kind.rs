//! What a file is: the viewer the center pane shows it in, and the language its text is
//! highlighted as.

use std::path::Path;

/// What the center pane shows for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Code or plain text: the editor.
    Text,
    /// Editor plus rendered preview.
    Markdown,
    Html,
    Svg,
    /// Mermaid source (`.mmd`), previewed as the diagram.
    Mermaid,
    /// Viewers.
    Image,
    Pdf,
    Video,
    /// A Sawit canvas file (`.sawit`), shown on a canvas bound to the file.
    Canvas,
    /// An Excalidraw scene (`.excalidraw`), imported onto a canvas that is saved as the
    /// `.sawit` file beside it.
    Excalidraw,
}

/// The extension of Sawit's canvas files: the project's `canvas.sawit`, and every file Save as…
/// writes.
pub const CANVAS_EXTENSION: &str = "sawit";

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn classify(path: &Path) -> FileKind {
    match extension(path).as_str() {
        "md" | "mdx" | "markdown" => FileKind::Markdown,
        "html" | "htm" => FileKind::Html,
        "svg" => FileKind::Svg,
        "mmd" | "mermaid" => FileKind::Mermaid,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "heic"
        | "heif" | "avif" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" => FileKind::Video,
        CANVAS_EXTENSION => FileKind::Canvas,
        "excalidraw" => FileKind::Excalidraw,
        _ => FileKind::Text,
    }
}

/// Highlighter language token for a file: the name of a tree-sitter grammar
/// the editor ships ("rust", "tsx", "sql"), a syntax name its syntect backend
/// knows, or the bare extension for syntect to try.
pub fn language_for(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(".env") {
        return "bash".into();
    }
    match lower.as_str() {
        "dockerfile" | "containerfile" => return "dockerfile".into(),
        "makefile" | "gnumakefile" => return "make".into(),
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zprofile" | ".zshenv"
        | "bashrc" | "bash.bashrc" | "zshrc" | "zshenv" | "zprofile" | "profile" => {
            return "bash".into();
        }
        "cargo.lock" => return "toml".into(),
        "yarn.lock" => return "yaml".into(),
        _ => {}
    }
    let ext = extension(path);
    let lang = match ext.as_str() {
        "" => "text",
        "dockerfile" => "dockerfile",
        "sql" | "psql" => "sql",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "json" | "jsonc" | "json5" | "lock" => "json",
        "md" | "mdx" | "markdown" => "markdown",
        "toml" => "toml",
        "rs" => "rust",
        "css" => "css",
        "scss" | "sass" => "scss",
        "less" => "less",
        "html" | "htm" => "html",
        "xml" | "svg" | "plist" => "xml",
        "yml" | "yaml" => "yaml",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "go" => "go",
        "py" | "pyi" => "python",
        "kt" | "kts" => "kotlin",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "lua" => "lua",
        "zig" => "zig",
        "vue" => "vue",
        "svelte" => "svelte",
        "graphql" | "gql" => "graphql",
        "ini" | "cfg" | "conf" => "ini",
        "diff" | "patch" => "diff",
        "prisma" => "prisma",
        "txt" | "log" => "text",
        other => return other.to_string(),
    };
    lang.into()
}

/// Human name of a language token, for the editor's status line.
pub fn language_label(token: &str) -> String {
    let label = match token {
        "text" => "Text",
        "sql" => "SQL",
        "typescript" => "TypeScript",
        "tsx" => "TSX",
        "javascript" => "JavaScript",
        "json" => "JSON",
        "markdown" => "Markdown",
        "mmd" | "mermaid" => "Mermaid",
        "toml" => "TOML",
        "rust" => "Rust",
        "css" => "CSS",
        "scss" => "SCSS",
        "less" => "Less",
        "html" => "HTML",
        "xml" => "XML",
        "yaml" => "YAML",
        "bash" => "Shell",
        "go" => "Go",
        "python" => "Python",
        "kotlin" => "Kotlin",
        "java" => "Java",
        "c" => "C",
        "cpp" => "C++",
        "csharp" => "C#",
        "ruby" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "lua" => "Lua",
        "zig" => "Zig",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "graphql" => "GraphQL",
        "ini" => "INI",
        "diff" => "Diff",
        "prisma" => "Prisma",
        "dockerfile" => "Dockerfile",
        "make" => "Makefile",
        other => return other.to_string(),
    };
    label.into()
}
