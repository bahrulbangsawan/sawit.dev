//! The JavaScript package managers that run a package once (`npx`, `pnpm
//! dlx`, `bunx`, `yarn dlx`) or install one for every project (`npm install
//! -g`, `pnpm add -g`, `bun add -g`, `yarn global add`): how each spells
//! those commands, which of them are on this Mac, and the one the user picked
//! last. The Skills page, the MCP page and the Dev Tools page offer the
//! choice wherever an install goes through one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Result;
use crate::instances::AppPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Bun,
    Yarn,
}

/// What a command does with a package: run it once, or install it for every
/// project. Yarn 1 can only install, Yarn 2 and later can only run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Run,
    Global,
}

impl PackageManager {
    pub const ALL: [PackageManager; 4] = [
        PackageManager::Npm,
        PackageManager::Pnpm,
        PackageManager::Bun,
        PackageManager::Yarn,
    ];

    /// Its name, as the choice is shown, kept and passed to tools.
    pub fn id(self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Bun => "bun",
            PackageManager::Yarn => "yarn",
        }
    }

    /// `npm`, `pnpm`, `bun` or `yarn`, any case, or the program that runs a
    /// package (`npx`, `bunx`).
    pub fn parse(text: &str) -> Option<PackageManager> {
        match text.trim().to_ascii_lowercase().as_str() {
            "npm" | "npx" => Some(PackageManager::Npm),
            "pnpm" => Some(PackageManager::Pnpm),
            "bun" | "bunx" => Some(PackageManager::Bun),
            "yarn" => Some(PackageManager::Yarn),
            _ => None,
        }
    }

    /// The program that runs a package once.
    pub fn runner(self) -> &'static str {
        match self {
            PackageManager::Npm => "npx",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Bun => "bunx",
            PackageManager::Yarn => "yarn",
        }
    }

    /// What goes between the runner and the package: `npx -y pkg` (no
    /// question before the download), `pnpm dlx pkg`, `bunx pkg`, `yarn dlx
    /// pkg`.
    pub fn runner_args(self) -> &'static [&'static str] {
        match self {
            PackageManager::Npm => &["-y"],
            PackageManager::Pnpm | PackageManager::Yarn => &["dlx"],
            PackageManager::Bun => &[],
        }
    }

    /// The runner and its words, then `args` (the package and what it's
    /// given): `["bunx", "skills", "add", …]`.
    pub fn run_words(self, args: &[&str]) -> Vec<String> {
        std::iter::once(self.runner())
            .chain(self.runner_args().iter().copied())
            .chain(args.iter().copied())
            .map(String::from)
            .collect()
    }

    /// [`Self::run_words`] as one shell command line.
    pub fn run_line(self, args: &[&str]) -> String {
        let words = self.run_words(args);
        words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
    }

    /// The command that installs `package` for every project, the way its
    /// documentation spells it.
    pub fn global_install(self, package: &str) -> String {
        let package = quote(package);
        match self {
            PackageManager::Npm => format!("npm install -g {package}"),
            PackageManager::Pnpm => format!("pnpm add -g {package}"),
            PackageManager::Bun => format!("bun add -g {package}"),
            PackageManager::Yarn => format!("yarn global add {package}"),
        }
    }

    /// The command that adds `package` (`wrangler@latest`) to the project in the folder it runs
    /// in as a dev dependency, or brings it up to date there.
    pub fn dev_install(self, package: &str) -> String {
        let package = quote(package);
        match self {
            PackageManager::Npm => format!("npm install -D {package}"),
            PackageManager::Pnpm => format!("pnpm add -D {package}"),
            PackageManager::Bun => format!("bun add -d {package}"),
            PackageManager::Yarn => format!("yarn add -D {package}"),
        }
    }

    /// The command that removes a package [`Self::global_install`] put in place.
    pub fn global_uninstall(self, package: &str) -> String {
        let package = quote(package);
        match self {
            PackageManager::Npm => format!("npm uninstall -g {package}"),
            PackageManager::Pnpm => format!("pnpm remove -g {package}"),
            PackageManager::Bun => format!("bun remove -g {package}"),
            PackageManager::Yarn => format!("yarn global remove {package}"),
        }
    }

    /// Which package manager installed the global package whose files are at
    /// `real` (a program's path with its links followed): Bun keeps them in
    /// `~/.bun/install/global`, pnpm in its `global` folder, Yarn 1 in
    /// `~/.config/yarn/global`; anything else is npm's.
    pub fn owning(real: &str) -> PackageManager {
        if real.contains("/.bun/install/global/") {
            PackageManager::Bun
        } else if real.contains("/pnpm/global/") || real.contains("/.pnpm-global/") {
            PackageManager::Pnpm
        } else if real.contains("/yarn/global/node_modules/") {
            PackageManager::Yarn
        } else {
            PackageManager::Npm
        }
    }
}

