//! What a `create-better-t-stack` release accepts, read from its `create --help`, against what
//! the New Project page offers: which flags and values are new, which are gone, so an upgrade
//! never runs the CLI with a flag or value it no longer takes.

use std::collections::{BTreeMap, BTreeSet};

use super::{Backend, Category, catalog, rules};

/// Each `--flag` of `create --help`, with its `choices` (empty for a flag without a list).
pub type Flags = BTreeMap<String, BTreeSet<String>>;

/// The flags and choices in `create --help`'s text. The help wraps a choice list over several
/// lines, each continued with spaces to the description column.
pub fn parse_help(text: &str) -> Flags {
    // Each option's block: its line starting `--`, and the lines after it that continue it.
    let mut blocks: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with("--") && indent <= 4 {
            blocks.push(trimmed.to_string());
        } else if trimmed.starts_with('-') && indent <= 4 {
            // A short option (`-h, --help`): not one Sawit passes.
            blocks.push(String::new());
        } else if indent > 4 && !trimmed.is_empty() {
            if let Some(last) = blocks.last_mut() {
                last.push(' ');
                last.push_str(trimmed);
            }
        } else {
            blocks.push(String::new());
        }
    }
    let mut flags = Flags::new();
    for block in blocks.iter().filter(|b| b.starts_with("--")) {
        let name: String = block.chars().take_while(|c| !c.is_whitespace()).collect();
        let name = name.trim_start_matches("--").to_string();
        if name.starts_with("no-") {
            // `--no-git`: the negation of `--git`.
            flags.entry(name).or_default();
            continue;
        }
        let mut choices = BTreeSet::new();
        if let Some(at) = block.find("choices:") {
            let rest = &block[at + "choices:".len()..];
            let end = rest.find(')').unwrap_or(rest.len());
            for part in rest[..end].split(',') {
                let value = part.trim().trim_matches('"');
                if !value.is_empty() {
                    choices.insert(value.to_string());
                }
            }
        }
        flags.insert(name, choices);
    }
    flags
}

/// The flag the page passes for a category's options.
pub fn flag(category: Category) -> &'static str {
    match category {
        Category::WebFrontend | Category::NativeFrontend => "frontend",
        Category::Backend => "backend",
        Category::Runtime => "runtime",
        Category::Api => "api",
        Category::Database => "database",
        Category::Orm => "orm",
        Category::DbSetup => "db-setup",
        Category::WebDeploy => "web-deploy",
        Category::ServerDeploy => "server-deploy",
        Category::Auth => "auth",
        Category::Payments => "payments",
        Category::PackageManager => "package-manager",
        Category::Addons => "addons",
        Category::Examples => "examples",
        Category::Git => "git",
        Category::Install => "install",
    }
}

/// The value the CLI gets for the page's option `id` of `category`: the id itself, but for a
/// backend that is the frontend's own server (`self-next`), which the CLI takes as `self`.
pub fn cli_value(category: Category, id: &'static str) -> &'static str {
    match (category, Backend::parse(id)) {
        (Category::Backend, Some(backend)) => rules::core_backend(backend),
        _ => id,
    }
}

/// What the page offers, as flags and values: every option's CLI value under its category's
/// flag, and the flags Create Project adds (`--directory-conflict error`, `--open none`,
/// `--manual-db`, `--yes`, `--no-git`, `--no-install`).
pub fn offered() -> Flags {
    let mut flags = Flags::new();
    for &category in Category::ALL {
        let values = flags.entry(flag(category).to_string()).or_default();
        if matches!(category, Category::Git | Category::Install) {
            continue;
        }
        for option in catalog::options(category) {
            values.insert(cli_value(category, option.id).to_string());
        }
    }
    for (f, value) in [("directory-conflict", "error"), ("open", "none")] {
        flags
            .entry(f.to_string())
            .or_default()
            .insert(value.to_string());
    }
    for f in ["manual-db", "yes", "no-git", "no-install"] {
        flags.entry(f.to_string()).or_default();
    }
    flags
}

/// What changed from what the page offers (`offered`) to what a release accepts (`accepted`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OptionsDiff {
    /// Flags the release has that the page doesn't pass, and values it accepts for a flag the
    /// page passes that the page doesn't offer: `--frontend solid-start`.
    pub new: Vec<String>,
    /// Flags the page passes that the release doesn't take, and values it no longer accepts:
    /// `--orm mongoose`. The page hides these while the release is in use.
    pub gone: Vec<String>,
}

