//! canvas_design_guide: Sawit's design standard whole, one section of it with the checklist, and
//! the headings when no section has the name asked for.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use sawit_core::instances::AppPaths;
use sawit_core::skills::bundled;

use super::super::CanvasDesignGuideArgs;
use super::super::guide::answer;
use crate::Sawit;

/// The guide's answer for `section`, as the tool gives it.
async fn guide(section: Option<&str>) -> CallToolResult {
    let sawit = Sawit::new(AppPaths {
        root: std::env::temp_dir(),
    });
    let args = CanvasDesignGuideArgs {
        section: section.map(str::to_string),
    };
    sawit.canvas_design_guide(Parameters(args)).await
}

fn said(result: &CallToolResult) -> String {
    let texts = result.content.iter().filter_map(|c| c.as_text());
    texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
}

/// The SKILL.md Sawit ships, which the Canvas Design skill installs.
fn skill_md() -> &'static str {
    bundled::text("sawit-canvas-design", "SKILL.md").unwrap()
}

/// The line of the skill's heading `name`, at whatever level the guide has it.
fn heading_line(name: &str) -> &'static str {
    let line = skill_md().lines().find(|line| {
        let words = line.trim_start_matches('#');
        line.starts_with('#') && words.trim() == name
    });
    line.unwrap_or_else(|| panic!("the guide has no heading {name}"))
}

/// Without a section the guide is that SKILL.md, whole.
#[tokio::test]
async fn the_guide_is_the_skill_sawit_ships() {
    let whole = guide(None).await;
    assert_ne!(whole.is_error, Some(true), "{}", said(&whole));
    assert_eq!(said(&whole), skill_md());
    // A blank section is none.
    assert_eq!(said(&guide(Some("  ")).await), skill_md());
}

/// A section is the one its heading names, in any case or form, with the Checklist section after
/// it, and nothing of the sections between; the Checklist itself comes once.
#[tokio::test]
async fn a_section_comes_with_the_checklist() {
    let checklist = said(&guide(Some("Checklist")).await);
    assert!(
        checklist.starts_with(heading_line("Checklist")),
        "{checklist}"
    );
    assert_eq!(checklist.matches(heading_line("Checklist")).count(), 1);

    let typography = said(&guide(Some("Typography")).await);
    let start = format!("{}\n", heading_line("Typography"));
    assert!(typography.starts_with(&start), "{typography}");
    assert!(
        typography.ends_with(&format!("\n\n{checklist}")),
        "{typography}"
    );
    for other in ["Layout", "Color", "Canvas craft"] {
        assert!(
            !typography.contains(heading_line(other)),
            "{other}: {typography}"
        );
    }
    for asked in ["typography", "  ## TYPOGRAPHY "] {
        assert_eq!(said(&guide(Some(asked)).await), typography, "{asked}");
    }
    // A word of one heading only finds it.
    let craft = said(&guide(Some("craft")).await);
    assert!(craft.starts_with(heading_line("Canvas craft")), "{craft}");
    // Colour is Color.
    let color = said(&guide(Some("Color")).await);
    assert!(color.starts_with(heading_line("Color")), "{color}");
    assert_eq!(said(&guide(Some("colour")).await), color);
}

/// A section no heading names is an error that lists the guide's headings, to ask again: the
/// sections the standard has.
#[tokio::test]
async fn an_unknown_section_lists_the_headings() {
    let unknown = guide(Some("Sawit Palsu")).await;
    assert_eq!(unknown.is_error, Some(true));
    let text = said(&unknown);
    assert!(text.contains("no section “Sawit Palsu”"), "{text}");
    let listed: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("- "))
        .collect();
    for heading in [
        "Layout",
        "Typography",
        "Color",
        "Spacing",
        "Copywriting",
        "Components",
        "States",
        "Accessibility",
        "Motion",
        "Polish",
        "Canvas craft",
        "Checklist",
    ] {
        assert!(listed.contains(&heading), "{heading}: {text}");
    }
}

/// How a guide is read: the frontmatter and code blocks have no headings, a section holds its
/// subsections and ends at the next heading of its level, a closing run of `#` isn't part of a
/// name, colour is color, and a word that names two headings names neither: the error names
/// both, apart from one for a name no heading has.
#[test]
fn sections_are_read_as_markdown_has_them() {
    const DOC: &str = "\
---
name: x
# a YAML comment
---

# Title

Intro.

## Layout

Grid.

### Columns

Twelve.

```sh
# a shell comment
```

## Color ##

Ink.

## Checklist

- [ ] Done.
";
    let checklist = "## Checklist\n\n- [ ] Done.";
    assert_eq!(answer(DOC, None).unwrap(), DOC);
    assert_eq!(
        answer(DOC, Some("layout")).unwrap(),
        format!(
            "## Layout\n\nGrid.\n\n### Columns\n\nTwelve.\n\n```sh\n# a shell comment\n```\n\n{checklist}"
        )
    );
    assert_eq!(
        answer(DOC, Some("Columns")).unwrap(),
        format!("### Columns\n\nTwelve.\n\n```sh\n# a shell comment\n```\n\n{checklist}")
    );
    for color in ["color", "Colour", " COLOUR "] {
        assert_eq!(
            answer(DOC, Some(color)).unwrap(),
            format!("## Color ##\n\nInk.\n\n{checklist}"),
            "{color}"
        );
    }
    assert_eq!(answer(DOC, Some("checklist")).unwrap(), checklist);
    // The title's section holds the checklist already.
    let title = answer(DOC, Some("Title")).unwrap();
    assert!(
        title.starts_with("# Title\n") && title.ends_with(checklist),
        "{title}"
    );
    assert_eq!(title.matches("## Checklist").count(), 1);
    // "col" is in Columns and Color.
    for (asked, said) in [("col", "col"), (" COL ", "COL")] {
        assert_eq!(
            answer(DOC, Some(asked)).unwrap_err(),
            format!(
                "“{said}” names more than one section of the design guide: Columns, Color. Ask for one of them."
            )
        );
    }
    // A comment is no heading.
    for unknown in ["a shell comment", "a YAML comment", "colour wheel"] {
        let listed = answer(DOC, Some(unknown)).unwrap_err();
        assert!(
            listed.starts_with(&format!("The design guide has no section “{unknown}”.")),
            "{unknown}: {listed}"
        );
        assert!(
            listed.ends_with("Its sections:\n- Layout\n  - Columns\n- Color\n- Checklist"),
            "{unknown}: {listed}"
        );
    }
}