/// `text` as one word for the shell: as it is when it has nothing the shell
/// reads specially, else in single quotes.
pub fn quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%+=:,./_-".contains(c));
    if plain {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// A package manager found on this Mac.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub pm: PackageManager,
    /// Where its runner is ([`PackageManager::runner`]).
    pub runner: PathBuf,
    /// `yarn --version`, for Yarn: which commands it has.
    pub version: Option<String>,
}

impl Available {
    /// Whether it can do `what`: Yarn 1 has no `dlx`, and Yarn 2 and later no
    /// `global`.
    pub fn can(&self, what: Use) -> bool {
        if self.pm != PackageManager::Yarn {
            return true;
        }
        let major: u64 = self
            .version
            .as_deref()
            .and_then(|v| v.trim().split('.').next()?.parse().ok())
            .unwrap_or(1);
        match what {
            Use::Run => major >= 2,
            Use::Global => major < 2,
        }
    }
}

/// The package managers on this Mac, in [`PackageManager::ALL`]'s order,
/// found where the Dev Tools page finds programs.
pub async fn detect() -> Vec<Available> {
    let found = detect_with(crate::dev_tools::find_program);
    let mut out = Vec::new();
    for mut available in found {
        if available.pm == PackageManager::Yarn {
            available.version =
                crate::cmd::run(&available.runner, ["--version"], Duration::from_secs(5))
                    .await
                    .ok()
                    .map(|o| o.stdout.trim().to_string())
                    .filter(|v| !v.is_empty());
        }
        out.push(available);
    }
    out
}

/// [`detect`] with `find` looking programs up, Yarn's version unread.
pub fn detect_with(find: impl Fn(&str) -> Option<PathBuf>) -> Vec<Available> {
    PackageManager::ALL
        .into_iter()
        .filter_map(|pm| {
            Some(Available {
                pm,
                runner: find(pm.runner())?,
                version: None,
            })
        })
        .collect()
}

/// The ones of `available` that can do `what`.
pub fn usable(available: &[Available], what: Use) -> Vec<&Available> {
    available.iter().filter(|a| a.can(what)).collect()
}

/// The package manager to use for `what`: `wanted` when it's on this Mac and
/// can, else the one picked last when it can, else the first that can.
pub fn choose(
    available: &[Available],
    wanted: Option<PackageManager>,
    remembered: Option<PackageManager>,
    what: Use,
) -> Option<PackageManager> {
    let usable = usable(available, what);
    let has = |pm: Option<PackageManager>| pm.filter(|pm| usable.iter().any(|a| a.pm == *pm));
    has(wanted)
        .or(has(remembered))
        .or(usable.first().map(|a| a.pm))
}

/// Where the last choice is kept, in Sawit's data folder.
fn choice_file(paths: &AppPaths) -> PathBuf {
    paths.root.join("package-manager")
}

/// The package manager the user picked last, on any page.
pub fn remembered(paths: &AppPaths) -> Option<PackageManager> {
    remembered_in(&choice_file(paths))
}

fn remembered_in(file: &Path) -> Option<PackageManager> {
    PackageManager::parse(&std::fs::read_to_string(file).ok()?)
}

