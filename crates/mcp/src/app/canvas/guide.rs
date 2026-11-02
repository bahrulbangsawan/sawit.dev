//! What canvas_design_guide answers: Sawit's design standard for UI on the canvas, the SKILL.md of
//! the Canvas Design skill Sawit ships ([`sawit_core::skills::bundled`]), whole or one section of
//! it with the checklist.

/// The skill Sawit ships whose SKILL.md is the guide.
pub(super) const SKILL: &str = "sawit-canvas-design";

/// The section that goes with every other one: what to check before a design is done.
const CHECKLIST: &str = "checklist";

/// A heading of the guide: its level (how many `#`), its name, and the line it is on.
struct Heading<'a> {
    level: usize,
    name: &'a str,
    line: usize,
}

/// The guide's text.
pub(super) fn text() -> Result<&'static str, String> {
    sawit_core::skills::bundled::text(SKILL, "SKILL.md")
        .ok_or_else(|| "This Sawit was built without its design guide.".to_string())
}

/// What a section's name finds among the headings.
enum Found {
    One(usize),
    /// A part of each of these, and of no heading whole.
    Several(Vec<usize>),
    None,
}

/// The answer for `section`: the whole of `text` without one; else the section whose heading it
/// names, with the Checklist section after it; else an error that names the headings it could
/// mean, or when none, lists them all.
pub(super) fn answer(text: &str, section: Option<&str>) -> Result<String, String> {
    let Some(wanted) = section.map(key).filter(|wanted| !wanted.is_empty()) else {
        return Ok(text.to_string());
    };
    let asked = section.unwrap_or_default().trim();
    let lines: Vec<&str> = text.lines().collect();
    let headings = headings(&lines);
    let found = match find(&headings, &wanted) {
        Found::One(found) => found,
        Found::Several(all) => {
            let names: Vec<&str> = all.iter().map(|&i| headings[i].name).collect();
            return Err(format!(
                "“{asked}” names more than one section of the design guide: {}. Ask for one of them.",
                names.join(", ")
            ));
        }
        Found::None => {
            return Err(format!(
                "The design guide has no section “{asked}”. Its sections:\n{}",
                outline(&headings)
            ));
        }
    };
    let (start, end) = span(&headings, found, lines.len());
    let mut out = lines[start..end].join("\n").trim_end().to_string();
    if let Found::One(checklist) = find(&headings, CHECKLIST) {
        let (from, to) = span(&headings, checklist, lines.len());
        // Unless the section asked for holds it, or is it.
        if !(start..end).contains(&from) {
            out.push_str("\n\n");
            out.push_str(lines[from..to].join("\n").trim_end());
        }
    }
    Ok(out)
}

/// `name` compared without case, `#`, numbering marks or punctuation, and with British spelling
/// as American: "## 2. Canvas craft" is "2 canvas craft", "Colours" is "colors".
fn key(name: &str) -> String {
    let words = name.split(|c: char| !c.is_alphanumeric());
    let words: Vec<String> = words
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase().replace("colour", "color"))
        .collect();
    words.join(" ")
}

/// The heading `wanted` names: the one it is, else the heading it is part of ("craft" for
/// "Canvas craft"); a part of more than one heading is each of them.
fn find(headings: &[Heading], wanted: &str) -> Found {
    if let Some(exact) = headings.iter().position(|h| key(h.name) == wanted) {
        return Found::One(exact);
    }
    let part: Vec<usize> = (0..headings.len())
        .filter(|&i| key(headings[i].name).contains(wanted))
        .collect();
    match part[..] {
        [] => Found::None,
        [only] => Found::One(only),
        _ => Found::Several(part),
    }
}

/// The lines of the section under heading `at`: to the next heading of its level or above.
fn span(headings: &[Heading], at: usize, lines: usize) -> (usize, usize) {
    let heading = &headings[at];
    let next = headings[at + 1..].iter().find(|h| h.level <= heading.level);
    (heading.line, next.map_or(lines, |h| h.line))
}

/// The ATX headings of `lines` (`## Typography`), outside the frontmatter and fenced code blocks
/// (where `# a comment` is code, not a heading).
fn headings<'a>(lines: &[&'a str]) -> Vec<Heading<'a>> {
    let mut out = Vec::new();
    let mut skip = 0;
    if lines.first() == Some(&"---") {
        let end = lines[1..].iter().position(|l| matches!(*l, "---" | "..."));
        skip = end.map_or(0, |end| end + 2);
    }
    let mut fence: Option<char> = None;
    for (line, text) in lines.iter().enumerate().skip(skip) {
        let trimmed = text.trim_start();
        let marker = ['`', '~']
            .into_iter()
            .find(|&c| trimmed.starts_with(&c.to_string().repeat(3)));
        if let Some(marker) = marker {
            fence = match fence {
                None => Some(marker),
                Some(open) if open == marker => None,
                open => open,
            };
            continue;
        }
        if fence.is_some() || text.len() - trimmed.len() > 3 {
            continue;
        }
        let level = trimmed.chars().take_while(|&c| c == '#').count();
        let rest = &trimmed[level..];
        if (1..=6).contains(&level) && (rest.is_empty() || rest.starts_with([' ', '\t'])) {
            // A closing run of `#` goes when a space comes before it: "## Notes ##", not "C#".
            let name = rest.trim();
            let open = name.trim_end_matches('#');
            let name = match open.is_empty() || open.ends_with([' ', '\t']) {
                true => open.trim_end(),
                false => name,
            };
            out.push(Heading { level, name, line });
        }
    }
    out
}

/// The headings as a list, each under the one it belongs to; a document's one title left out.
fn outline(headings: &[Heading]) -> String {
    let top = headings.iter().map(|h| h.level).min().unwrap_or(1);
    let titled = headings.iter().filter(|h| h.level == top).count() == 1 && headings.len() > 1;
    let shown = headings.iter().filter(|h| !(titled && h.level == top));
    let base = if titled { top + 1 } else { top };
    let lines: Vec<String> = shown
        .map(|h| format!("{}- {}", "  ".repeat(h.level.saturating_sub(base)), h.name))
        .collect();
    lines.join("\n")
}
