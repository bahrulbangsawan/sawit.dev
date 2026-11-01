//! Creating a project with the New Project page's stack builder ([`crate::stack_builder`]),
//! what the page, `sawit-mcp` and `sawit new` share: the choices the page remembers, where a
//! project goes by default, a free name there, the checks before a create, whether the create
//! made a project, and the create itself without the app.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::instances::AppPaths;
use crate::stack_builder::{self, DEFAULT_NAME, Stack};

/// What the New Project page keeps between openings: the last stack (its name aside), the
/// folder projects were created in, and the package manager, which is the stack's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remembered {
    pub stack: Stack,
    pub location: Option<PathBuf>,
}

/// The file the page keeps them in, in Sawit's data folder.
pub fn remembered_file(paths: &AppPaths) -> PathBuf {
    paths.root.join("new-project.json")
}

/// What the page kept, or nothing when there is no file or it isn't one Sawit wrote.
pub fn load(paths: &AppPaths) -> Option<Remembered> {
    let text = std::fs::read_to_string(remembered_file(paths)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Keeps `remembered` for the next time the page opens.
pub fn save(paths: &AppPaths, remembered: &Remembered) -> Result<()> {
    let file = remembered_file(paths);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(remembered)
        .map_err(|e| crate::CoreError::Config(e.to_string()))?;
    crate::files::write_text_atomic(&file, &format!("{text}\n"))
}

/// Where a new project goes when none was picked: beside the open project, else
/// `~/Developer` when there is one, else the home folder.
pub fn default_location(open_project: Option<&Path>, home: &Path) -> PathBuf {
    if let Some(parent) = open_project.and_then(Path::parent).filter(|p| p.is_dir()) {
        return parent.to_path_buf();
    }
    let developer = home.join("Developer");
    if developer.is_dir() {
        developer
    } else {
        home.to_path_buf()
    }
}

/// `base`, or `base-2`, `base-3`, … : the first that isn't in `location` yet.
pub fn free_name(location: &Path, base: &str) -> String {
    let base = if base.trim().is_empty() {
        DEFAULT_NAME
    } else {
        base
    };
    if !location.join(base).exists() {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !location.join(name).exists())
        .unwrap_or_else(|| base.to_string())
}

/// The folder creating `stack` in `location` makes, once the name is one the CLI takes,
/// `location` is a folder and nothing is there yet; else why it can't start.
pub fn target(location: &Path, stack: &Stack) -> std::result::Result<PathBuf, String> {
    if let Some(error) = stack_builder::validate_project_name(&stack.project_name) {
        return Err(error);
    }
    if !location.is_absolute() {
        return Err(format!(
            "{} isn't an absolute path: pick the folder to create the project in.",
            location.display()
        ));
    }
    if !location.is_dir() {
        return Err(format!("{} isn't a folder.", tilde(location)));
    }
    let dir = location.join(stack_builder::format_project_name(&stack.project_name));
    if dir.exists() {
        return Err(format!(
            "{} already exists. Pick another name or location.",
            tilde(&dir)
        ));
    }
    Ok(dir)
}

/// Whether the create made a project in `dir`: the CLI writes its `package.json` first.
pub fn created(dir: &Path) -> bool {
    dir.join("package.json").is_file()
}

/// `~/…` for a path in the home folder.
fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    crate::recent::tildify(path, home.as_deref())
}

/// The shell line that creates `stack` in `location`: there, then [`stack_builder::run_command`].
pub fn shell_line(location: &Path, stack: &Stack) -> String {
    format!(
        "cd {} && {}",
        crate::package_manager::quote(&location.to_string_lossy()),
        stack_builder::run_command(stack)
    )
}

/// Creates `stack`'s project in `location` without the app: the run command in the login
/// shell, with no terminal to ask in (an addon's setup question then fails rather than waits),
/// its output to `on_line`. Returns the project's folder once it has a `package.json`.
pub async fn create(
    location: &Path,
    stack: &Stack,
    on_line: impl Fn(String) + Send + Sync,
) -> std::result::Result<PathBuf, String> {
    let dir = target(location, stack)?;
    let line = shell_line(location, stack);
    let ran = crate::dev_tools::run_shell("new-project", "create-better-t-stack", &line, on_line)
        .await
        .map_err(|e| e.to_string());
    match ran {
        Ok(()) if created(&dir) => Ok(dir),
        Ok(()) => Err(format!(
            "create-better-t-stack finished, but {} has no package.json.",
            tilde(&dir)
        )),
        // The CLI may fail after the project is written (an install step, a git step): it is
        // still a project.
        Err(e) if created(&dir) => Err(format!(
            "{} was created, but a step after it failed: {e}",
            tilde(&dir)
        )),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sawit-new-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Beside the open project; else ~/Developer when there is one; else home.
    #[test]
    fn a_project_goes_beside_the_open_one_or_in_developer() {
        let home = scratch("home");
        let open = home.join("work/shop");
        std::fs::create_dir_all(&open).unwrap();
        assert_eq!(default_location(Some(&open), &home), home.join("work"));
        assert_eq!(default_location(None, &home), home);
        std::fs::create_dir_all(home.join("Developer")).unwrap();
        assert_eq!(default_location(None, &home), home.join("Developer"));
    }

    /// The first name not taken, so a page opened again doesn't start on a folder that exists.
    #[test]
    fn the_name_is_the_first_free_one() {
        let dir = scratch("names");
        assert_eq!(free_name(&dir, DEFAULT_NAME), DEFAULT_NAME);
        std::fs::create_dir_all(dir.join(DEFAULT_NAME)).unwrap();
        std::fs::create_dir_all(dir.join("my-better-t-app-2")).unwrap();
        assert_eq!(free_name(&dir, DEFAULT_NAME), "my-better-t-app-3");
        assert_eq!(free_name(&dir, ""), "my-better-t-app-3");
    }

    /// A create never starts over a folder that exists, in a location that isn't a folder, or
    /// with a name the CLI refuses.
    #[test]
    fn a_create_refuses_what_it_would_overwrite() {
        let dir = scratch("target");
        let stack = |name: &str| Stack {
            project_name: name.into(),
            ..Stack::default()
        };
        assert_eq!(target(&dir, &stack("shop")), Ok(dir.join("shop")));
        assert_eq!(
            target(&dir, &stack("my shop")),
            Ok(dir.join("my-shop")),
            "spaces become dashes, as in the command"
        );
        std::fs::create_dir_all(dir.join("shop")).unwrap();
        assert!(
            target(&dir, &stack("shop"))
                .unwrap_err()
                .contains("already exists")
        );
        assert!(
            target(&dir.join("nope"), &stack("x"))
                .unwrap_err()
                .contains("isn't a folder")
        );
        assert!(target(Path::new("rel"), &stack("x")).is_err());
        assert!(target(&dir, &stack(".x")).unwrap_err().contains("dot"));
        assert!(!created(&dir.join("shop")));
        std::fs::write(dir.join("shop/package.json"), "{}").unwrap();
        assert!(created(&dir.join("shop")));
    }

    /// The page's choices come back as they were kept; a file that isn't Sawit's is ignored.
    #[test]
    fn the_last_choices_come_back() {
        let paths = AppPaths {
            root: scratch("remembered"),
        };
        assert_eq!(load(&paths), None);
        let kept = Remembered {
            stack: Stack {
                backend: stack_builder::Backend::Convex,
                package_manager: stack_builder::Pm::Pnpm,
                ..Stack::default()
            },
            location: Some("/Users/me/Developer".into()),
        };
        save(&paths, &kept).unwrap();
        assert_eq!(load(&paths), Some(kept));
        std::fs::write(remembered_file(&paths), "not json").unwrap();
        assert_eq!(load(&paths), None);
    }

    /// The line runs the command in the location, quoted for the shell.
    #[test]
    fn the_shell_line_runs_in_the_location() {
        let line = shell_line(Path::new("/Users/me/My Code"), &Stack::default());
        assert!(
            line.starts_with("cd '/Users/me/My Code' && bun create better-t-stack@3.49.0 "),
            "{line}"
        );
    }
}
