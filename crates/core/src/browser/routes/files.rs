//! The routes a framework's files make, framework by framework: Next.js (app and pages),
//! TanStack Router and Start (`src/routes`), SvelteKit, Remix and React Router (flat routes in
//! `app/routes`), Nuxt and Astro. Each walks its routes folder and turns each page's file into a
//! [`Pattern`]; the folder is named in each [`Pattern::file`] as `<folder>/<file>`.

use std::path::{Path, PathBuf};

use super::pattern::{Pattern, Segment};

/// Most files looked at in one routes folder: a folder that big is no app's routes.
const MOST_FILES: usize = 20_000;
/// Deepest folder looked into under a routes folder.
const DEEPEST: usize = 24;

/// The files under `root`, relative to it, sorted: none in a dot folder or `node_modules`.
pub(super) fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![(PathBuf::new(), 0)];
    while let Some((rel, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&rel)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(text) = name.to_str() else { continue };
            if text.starts_with('.') || text == "node_modules" {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && depth < DEEPEST {
                folders.push((rel.join(text), depth + 1));
            } else if kind.is_file() && found.len() < MOST_FILES {
                found.push(rel.join(text));
            }
        }
    }
    found.sort();
    found
}

/// `file`'s folders under the routes folder, as names.
fn folders(file: &Path) -> Vec<&str> {
    (file.parent().into_iter())
        .flat_map(Path::components)
        .filter_map(|c| c.as_os_str().to_str())
        .collect()
}

/// `file`'s name without its extension, when the extension is one of `exts`.
fn stem_with<'a>(file: &'a Path, exts: &[&str]) -> Option<&'a str> {
    let name = file.file_name()?.to_str()?;
    let (stem, ext) = name.rsplit_once('.')?;
    exts.contains(&ext).then_some(stem)
}

fn pattern(segments: Vec<Segment>, label: &str, file: &Path) -> Pattern {
    Pattern {
        segments,
        file: format!("{label}/{}", file.display()),
    }
}

/// A folder or file name in the bracket syntax of Next.js, SvelteKit, Nuxt and Astro: `[slug]`,
/// `[[lang]]`, `[...path]`, `[[...path]]`, SvelteKit's `[id=integer]`; a name with brackets in it
/// (`post-[id]`) is a value too.
fn bracket(name: &str) -> Segment {
    let inner = |text: &str| text.split('=').next().unwrap_or(text).to_string();
    if let Some(rest) = name
        .strip_prefix("[[...")
        .and_then(|n| n.strip_suffix("]]"))
    {
        Segment::OptionalRest(inner(rest))
    } else if let Some(rest) = name.strip_prefix("[...").and_then(|n| n.strip_suffix(']')) {
        Segment::Rest(inner(rest))
    } else if let Some(opt) = name.strip_prefix("[[").and_then(|n| n.strip_suffix("]]")) {
        Segment::Optional(inner(opt))
    } else if let Some(param) = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')) {
        Segment::Param(inner(param))
    } else if name.contains('[') {
        Segment::Param(name.to_string())
    } else {
        Segment::Static(name.to_string())
    }
}

/// A route group, `(marketing)`: in the folders, not in the path.
fn is_group(name: &str) -> bool {
    name.starts_with('(') && name.ends_with(')')
}

/// Next.js's App Router (`app/`, `src/app/`): each folder with a `page` file is a route. Groups
/// leave the path, and private folders (`_lib`), parallel slots (`@modal`) and intercepting
/// routes (`(.)photo`) make none of their own.
pub(super) fn next_app(root: &Path, label: &str) -> Vec<Pattern> {
    let pages = ["tsx", "ts", "jsx", "js", "mdx", "md"];
    (files_under(root).iter())
        .filter(|file| stem_with(file, &pages) == Some("page"))
        .filter_map(|file| {
            let mut segments = Vec::new();
            for name in folders(file) {
                if name.starts_with("(.") || name.starts_with('@') || name.starts_with('_') {
                    return None;
                }
                if !is_group(name) {
                    segments.push(bracket(name));
                }
            }
            Some(pattern(segments, label, file))
        })
        .collect()
}

/// Next.js's Pages Router (`pages/`, `src/pages/`): each page file, `index` its folder's route.
/// `_app`, `_document`, the error pages and `api/` are no pages.
pub(super) fn next_pages(root: &Path, label: &str) -> Vec<Pattern> {
    let pages = ["tsx", "ts", "jsx", "js", "mdx", "md"];
    (files_under(root).iter())
        .filter_map(|file| {
            let stem = stem_with(file, &pages)?;
            let dirs = folders(file);
            let private = dirs.iter().chain([&stem]).any(|n| n.starts_with('_'));
            if private || dirs.first() == Some(&"api") || ["404", "500"].contains(&stem) {
                return None;
            }
            let mut segments: Vec<Segment> = dirs.into_iter().map(bracket).collect();
            if stem != "index" {
                segments.push(bracket(stem));
            }
            Some(pattern(segments, label, file))
        })
        .collect()
}

/// `name` split at its dots, but not at a dot in brackets (`sitemap[.]xml`).
fn dotted(name: &str) -> Vec<String> {
    let (mut parts, mut part, mut depth) = (Vec::new(), String::new(), 0usize);
    for c in name.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '.' if depth == 0 => {
                parts.push(std::mem::take(&mut part));
                continue;
            }
            _ => {}
        }
        part.push(c);
    }
    parts.push(part);
    parts
}

