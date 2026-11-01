//! Paths as agents name them: `~` for the home folder, a folder that must be there, the file a
//! tool is asked to write (never a canvas file), and the project an agent works in.

use std::path::{Path, PathBuf};

/// `~/x` for paths in the home folder.
pub(crate) fn tilde(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display())
        }
        _ => path.display().to_string(),
    }
}

/// An existing folder given by an agent (absolute, `~` expanded).
pub(crate) fn existing_dir(dir: &str) -> Result<PathBuf, String> {
    let path = expand_home(dir.trim());
    if !path.is_absolute() {
        return Err(format!("“{dir}” must be an absolute path."));
    }
    if !path.is_dir() {
        return Err(format!("{} is not a folder.", path.display()));
    }
    Ok(path)
}

/// The file `output_path` names for `tool` to write, if it names one, read as every tool that
/// writes a file reads it: trimmed (a blank one names none, and the tool picks its own file), `~`
/// for the home folder, absolute, and never a canvas file ([`refuse_canvas_file`]). It is checked
/// before anything is made, and it is the path written.
pub(crate) fn output_path(
    tool: &str,
    output_path: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let Some(p) = output_path.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    let path = expand_home(p);
    if !path.is_absolute() {
        return Err(format!("output_path “{p}” must be absolute."));
    }
    refuse_canvas_file(tool, &path)?;
    Ok(Some(path))
}

/// Refuses `path`, a file an agent asked `tool` to write, when it is a canvas file: a `.sawit` or
/// `.excalidraw` file, or a copy Sawit keeps of one ([`sawit_board::ops::is_canvas_file`]).
/// While the app is closed nothing else holds the board, so one mistaken path would replace it
/// with a screenshot, a table or a dump (ruling P7-X3, which the canvas's own export commands
/// keep too). Only the name counts: `canvas.sawit.png` is an image, and is written.
pub(crate) fn refuse_canvas_file(tool: &str, path: &Path) -> Result<(), String> {
    if sawit_board::ops::is_canvas_file(path) {
        return Err(format!(
            "{tool} would overwrite a canvas file: {}",
            path.display()
        ));
    }
    Ok(())
}

/// `path` as an agent means it: trimmed, with `~` for the home folder, and a relative path taken
/// from the folder the agent runs in (sawit-mcp's), not from the app's, whose folder is another.
pub(crate) fn agent_path(path: &str) -> PathBuf {
    let path = expand_home(path.trim());
    match std::env::current_dir() {
        Ok(cwd) if path.is_relative() => cwd.join(path),
        _ => path,
    }
}

pub(crate) fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ if path == "~" => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default(),
        _ => PathBuf::from(path),
    }
}

/// The project a tool's files belong to: the `project` argument, else the
/// folder the agent started `sawit-mcp` in (its project), unless that is
/// `/` or the home folder. The canvas, and the diagrams and exports written
/// with no path, go by the project that holds it ([`agent_project`]).
pub(crate) fn project_dir(project: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = project.map(str::trim).filter(|p| !p.is_empty()) {
        return Some(expand_home(p));
    }
    let cwd = std::env::current_dir().ok()?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    (cwd.parent().is_some() && Some(&cwd) != home.as_ref()).then_some(cwd)
}