/// Keeps `pm` as the choice the pages and tools start from.
pub fn remember(paths: &AppPaths, pm: PackageManager) -> Result<()> {
    let file = choice_file(paths);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::files::write_text_atomic(&file, &format!("{}\n", pm.id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `npx -y pkg` → `bunx pkg` → `pnpm dlx pkg` → `yarn dlx pkg`: each one's
    /// own way to run a package once, without a question first.
    #[test]
    fn each_runs_a_package_its_own_way() {
        let args = ["api-anything", "mcp"];
        let line = |pm: PackageManager| pm.run_line(&args);
        assert_eq!(line(PackageManager::Npm), "npx -y api-anything mcp");
        assert_eq!(line(PackageManager::Bun), "bunx api-anything mcp");
        assert_eq!(line(PackageManager::Pnpm), "pnpm dlx api-anything mcp");
        assert_eq!(line(PackageManager::Yarn), "yarn dlx api-anything mcp");
        assert_eq!(
            PackageManager::Pnpm.run_words(&["skills", "add", "x/y"]),
            ["pnpm", "dlx", "skills", "add", "x/y"]
        );
        // What the shell reads specially is quoted.
        assert_eq!(
            PackageManager::Npm.run_line(&["skills", "add", "a/b", "--skill", "*"]),
            "npx -y skills add a/b --skill '*'"
        );
    }

    /// The global installs the cf docs list, one per package manager, and the
    /// commands that undo them.
    #[test]
    fn each_installs_globally_its_own_way() {
        let all: Vec<String> = PackageManager::ALL
            .iter()
            .map(|pm| pm.global_install("cf"))
            .collect();
        assert_eq!(
            all,
            [
                "npm install -g cf",
                "pnpm add -g cf",
                "bun add -g cf",
                "yarn global add cf"
            ]
        );
        assert_eq!(
            PackageManager::Bun.global_uninstall("@google/gemini-cli"),
            "bun remove -g @google/gemini-cli"
        );
        assert_eq!(
            PackageManager::Yarn.global_uninstall("cf"),
            "yarn global remove cf"
        );
    }

    #[test]
    fn the_owner_of_a_global_package_is_read_from_its_path() {
        let owner = |p: &str| PackageManager::owning(p);
        assert_eq!(
            owner("/Users/me/.bun/install/global/node_modules/@google/gemini-cli/dist/index.js"),
            PackageManager::Bun
        );
        assert_eq!(
            owner("/Users/me/Library/pnpm/global/5/.pnpm/cf@1.0.0/node_modules/cf/bin/cf"),
            PackageManager::Pnpm
        );
        assert_eq!(
            owner("/Users/me/.config/yarn/global/node_modules/cf/bin/cf"),
            PackageManager::Yarn
        );
        assert_eq!(
            owner("/Users/me/.nvm/versions/node/v22.11.0/lib/node_modules/cf/bin/cf"),
            PackageManager::Npm
        );
    }

    #[test]
    fn names_and_runners_parse() {
        for pm in PackageManager::ALL {
            assert_eq!(PackageManager::parse(pm.id()), Some(pm));
        }
        assert_eq!(PackageManager::parse(" NPX "), Some(PackageManager::Npm));
        assert_eq!(PackageManager::parse("bunx"), Some(PackageManager::Bun));
        assert_eq!(PackageManager::parse("deno"), None);
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("$HOME"), "'$HOME'");
    }

    fn found(pm: PackageManager, version: Option<&str>) -> Available {
        Available {
            pm,
            runner: PathBuf::from(format!("/bin/{}", pm.runner())),
            version: version.map(String::from),
        }
    }

    /// Only what's installed is offered, Yarn only for what its version has;
    /// the choice is the one asked for, else the last one, else the first.
    #[test]
    fn the_choice_is_one_that_is_installed_and_can() {
        let find = |name: &str| {
            ["npx", "bunx"]
                .contains(&name)
                .then(|| PathBuf::from(format!("/x/{name}")))
        };
        let pms: Vec<PackageManager> = detect_with(find).iter().map(|a| a.pm).collect();
        assert_eq!(pms, [PackageManager::Npm, PackageManager::Bun]);

        let classic = found(PackageManager::Yarn, Some("1.22.22"));
        let berry = found(PackageManager::Yarn, Some("4.5.0"));
        assert!(classic.can(Use::Global) && !classic.can(Use::Run));
        assert!(berry.can(Use::Run) && !berry.can(Use::Global));
        assert!(found(PackageManager::Yarn, None).can(Use::Global));

        let available = [
            found(PackageManager::Npm, None),
            berry,
            found(PackageManager::Bun, None),
        ];
        let pick = |wanted, last, what| choose(&available, wanted, last, what);
        assert_eq!(pick(None, None, Use::Run), Some(PackageManager::Npm));
        assert_eq!(
            pick(None, Some(PackageManager::Bun), Use::Run),
            Some(PackageManager::Bun)
        );
        assert_eq!(
            pick(
                Some(PackageManager::Yarn),
                Some(PackageManager::Bun),
                Use::Run
            ),
            Some(PackageManager::Yarn)
        );
        // Yarn 4 can't install globally; pnpm isn't here.
        assert_eq!(
            pick(Some(PackageManager::Yarn), None, Use::Global),
            Some(PackageManager::Npm)
        );
        assert_eq!(
            pick(Some(PackageManager::Pnpm), None, Use::Run),
            Some(PackageManager::Npm)
        );
        assert_eq!(choose(&[], Some(PackageManager::Npm), None, Use::Run), None);
    }

    #[test]
    fn the_choice_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            root: dir.path().join("sawit"),
        };
        assert_eq!(remembered(&paths), None);
        remember(&paths, PackageManager::Pnpm).unwrap();
        assert_eq!(remembered(&paths), Some(PackageManager::Pnpm));
        std::fs::write(choice_file(&paths), "nope").unwrap();
        assert_eq!(remembered(&paths), None);
    }
}
