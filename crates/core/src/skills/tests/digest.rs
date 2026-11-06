//! The design standard's digest is SKILL.md's own words, made by the build.

use crate::skills::bundled::{DESIGN_DIGEST, text};
use crate::skills::digest::{RULES, SKILL, digest, sections};

/// The digest the build made is the one SKILL.md makes now, every rule it carries is SKILL.md's
/// line for it, the checklist is SKILL.md's whole, and it fits the ~1–2k tokens the first UI
/// work is sent (about four characters a token).
#[test]
fn the_digest_matches_skill_md() {
    let skill = text(SKILL, "SKILL.md").expect("the design guide is bundled");
    assert_eq!(digest(skill).unwrap(), DESIGN_DIGEST);
    let lines: Vec<&str> = skill.lines().collect();
    for id in RULES {
        let start = format!("- **{id}** ");
        let rule = lines.iter().find(|l| l.starts_with(&start)).unwrap();
        assert!(DESIGN_DIGEST.contains(rule), "{id} isn't SKILL.md's line");
    }
    let at = skill.find("## Checklist").unwrap();
    assert!(DESIGN_DIGEST.contains(skill[at..].trim_end()));
    for section in sections(&lines) {
        assert!(DESIGN_DIGEST.contains(section), "{section}");
    }
    let chars = DESIGN_DIGEST.chars().count();
    assert!(
        (3_000..=8_500).contains(&chars),
        "the digest is {chars} characters"
    );
}

/// A rule the digest names that SKILL.md lost is an error (the build fails on it), not a gap.
#[test]
fn a_lost_rule_is_an_error() {
    let skill = text(SKILL, "SKILL.md").unwrap();
    let without = skill.replace("- **col.10** ", "- **col.x** ");
    assert_eq!(digest(&without).unwrap_err(), "SKILL.md has no rule col.10");
}