/// The project whose canvas the canvas tools use: the `project` argument, else the project that
/// holds the folder the agent started `sawit-mcp` in ([`project_dir`], [`project_holding`]). The
/// app takes an agent's folder to mean the project of its window that holds it, so an agent
/// started in a subfolder (`web/` of a repository) reaches the canvas the app shows for that
/// project with the app closed too, rather than a canvas of the subfolder's own that the app never
/// shows (Q12). The diagrams and canvas exports an agent writes with no path go to its `.nyawit/`
/// too, so that sawit-mcp makes no `.nyawit/` of a subfolder's own.
pub(crate) fn agent_project(project: Option<&str>) -> Option<PathBuf> {
    let named = project.is_some_and(|p| !p.trim().is_empty());
    let folder = project_dir(project)?;
    if named {
        return Some(folder);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Some(project_holding(&folder, home.as_deref()))
}

/// The project that holds `folder`, from what is on disk, as the app would have it open: within
/// the git repository `folder` is in (the nearest folder from it up with `.git`), the nearest
/// folder whose `.nyawit/` holds a canvas (`canvas.sawit`, or the old `canvas.excalidraw`), which
/// the app made for a project it opened (a package of the repository opened as a project of its
/// own, say); else the repository. Outside a repository, `folder` itself.
///
/// A `.nyawit/` with no canvas in it decides nothing: sawit-mcp made one for the diagrams an agent
/// rendered in `shop/web`, which then took the agent's canvas too, one the app never shows with
/// `shop` open. Nor does a canvas above the repository: an agent once run in `~/code` left one
/// there, which every repository under it with no canvas of its own then shared. The home folder
/// and `/` are never one, nor is a folder above them, as [`project_dir`] has it.
fn project_holding(folder: &Path, home: Option<&Path>) -> PathBuf {
    let below_home = |dir: &&Path| dir.parent().is_some() && Some(*dir) != home;
    let up: Vec<&Path> = folder.ancestors().take_while(below_home).collect();
    let Some(repository) = up.iter().position(|dir| dir.join(".git").exists()) else {
        return folder.to_path_buf();
    };
    let holds_a_canvas = |dir: &Path| {
        sawit_core::nyawit::board_file(dir).exists()
            || sawit_core::nyawit::legacy_canvas_file(dir).exists()
    };
    let mut within = up[..=repository].iter().copied();
    within
        .find(|dir| holds_a_canvas(dir))
        .unwrap_or(up[repository])
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tool_may_write_a_canvas_file() {
        for name in [
            "/p/.nyawit/canvas.sawit",
            "/p/Board.Sawit",
            "/p/.nyawit/canvas.excalidraw",
            "/p/.nyawit/canvas.sawit.bak",
            "/p/.nyawit/canvas.sawit.bak.3",
            "/p/.nyawit/canvas.sawit.del.bak",
            "/p/design/home.sawit.del.bak.4",
            "/p/.nyawit/canvas.sawit.tmp",
        ] {
            assert_eq!(
                refuse_canvas_file("render_mermaid", Path::new(name)),
                Err(format!(
                    "render_mermaid would overwrite a canvas file: {name}"
                ))
            );
        }
        for name in ["/p/page.png", "/p/canvas.sawit.svg", "/p/.nyawit/rows.csv"] {
            assert_eq!(refuse_canvas_file("export_query", Path::new(name)), Ok(()));
        }
    }

    /// Q12: with the app closed, a canvas tool that names no project draws on the canvas of the
    /// project that holds the agent's folder, as the app does: within the agent's repository, the
    /// nearest folder up whose `.nyawit/` holds a canvas, else the repository; outside one, the
    /// folder itself. An agent started in `shop/web` edited `shop/web/.nyawit/canvas.sawit`,
    /// which the app never shows. A `.nyawit/` with no canvas in it (one sawit-mcp made for an
    /// agent's diagrams), or a canvas above the repository (left by an agent run in `~/code`),
    /// decides nothing: either took the canvas of every agent below it. The home folder is never
    /// the project, whatever it holds.
    #[test]
    fn the_agents_project_is_the_nearest_folder_up_that_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let code = home.join("code");
        let shop = code.join("shop");
        let web = shop.join("web");
        let src = web.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let project = |folder: &Path| project_holding(folder, Some(&home));
        let canvas_in = |folder: &Path, name: &str| {
            let nyawit = sawit_core::nyawit::dir(folder);
            std::fs::create_dir_all(&nyawit).unwrap();
            std::fs::write(nyawit.join(name), "").unwrap();
        };
        assert_eq!(project(&src), src, "nothing marks a project");

        std::fs::create_dir(shop.join(".git")).unwrap();
        assert_eq!(project(&src), shop, "the repository");
        assert_eq!(project(&shop), shop);
        // The `.nyawit/` sawit-mcp made in the agent's subfolder for its diagrams.
        std::fs::create_dir_all(sawit_core::nyawit::diagrams_dir(&web)).unwrap();
        assert_eq!(project(&src), shop, "a .nyawit/ without a canvas took it");
        // A canvas above the repository, which an agent once run in `~/code` left there: the
        // repositories under it keep their own.
        canvas_in(&code, "canvas.sawit");
        assert_eq!(project(&src), shop, "a canvas above the repository took it");
        let blog = code.join("blog");
        std::fs::create_dir_all(blog.join(".git")).unwrap();
        assert_eq!(project(&blog), blog);
        // A folder of the repository the app has made a canvas for, a package opened as a project
        // of its own: its old canvas counts too.
        canvas_in(&web, "canvas.excalidraw");
        assert_eq!(project(&src), web);
        canvas_in(&shop, "canvas.sawit");
        assert_eq!(project(&src), web, "the nearest");
        // Regression: a file named with the canvas engine's own extension is no canvas, and marks
        // no project.
        let admin = shop.join("admin");
        std::fs::create_dir_all(admin.join("src")).unwrap();
        canvas_in(&admin, "canvas.figcraft");
        assert_eq!(project(&admin.join("src")), shop);
        // A package with a repository of its own (a submodule's `.git` is a file) is its own.
        let api = shop.join("api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(api.join(".git"), "gitdir: ../.git/modules/api").unwrap();
        assert_eq!(project(&api.join("src")), api);
        // A file at `.nyawit`'s name holds no canvas.
        std::fs::remove_dir_all(sawit_core::nyawit::dir(&web)).unwrap();
        std::fs::write(sawit_core::nyawit::dir(&web), "").unwrap();
        assert_eq!(project(&src), shop);

        // The home folder isn't one, nor is anything above it.
        std::fs::create_dir(home.join(".nyawit")).unwrap();
        std::fs::create_dir(home.join(".git")).unwrap();
        let notes = home.join("notes");
        std::fs::create_dir(&notes).unwrap();
        assert_eq!(project(&notes), notes);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert_eq!(project(&notes), notes);
    }
}
