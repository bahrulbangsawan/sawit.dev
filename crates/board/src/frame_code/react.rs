//! A walked frame ([`super::tree::Built`]) as a React component in TypeScript and its CSS module.
//! Each component its instances use is a function component of its own, before the frame's: a
//! prop for each variant property (its values as a union of string literals), `texts` for the
//! texts an instance shows instead of its main's, and `className`, which places it.

use std::fmt::Write as _;

use base64::Engine as _;

use super::REFERENCE;
use super::render::{rules, tokens};
use super::tree::{Built, Component, Item};

/// The `.tsx` of component `name` importing `css_file`, and the CSS module.
pub(super) fn tsx(name: &str, css_file: &str, built: &mut Built) -> (String, String) {
    let mut out = String::new();
    for line in wrap(REFERENCE, 96) {
        let _ = writeln!(out, "// {line}");
    }
    let _ = writeln!(
        out,
        "import styles from {};\n",
        json(&format!("./{css_file}"))
    );
    for component in &built.components {
        component_code(component, built, &mut out);
    }
    let _ = writeln!(out, "export default function {name}() {{");
    out.push_str("  return (\n");
    jsx(&built.root, 2, None, built, &mut out);
    out.push_str("  );\n}\n");
    let mut css = format!("/* {} */\n", REFERENCE);
    css.push_str(&tokens(&built.tokens));
    if let Some(root) = root_class(&built.root) {
        let _ = writeln!(css, ".{root}, .{root} * {{ box-sizing: border-box; }}");
    }
    css.push_str(&rules(&built.rules));
    css.push_str(&rules(&built.placements));
    (out, css)
}

/// One component: its props' type and its function, a branch for each variant the frame uses.
fn component_code(c: &Component, built: &Built, out: &mut String) {
    let _ = writeln!(out, "type {}Props = {{", c.name);
    for p in &c.props {
        let values: Vec<String> = p.values.iter().map(|v| json(v)).collect();
        let _ = writeln!(out, "  {}?: {};", p.ident, values.join(" | "));
    }
    if !c.texts.is_empty() {
        let keys: Vec<String> = c.texts.iter().map(|t| json(t)).collect();
        let _ = writeln!(
            out,
            "  texts?: Partial<Record<{}, string>>;",
            keys.join(" | ")
        );
    }
    out.push_str("  className?: string;\n};\n\n");
    let branches = c.variants.len() > 1;
    let mut args: Vec<String> = Vec::new();
    if branches {
        args.extend(
            c.props
                .iter()
                .map(|p| format!("{} = {}", p.ident, json(&p.default))),
        );
    }
    if !c.texts.is_empty() {
        args.push("texts".into());
    }
    args.push("className".into());
    let _ = writeln!(
        out,
        "export function {}({{ {} }}: {}Props) {{",
        c.name,
        args.join(", "),
        c.name
    );
    if branches {
        let idents: Vec<&str> = c.props.iter().map(|p| p.ident.as_str()).collect();
        let _ = writeln!(
            out,
            "  const variant = [{}].join(\"|\");",
            idents.join(", ")
        );
        for (pairs, item) in c.variants.iter().skip(1) {
            let key: Vec<String> = c
                .props
                .iter()
                .map(|p| {
                    pairs
                        .get(&p.label)
                        .cloned()
                        .unwrap_or_else(|| p.default.clone())
                })
                .collect();
            let _ = writeln!(out, "  if (variant === {}) {{", json(&key.join("|")));
            out.push_str("    return (\n");
            jsx(item, 3, Some("className"), built, out);
            out.push_str("    );\n  }\n");
        }
    }
    if let Some((_, item)) = c.variants.first() {
        out.push_str("  return (\n");
        jsx(item, 2, Some("className"), built, out);
        out.push_str("  );\n");
    } else {
        out.push_str("  return <div className={className} />;\n");
    }
    out.push_str("}\n\n");
}

/// `item` as JSX at `depth`; `extra` is the prop a component's root adds to its class.
fn jsx(item: &Item, depth: usize, extra: Option<&str>, built: &Built, out: &mut String) {
    let pad = "  ".repeat(depth);
    let class_attr = |class: &str| match extra {
        Some(prop) => format!(
            "className={{[styles[{}], {prop}].filter(Boolean).join(\" \")}}",
            json(class)
        ),
        None => format!("className={{styles[{}]}}", json(class)),
    };
    match item {
        Item::Box {
            class,
            name,
            children,
        } => {
            let open = format!("<div {} data-name={{{}}}", class_attr(class), json(name));
            if children.is_empty() {
                let _ = writeln!(out, "{pad}{open} />");
                return;
            }
            let _ = writeln!(out, "{pad}{open}>");
            for child in children {
                jsx(child, depth + 1, None, built, out);
            }
            let _ = writeln!(out, "{pad}</div>");
        }
        Item::Text {
            class,
            name,
            text,
            key,
        } => {
            let body = match key {
                Some(key) => format!("{{texts?.[{}] ?? {}}}", json(key), json(text)),
                None => format!("{{{}}}", json(text)),
            };
            let _ = writeln!(
                out,
                "{pad}<p {} data-name={{{}}}>{body}</p>",
                class_attr(class),
                json(name)
            );
        }
        Item::Svg { class, name, svg } => {
            let src = format!(
                "data:image/svg+xml;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
            );
            let _ = writeln!(
                out,
                "{pad}<img {} data-name={{{}}} src={{{}}} alt=\"\" />",
                class_attr(class),
                json(name),
                json(&src)
            );
        }
        Item::Call {
            component,
            class,
            name,
            props,
            texts,
        } => {
            let c = &built.components[*component];
            let mut attrs = Vec::new();
            if let Some(class) = class {
                attrs.push(format!("className={{styles[{}]}}", json(class)));
            }
            for (ident, value) in props {
                attrs.push(format!("{ident}={{{}}}", json(value)));
            }
            if !texts.is_empty() {
                let pairs: Vec<String> = texts
                    .iter()
                    .map(|(k, v)| format!("{}: {}", json(k), json(v)))
                    .collect();
                attrs.push(format!("texts={{{{ {} }}}}", pairs.join(", ")));
            }
            let _ = writeln!(out, "{pad}{{/* {} */}}", name.replace("*/", "* /"));
            let _ = writeln!(out, "{pad}<{} {} />", c.name, attrs.join(" "));
        }
    }
}

/// The class of the frame's root element.
fn root_class(item: &Item) -> Option<&str> {
    match item {
        Item::Box { class, .. } | Item::Text { class, .. } | Item::Svg { class, .. } => Some(class),
        Item::Call { .. } => None,
    }
}

/// `text` as a JSON (and so a JavaScript) string literal.
fn json(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// `text` in lines of at most `width` characters, broken at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().expect("a line");
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(word.to_string());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
}
