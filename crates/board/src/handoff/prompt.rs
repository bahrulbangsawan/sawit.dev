//! What the coding agent a design is handed to is asked: `prompt.md`.

use std::fmt::Write as _;
use std::path::Path;

use super::{Files, Target};

/// The design standard the prompt names: the skill Sawit ships, and the tool that reads it.
pub const STANDARD: &str = "Sawit's canvas design standard (the sawit-canvas-design skill; with \
     Sawit's MCP server, canvas_design_guide)";

/// The prompt that hands `frames` (with their files) to a coding agent: `design` is the picture
/// of them all, `spec` the spec, `out` the handoff's folder.
pub fn prompt(
    frames: &[(Target, Files)],
    design: Option<&Path>,
    spec: &Path,
    out: &Path,
) -> String {
    let flow = frames.len() > 1;
    let titles: Vec<String> = frames
        .iter()
        .map(|(t, _)| format!("\"{}\"", t.title))
        .collect();
    let mut text = match flow {
        true => format!(
            "Build the flow {} in this project, in its own stack: its framework, its components, \
             its styles and tokens, and its conventions for pages and routes. Make each a real, \
             working screen of the app, linked in this order.\n\n",
            titles.join(" → ")
        ),
        false => format!(
            "Build the screen {} in this project, in its own stack: its framework, its \
             components, its styles and tokens, and its conventions for pages and routes. Make it \
             a real, working screen of the app.\n\n",
            titles.join("")
        ),
    };
    let _ = writeln!(
        text,
        "It was designed on Sawit's canvas. Its files are in {}:",
        out.display()
    );
    if let Some(design) = design {
        let _ = writeln!(
            text,
            "- A picture of it as designed, at 2×: {}",
            design.display()
        );
    }
    let _ = writeln!(
        text,
        "- The spec: sizes and breakpoints, the layer tree, the tokens, components and variants, \
         the copy, the states drawn{}: {}",
        if flow {
            " and the links between the screens"
        } else {
            ""
        },
        spec.display()
    );
    for (i, (target, files)) in frames.iter().enumerate() {
        let what = match &target.screen {
            Some(name) => format!("the screen \"{}\" (HTML, screen \"{name}\")", target.title),
            None => format!("the frame \"{}\" (drawn in layers)", target.title),
        };
        let lead = match flow {
            true => format!("- {}. {what}", i + 1),
            false => format!("- For {what}"),
        };
        let mut parts = Vec::new();
        if let Some(png) = &files.png {
            parts.push(format!("its picture {}", png.display()));
        }
        if let Some(html) = &files.html {
            parts.push(format!("its HTML and CSS in one file {}", html.display()));
        }
        if let (Some(tsx), Some(css)) = (&files.tsx, &files.css) {
            parts.push(format!(
                "a React component, a mechanical conversion for reference: {} with {}",
                tsx.display(),
                css.display()
            ));
        }
        if let Some(source) = &files.source {
            parts.push(format!("its source folder {}", source.display()));
        }
        match parts.is_empty() {
            true => {
                let _ = writeln!(text, "{lead}.");
            }
            false => {
                let _ = writeln!(text, "{lead}: {}.", parts.join("; "));
            }
        }
    }
    let check = match design {
        Some(design) => format!(
            "When it runs, open it in Sawit's browser (browser_navigate, then browser_screenshot) \
             and compare it with {} until the layout, copy, colours, type and spacing match.",
            design.display()
        ),
        None => {
            "When it runs, open it in Sawit's browser and check it against the design.".to_string()
        }
    };
    let _ = write!(
        text,
        "\nMatch the design's layout, copy, colours, type and spacing. Reuse the project's \
         existing components and tokens where they fit rather than pasting the exported code: \
         the HTML and React files are references, not code to ship. Wire up what the design \
         implies (forms, links{}, states), and keep it accessible. Follow {STANDARD}: read it \
         before you start, and run its checklist before you say you're done. {check}\n",
        if flow { ", the order of the flow" } else { "" }
    );
    text
}
