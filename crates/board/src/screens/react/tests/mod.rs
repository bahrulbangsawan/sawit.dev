//! The React export: attributes and styles as props, text and whitespace as JSX, and the page's
//! styles, scripts and files around the component.

mod attrs;
mod page;
mod text;

use super::{ReactExport, convert};

/// `html` converted as component `Screen`, its files `files`.
fn convert_page(html: &str, files: &[(&str, &[u8])]) -> ReactExport {
    let files: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
        .collect();
    let read = move |rel: &str| {
        files
            .iter()
            .find(|(path, _)| path == rel)
            .map(|(_, b)| b.clone())
    };
    convert(html, "Screen", "Screen.css", "Screen-assets", &read)
}

/// The JSX inside the component's fragment for a page whose body is `body`, without the
/// fragment's indentation.
fn jsx(body: &str) -> String {
    let html = format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    inner(&convert_page(&html, &[]).tsx)
}

/// The JSX inside the fragment of `tsx`.
fn inner(tsx: &str) -> String {
    let start = tsx.find("    <>\n").expect("a fragment") + "    <>\n".len();
    let end = tsx.rfind("    </>\n").expect("its end");
    let lines: Vec<&str> = tsx[start..end]
        .lines()
        .map(|line| line.strip_prefix("      ").unwrap_or(line))
        .collect();
    lines.join("\n")
}