/// A name with its escaping brackets gone: `sitemap[.]xml` is `sitemap.xml`.
fn unescaped(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '[' | ']')).collect()
}

/// TanStack Router's file routes (`src/routes`): folders and dotted names nest (`posts.$id.tsx`
/// is `/posts/$id`), `index` is its parent's route, `route.tsx` its folder's, `$id` a value, `$`
/// the rest, `{-$lang}` an optional value. `__root`, pathless layouts (`_auth`), groups and names
/// starting with `-` leave the path; `posts_` is `posts`.
pub(super) fn tanstack(root: &Path, label: &str) -> Vec<Pattern> {
    let code = ["tsx", "ts", "jsx", "js"];
    (files_under(root).iter())
        .filter_map(|file| {
            let stem = stem_with(file, &code)?;
            let stem = stem.strip_suffix(".lazy").unwrap_or(stem);
            let dirs = folders(file);
            let ignored = dirs.iter().chain([&stem]).any(|n| n.starts_with('-'));
            let test = stem.ends_with(".test") || stem.ends_with(".spec");
            if ignored || test || stem.starts_with("__root") {
                return None;
            }
            let mut tokens: Vec<String> = dirs.iter().flat_map(|d| dotted(d)).collect();
            tokens.extend(dotted(stem));
            if tokens.last().is_some_and(|t| t == "index" || t == "route") {
                tokens.pop();
            }
            let segments = tokens.iter().filter_map(|t| tanstack_token(t)).collect();
            Some(pattern(segments, label, file))
        })
        .collect()
}

fn tanstack_token(token: &str) -> Option<Segment> {
    if is_group(token) || token.starts_with('_') {
        return None;
    }
    let token = token.strip_suffix('_').unwrap_or(token);
    if token == "$" {
        return Some(Segment::Rest("_splat".into()));
    }
    if let Some(name) = token.strip_prefix("{-$").and_then(|t| t.strip_suffix('}')) {
        return Some(Segment::Optional(name.into()));
    }
    if token.contains('$') {
        let name: String = (token.rsplit('$').next().unwrap_or(token).chars())
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        return Some(Segment::Param(name));
    }
    Some(Segment::Static(unescaped(token)))
}

/// SvelteKit (`src/routes`): each folder with a `+page.svelte` is a route; groups leave the path.
pub(super) fn sveltekit(root: &Path, label: &str) -> Vec<Pattern> {
    (files_under(root).iter())
        .filter(|file| file.file_name().is_some_and(|n| n == "+page.svelte"))
        .map(|file| {
            let segments = (folders(file).into_iter())
                .filter(|name| !is_group(name))
                .map(bracket)
                .collect();
            pattern(segments, label, file)
        })
        .collect()
}

/// Remix's and React Router's flat routes (`app/routes`): each file there, or folder with a
/// `route` file, by its dotted name: `_index` is its parent's route, `$id` a value, `$` the rest,
/// `($lang)` an optional part; pathless layouts (`_auth`) leave the path, and `posts_` is `posts`.
pub(super) fn flat_routes(root: &Path, label: &str) -> Vec<Pattern> {
    let code = ["tsx", "ts", "jsx", "js", "mdx", "md"];
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let path = PathBuf::from(&name);
            if entry.file_type().ok()?.is_dir() {
                let route = code.iter().map(|ext| format!("route.{ext}"));
                let file = route
                    .map(|f| path.join(f))
                    .find(|f| root.join(f).is_file())?;
                return Some((name, file));
            }
            let stem = stem_with(&path, &code)?.to_string();
            Some((stem, path))
        })
        .filter(|(name, _)| !name.starts_with('.') && !name.contains(".server"))
        .filter(|(name, _)| !name.contains(".client"))
        .collect();
    found.sort();
    (found.into_iter())
        .map(|(name, file)| {
            let segments = dotted(&name)
                .iter()
                .filter_map(|t| remix_token(t))
                .collect();
            pattern(segments, label, &file)
        })
        .collect()
}

fn remix_token(token: &str) -> Option<Segment> {
    if token.starts_with('_') {
        return None;
    }
    let token = token.strip_suffix('_').unwrap_or(token);
    if token == "$" {
        return Some(Segment::Rest("*".into()));
    }
    if let Some(inner) = token.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        return Some(Segment::Optional(inner.trim_start_matches('$').into()));
    }
    if let Some(name) = token.strip_prefix('$') {
        return Some(Segment::Param(name.into()));
    }
    Some(Segment::Static(unescaped(token)))
}

/// Nuxt (`pages/`, or `app/pages/` in Nuxt 4) and Astro (`src/pages/`): each page file in the
/// bracket syntax, `index` its folder's route; groups leave the path. Astro's files and folders
/// starting with `_` are no pages, nor are its endpoints (`.ts`, `.js`).
pub(super) fn page_files(root: &Path, label: &str, exts: &[&str], astro: bool) -> Vec<Pattern> {
    (files_under(root).iter())
        .filter_map(|file| {
            let stem = stem_with(file, exts)?;
            let dirs = folders(file);
            if astro && dirs.iter().chain([&stem]).any(|n| n.starts_with('_')) {
                return None;
            }
            let mut segments: Vec<Segment> = (dirs.into_iter())
                .filter(|name| !is_group(name))
                .map(bracket)
                .collect();
            if stem != "index" {
                segments.push(bracket(stem));
            }
            Some(pattern(segments, label, file))
        })
        .collect()
}
