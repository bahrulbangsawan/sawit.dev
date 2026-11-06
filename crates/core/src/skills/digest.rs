//! The digest of Sawit's design standard: the part of `sawit-canvas-design/SKILL.md` an agent
//! gets with its first UI work on the canvas, without asking for it (`canvas_begin` in
//! `sawit-mcp`). It is made from SKILL.md by the build (crates/core/build.rs includes this file),
//! so it can't go stale: the floors, the rules the machine checks run, the checklist, and the
//! sections the whole guide has, each line as SKILL.md writes it. A rule named here that SKILL.md
//! no longer has fails the build.
//!
//! Only `std` here: the build script compiles this file on its own.

/// The skill whose SKILL.md the digest is made from.
pub const SKILL: &str = "sawit-canvas-design";

/// The rules the digest carries whole: the floors that never yield and the ones Sawit checks
/// after each write (contrast, targets, text sizes, the spacing scale, focus, labels and alt
/// text, text that overflows), with how UI is built on the canvas first.
pub const RULES: &[&str] = &[
    "cnv.3", "col.10", "typ.11", "a11y.2", "spc.1", "sta.8", "a11y.8", "a11y.7", "lay.18",
];

/// The `## ` sections of the guide the digest doesn't list: where the standard came from.
const UNLISTED: &[&str] = &["Sources"];

/// The digest of `skill` (the text of SKILL.md), or what it lacks.
pub fn digest(skill: &str) -> Result<String, String> {
    let lines: Vec<&str> = skill.lines().collect();
    let floors = lines
        .iter()
        .find(|line| line.starts_with("- Values are starting points"))
        .ok_or("SKILL.md has no line on the floors (\"- Values are starting points…\")")?;
    let mut rules = Vec::new();
    for id in RULES {
        let start = format!("- **{id}** ");
        let line = lines
            .iter()
            .find(|line| line.starts_with(&start))
            .ok_or_else(|| format!("SKILL.md has no rule {id}"))?;
        rules.push(*line);
    }
    let sections = sections(&lines);
    let checklist = section(&lines, "Checklist").ok_or("SKILL.md has no ## Checklist")?;
    let mut out = String::from(
        "# Sawit's design standard, in short\n\n\
         Every UI you build on Sawit's canvas follows Sawit's design standard (the \
         sawit-canvas-design skill). This is its digest: the floors, the rules Sawit checks \
         after each canvas_screen_write and when you finish with canvas_progress {status: \
         \"done\"}, and the checklist. Read the section your work needs before you build it: \
         canvas_design_guide {section: \"Components\"}, or the MCP resource \
         sawit://design-guide/<section> (sawit://design-guide is the whole guide). Later \
         canvas_begin answers bring the sections their `steps` name (\"Form\", \"Table\", \
         \"Navigation\", \"States\").\n\n",
    );
    out.push_str(&format!("Sections: {}.\n\n", sections.join(", ")));
    out.push_str(floors);
    out.push_str("\n\n## The rules Sawit checks\n\n");
    for rule in rules {
        out.push_str(rule);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(checklist.trim_end());
    out.push('\n');
    Ok(out)
}

/// The names of the `## ` sections of `lines`, outside fenced code, but [`UNLISTED`].
pub fn sections<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut fenced = false;
    let mut out = Vec::new();
    for line in lines {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if let Some(name) = line.strip_prefix("## ").filter(|_| !fenced) {
            let name = name.trim();
            if !UNLISTED.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// The `## name` section of `lines`, its heading included, to the next `## ` or `# ` heading.
fn section(lines: &[&str], name: &str) -> Option<String> {
    let heading = format!("## {name}");
    let start = lines.iter().position(|line| line.trim_end() == heading)?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.starts_with("## ") || line.starts_with("# "))
        .map_or(lines.len(), |at| start + 1 + at);
    Some(lines[start..end].join("\n"))
}
