//! Agent skills: the Skills page's catalog (skill packs from their official
//! sources), the skills folders of the agents on this Mac, which packs each
//! folder has, and the exact commands that install, update or remove a pack
//! there. A pack installs the way its source documents: the `skills` CLI
//! (`npx skills add owner/repo`, run with the package manager the user
//! picks), a Claude Code plugin from a marketplace (`claude plugin
//! marketplace add` and `claude plugin install`), or a copy of the skill's
//! folder from a git clone. Removing deletes the pack's skill folders from
//! the chosen agents' folders (`rm -rf` of `<folder>/<skill>`, a link or a
//! copy), or uninstalls its plugin.

mod catalog;
mod commands;
mod installed;
mod targets;
#[cfg(test)]
mod tests;

pub use catalog::{PACKS, get};
pub use commands::{Action, Env, Place, run, sample, script};
pub use installed::{Installed, Report, Status, read_installed, report, status};
pub use targets::{ClaudeHome, Target, find_targets, targets, targets_for};

/// What a pack is for: the page's category menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Design,
    Workflow,
    Platform,
    Media,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Design,
        Category::Workflow,
        Category::Platform,
        Category::Media,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Design => "Design & UI",
            Category::Workflow => "Workflow",
            Category::Platform => "Platforms & SDKs",
            Category::Media => "Video & Animation",
        }
    }
}

/// How a pack installs, as its source documents it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The `skills` CLI (skills.sh): `npx skills add <source>`, run with the
    /// package manager picked, globally (`-g`) for the chosen agents (`-a`),
    /// without questions (`-y`). `all`: every skill of the source (`--skill
    /// '*'`), else the pack's skills by name.
    SkillsCli { source: &'static str, all: bool },
    /// A Claude Code plugin: the marketplace (a GitHub `owner/repo`, added once
    /// per Claude Code profile under the name its `marketplace.json` gives),
    /// then `<plugin>@<marketplace>`.
    Plugin {
        repo: &'static str,
        marketplace: &'static str,
        plugin: &'static str,
    },
    /// A shallow clone of `url`, then each skill's folder under `path` copied
    /// into the agents' skills folders.
    Copy {
        url: &'static str,
        path: &'static str,
    },
}

impl Method {
    /// Its name for tools and the CLI.
    pub fn id(self) -> &'static str {
        match self {
            Method::SkillsCli { .. } => "skills-cli",
            Method::Plugin { .. } => "plugin",
            Method::Copy { .. } => "copy",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Method::SkillsCli { .. } => "skills CLI",
            Method::Plugin { .. } => "Claude Code plugin",
            Method::Copy { .. } => "Git clone & copy",
        }
    }

    /// Whether it runs through a JavaScript package manager (the choice the
    /// page offers).
    pub fn uses_package_manager(self) -> bool {
        matches!(self, Method::SkillsCli { .. })
    }

    /// Whether it can install into `target`: a plugin only into a Claude Code
    /// profile, the `skills` CLI only into the folders it knows.
    pub fn reaches(self, target: &Target) -> bool {
        match self {
            Method::SkillsCli { .. } => target.cli_agent.is_some(),
            Method::Plugin { .. } => target.claude.is_some(),
            Method::Copy { .. } => true,
        }
    }

    /// `name@marketplace`, for a plugin.
    pub fn plugin_key(self) -> Option<String> {
        match self {
            Method::Plugin {
                marketplace,
                plugin,
                ..
            } => Some(format!("{plugin}@{marketplace}")),
            _ => None,
        }
    }
}

/// A pack of skills from one source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pack {
    pub id: &'static str,
    pub name: &'static str,
    /// Who publishes it.
    pub by: &'static str,
    pub about: &'static str,
    pub category: Category,
    /// The skill folders it installs (each has a `SKILL.md`).
    pub skills: &'static [&'static str],
    /// The page that documents it and how to install it.
    pub home: &'static str,
    /// Its source repository.
    pub repo: &'static str,
    /// How it installs, the preferred way first.
    pub methods: &'static [Method],
    /// A requirement or caveat.
    pub note: Option<&'static str>,
}

impl Pack {
    pub fn method(&self, id: &str) -> Option<Method> {
        self.methods.iter().copied().find(|m| m.id() == id)
    }

    /// Its plugin, when it is one.
    pub fn plugin(&self) -> Option<Method> {
        self.methods
            .iter()
            .copied()
            .find(|m| matches!(m, Method::Plugin { .. }))
    }
}
