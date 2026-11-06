//! A walked frame ([`super::tree::Built`]) as one HTML file.

use std::fmt::Write as _;

use super::REFERENCE;
use super::tree::{Built, Decls, Item};

/// The page: the frame's markup in `<body>`, its rules in one `<style>`.
pub(super) fn html_page(title: &str, built: &mut Built) -> String {
    let mut css = tokens(&built.tokens);
    css.push_str(
        "*, *::before, *::after { box-sizing: border-box; }\nbody { margin: 0; }\n\
         svg { display: block; }\n",
    );
    css.push_str(&rules(&built.rules));
    css.push_str(&rules(&built.placements));
    let mut body = String::new();
    markup(&built.root, 0, &mut body);
    format!(
        "<!doctype html>\n<!-- {} -->\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>\n{css}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        REFERENCE.replace("--", "–"),
        escape(title)
    )
}

/// `:root { --name: value; }` for the variables, or nothing.
pub(super) fn tokens(tokens: &[(String, String)]) -> String {
    if tokens.is_empty() {
        return String::new();
    }
    let mut out = String::from(":root {\n");
    for (name, value) in tokens {
        let _ = writeln!(out, "  {name}: {value};");
    }
    out.push_str("}\n");
    out
}

/// Each class's rule, in order.
pub(super) fn rules(rules: &[(String, Decls)]) -> String {
    let mut out = String::new();
    for (class, decls) in rules {
        if decls.is_empty() {
            continue;
        }
        let _ = writeln!(out, ".{class} {{");
        for (k, v) in decls {
            let _ = writeln!(out, "  {k}: {v};");
        }
        out.push_str("}\n");
    }
    out
}

/// `item` as HTML, indented `depth` steps.
fn markup(item: &Item, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match item {
        Item::Box {
            class,
            name,
            children,
        } => {
            let open = format!("{pad}<div class=\"{class}\" data-name=\"{}\">", attr(name));
            if children.is_empty() {
                let _ = writeln!(out, "{open}</div>");
                return;
            }
            let _ = writeln!(out, "{open}");
            for child in children {
                markup(child, depth + 1, out);
            }
            let _ = writeln!(out, "{pad}</div>");
        }
        Item::Text {
            class, name, text, ..
        } => {
            let _ = writeln!(
                out,
                "{pad}<p class=\"{class}\" data-name=\"{}\">{}</p>",
                attr(name),
                escape(text)
            );
        }
        Item::Svg { class, name, svg } => {
            let tagged = svg.replacen(
                "<svg",
                &format!(
                    "<svg class=\"{class}\" data-name=\"{}\" aria-hidden=\"true\"",
                    attr(name)
                ),
                1,
            );
            let _ = writeln!(out, "{pad}{tagged}");
        }
        // HTML inlines instances: no calls.
        Item::Call { .. } => {}
    }
}

/// Text for HTML: `&`, `<` and `>` escaped.
pub(super) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// An attribute's value, quotes escaped too.
fn attr(text: &str) -> String {
    escape(text).replace('"', "&quot;")
}