impl OptionsDiff {
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.gone.is_empty()
    }

    /// Whether the page's option `id` of `category` is gone in the release: its value, or its
    /// whole flag.
    pub fn hides(&self, category: Category, id: &'static str) -> bool {
        let f = flag(category);
        let value = cli_value(category, id);
        self.gone
            .iter()
            .any(|g| *g == format!("--{f} {value}") || *g == format!("--{f}"))
    }

    /// The diff as the page says it: "New: `--frontend solid-start`. Gone: `--orm mongoose`."
    pub fn summary(&self) -> String {
        let list = |items: &[String]| items.join(", ");
        let mut parts = Vec::new();
        if !self.new.is_empty() {
            parts.push(format!("New: {}.", list(&self.new)));
        }
        if !self.gone.is_empty() {
            parts.push(format!("Gone: {}.", list(&self.gone)));
        }
        parts.join(" ")
    }
}

/// The pinned release's own `create --help` ([`super::CLI_VERSION`]'s), which a newer release's
/// is compared with for what is new. A Sawit release that ports a newer CLI replaces it.
pub const PINNED_HELP: &str = include_str!("fixtures/help-3.49.0.txt");

/// The flags and choices of the pinned release.
pub fn pinned() -> Flags {
    parse_help(PINNED_HELP)
}

/// What a release (`accepted`) changes: what it takes that the pinned release (`pinned`) didn't
/// (new), and what the page offers (`offered`) that it no longer takes (gone).
pub fn diff(offered: &Flags, pinned: &Flags, accepted: &Flags) -> OptionsDiff {
    let mut out = OptionsDiff::default();
    for (flag, values) in offered {
        match accepted.get(flag) {
            None => out.gone.push(format!("--{flag}")),
            Some(taken) if !taken.is_empty() => {
                for v in values.difference(taken) {
                    out.gone.push(format!("--{flag} {v}"));
                }
            }
            Some(_) => {}
        }
    }
    for (flag, taken) in accepted {
        match pinned.get(flag) {
            None => out.new.push(format!("--{flag}")),
            Some(before) => {
                for v in taken.difference(before) {
                    out.new.push(format!("--{flag} {v}"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP: &str = PINNED_HELP;

    #[test]
    fn the_help_lists_flags_and_their_choices() {
        let flags = parse_help(HELP);
        let orm: Vec<&str> = flags["orm"].iter().map(String::as_str).collect();
        assert_eq!(orm, ["drizzle", "mongoose", "none", "prisma"]);
        // A list wrapped over several lines is read whole.
        assert!(flags["frontend"].contains("native-unistyles"));
        assert!(flags["frontend"].contains("none"));
        assert!(flags["addons"].contains("evlog"));
        assert!(flags["git"].is_empty());
        assert!(flags.contains_key("no-git"));
        assert!(flags.contains_key("manual-db"));
        assert!(!flags.contains_key("help"));
    }

    /// The release Sawit's data was ported from takes everything the page offers.
    #[test]
    fn the_pinned_release_takes_every_option_the_page_offers() {
        let diff = diff(&offered(), &pinned(), &parse_help(HELP));
        assert!(diff.is_empty(), "{diff:?}");
    }

    /// A newer release that adds a frontend and drops an ORM: the diff names both, and the
    /// page hides the gone one.
    #[test]
    fn a_newer_release_adds_and_drops_options() {
        let newer = HELP
            .replace(
                "\"tanstack-router\", \"react-router\",",
                "\"tanstack-router\", \"react-router\", \"solid-start\",",
            )
            .replace("\"mongoose\", ", "");
        let diff = diff(&offered(), &pinned(), &parse_help(&newer));
        assert_eq!(diff.new, ["--frontend solid-start"]);
        assert_eq!(diff.gone, ["--orm mongoose"]);
        assert!(diff.hides(Category::Orm, "mongoose"));
        assert!(!diff.hides(Category::Orm, "drizzle"));
        assert_eq!(
            diff.summary(),
            "New: --frontend solid-start. Gone: --orm mongoose."
        );
    }

    #[test]
    fn a_self_backend_is_checked_as_self() {
        assert_eq!(cli_value(Category::Backend, "hono"), "hono");
        let selfs: Vec<_> = catalog::options(Category::Backend)
            .iter()
            .filter(|o| o.id.starts_with("self"))
            .map(|o| cli_value(Category::Backend, o.id))
            .collect();
        assert!(selfs.iter().all(|v| *v == "self"), "{selfs:?}");
    }
}
