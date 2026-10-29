//! Code formatting for the editor: Prettier for everything it supports, the
//! usual formatter for the rest. Tools come from the project first (its own
//! `node_modules/.bin`, so its version and config apply), then PATH, then
//! `npx`/`bunx` for the npm-based ones.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt as _;

use crate::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Prettier,
    /// `sql-formatter`, PostgreSQL dialect.
    SqlFormatter,
    Taplo,
    Rustfmt,
    Gofmt,
    Ruff,
    Shfmt,
    ClangFormat,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Prettier => "Prettier",
            Tool::SqlFormatter => "sql-formatter",
            Tool::Taplo => "Taplo",
            Tool::Rustfmt => "rustfmt",
            Tool::Gofmt => "gofmt",
            Tool::Ruff => "Ruff",
            Tool::Shfmt => "shfmt",
            Tool::ClangFormat => "clang-format",
        }
    }

    /// Executable name, and for npm tools the package `npx` should fetch.
    fn binary(self) -> (&'static str, Option<&'static str>) {
        match self {
            Tool::Prettier => ("prettier", Some("prettier@3")),
            Tool::SqlFormatter => ("sql-formatter", Some("sql-formatter@15")),
            Tool::Taplo => ("taplo", Some("@taplo/cli@0.7")),
            Tool::Rustfmt => ("rustfmt", None),
            Tool::Gofmt => ("gofmt", None),
            Tool::Ruff => ("ruff", None),
            Tool::Shfmt => ("shfmt", None),
            Tool::ClangFormat => ("clang-format", None),
        }
    }

    /// Arguments that make the tool read stdin and write stdout.
    fn args(self, file: &Path) -> Vec<String> {
        let file = file.display().to_string();
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        match self {
            Tool::Prettier => v(&["--stdin-filepath", &file]),
            Tool::SqlFormatter => v(&["--language", "postgresql"]),
            Tool::Taplo => v(&["fmt", "-"]),
            Tool::Rustfmt => v(&["--emit", "stdout", "--edition", "2021"]),
            Tool::Gofmt => Vec::new(),
            Tool::Ruff => v(&["format", "--stdin-filename", &file, "-"]),
            Tool::Shfmt => v(&["-filename", &file]),
            Tool::ClangFormat => v(&["--assume-filename", &file]),
        }
    }
}

/// The formatter for a file, by extension.
pub fn tool_for(path: &Path) -> Option<Tool> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "json" | "jsonc"
        | "json5" | "css" | "scss" | "less" | "html" | "htm" | "vue" | "md" | "mdx"
        | "markdown" | "yaml" | "yml" | "graphql" | "gql" | "hbs" | "handlebars" => Tool::Prettier,
        "sql" | "psql" => Tool::SqlFormatter,
        "toml" => Tool::Taplo,
        "rs" => Tool::Rustfmt,
        "go" => Tool::Gofmt,
        "py" | "pyi" => Tool::Ruff,
        "sh" | "bash" | "zsh" => Tool::Shfmt,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "java" | "m" | "mm" => Tool::ClangFormat,
        _ => return None,
    })
}

/// A ready-to-run formatter process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub tool: Tool,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The file's folder, so project config (.prettierrc, rustfmt.toml…) applies.
    pub cwd: PathBuf,
}

fn find_in(path_var: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Finds the tool for `file`. `path_var` overrides `$PATH` (tests).
pub fn resolve(
    tool: Tool,
    file: &Path,
    path_var: Option<&OsStr>,
) -> std::result::Result<Invocation, String> {
    let env_path: OsString = match path_var {
        Some(p) => p.to_owned(),
        None => std::env::var_os("PATH").unwrap_or_default(),
    };
    let cwd = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let (bin, npm_package) = tool.binary();
    let args = tool.args(file);
    let invocation = |program: PathBuf, args: Vec<String>| Invocation {
        tool,
        program,
        args,
        cwd: cwd.clone(),
    };
    if npm_package.is_some()
        && let Some(local) = cwd
            .ancestors()
            .map(|d| d.join("node_modules/.bin").join(bin))
            .find(|p| p.is_file())
    {
        return Ok(invocation(local, args));
    }
    if let Some(found) = find_in(&env_path, bin) {
        return Ok(invocation(found, args));
    }
    if let Some(package) = npm_package {
        for runner in ["npx", "bunx"] {
            if let Some(found) = find_in(&env_path, runner) {
                let mut all = if runner == "npx" {
                    vec!["--yes".to_string()]
                } else {
                    Vec::new()
                };
                all.push(package.to_string());
                all.extend(args.iter().cloned());
                return Ok(invocation(found, all));
            }
        }
        return Err(format!(
            "{} needs Node.js (npx) or Bun. Install one, or add it to the project: npm i -D {}",
            tool.name(),
            package
                .split('@')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(package)
        ));
    }
    let hint = match tool {
        Tool::Rustfmt => "rustup component add rustfmt",
        Tool::Gofmt => "brew install go",
        Tool::Ruff => "brew install ruff",
        Tool::Shfmt => "brew install shfmt",
        _ => "brew install clang-format",
    };
    Err(format!("{bin} not found. Install it with: {hint}"))
}

/// npx may download the tool on first use.
const TIMEOUT: Duration = Duration::from_secs(90);

/// Formats `text` as the contents of `path` (which need not be saved).
pub async fn format_text(path: &Path, text: &str) -> Result<String> {
    let tool = tool_for(path)
        .ok_or_else(|| CoreError::Format("No formatter for this file type yet.".into()))?;
    let inv = resolve(tool, path, None).map_err(CoreError::Format)?;
    let mut child = tokio::process::Command::new(&inv.program)
        .args(&inv.args)
        .current_dir(&inv.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped");
    let input = text.to_owned();
    let writer = tokio::spawn(async move {
        let _ = stdin.write_all(input.as_bytes()).await;
    });
    let out = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            CoreError::Format(format!("{} did not finish within 90 seconds.", tool.name()))
        })??;
    let _ = writer.await;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        return Err(CoreError::Format(format!(
            "{} failed: {}",
            tool.name(),
            if msg.is_empty() { "no message" } else { msg }
        )));
    }
    String::from_utf8(out.stdout)
        .map_err(|_| CoreError::Format(format!("{} produced non-UTF-8 output.", tool.name())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn prettier_covers_web_and_config_files() {
        for f in [
            "a.ts",
            "a.tsx",
            "a.js",
            "a.jsx",
            "a.mjs",
            "a.json",
            "a.jsonc",
            "a.css",
            "a.scss",
            "a.less",
            "a.html",
            "a.vue",
            "a.md",
            "a.mdx",
            "a.yaml",
            "a.yml",
            "a.graphql",
            "a.hbs",
        ] {
            assert_eq!(tool_for(Path::new(f)), Some(Tool::Prettier), "{f}");
        }
    }

    #[test]
    fn other_languages_use_their_own_formatter() {
        assert_eq!(tool_for(Path::new("q.sql")), Some(Tool::SqlFormatter));
        assert_eq!(tool_for(Path::new("Cargo.toml")), Some(Tool::Taplo));
        assert_eq!(tool_for(Path::new("main.rs")), Some(Tool::Rustfmt));
        assert_eq!(tool_for(Path::new("main.go")), Some(Tool::Gofmt));
        assert_eq!(tool_for(Path::new("app.py")), Some(Tool::Ruff));
        assert_eq!(tool_for(Path::new("run.sh")), Some(Tool::Shfmt));
        assert_eq!(tool_for(Path::new("x.c")), Some(Tool::ClangFormat));
        assert_eq!(tool_for(Path::new("notes.txt")), None);
        assert_eq!(tool_for(Path::new("photo.png")), None);
    }

    fn touch_exe(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn project_prettier_wins_over_path_and_npx() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("app");
        let file = project.join("src/deep/a.ts");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let global = d.path().join("bin");
        touch_exe(&global.join("prettier"));
        touch_exe(&global.join("npx"));
        let path_var = global.as_os_str().to_owned();

        // PATH beats npx.
        let cmd = resolve(Tool::Prettier, &file, Some(&path_var)).unwrap();
        assert_eq!(cmd.program, global.join("prettier"));

        // The project's own copy beats PATH.
        let local = project.join("node_modules/.bin/prettier");
        touch_exe(&local);
        let cmd = resolve(Tool::Prettier, &file, Some(&path_var)).unwrap();
        assert_eq!(cmd.program, local);
        assert_eq!(cmd.cwd, file.parent().unwrap());
        assert!(
            cmd.args.iter().any(|a| a.ends_with("a.ts")),
            "{:?}",
            cmd.args
        );
    }

    #[test]
    fn npx_is_the_fallback_for_npm_tools() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        touch_exe(&bin.join("npx"));
        let file = d.path().join("q.sql");
        let cmd = resolve(Tool::SqlFormatter, &file, Some(bin.as_os_str())).unwrap();
        assert_eq!(cmd.program, bin.join("npx"));
        assert_eq!(
            cmd.args[..2],
            ["--yes".to_string(), "sql-formatter@15".to_string()]
        );
        assert!(cmd.args.contains(&"postgresql".to_string()));
    }

    #[test]
    fn missing_tools_explain_how_to_install() {
        let d = tempfile::tempdir().unwrap();
        let empty = d.path().join("nothing");
        let err = resolve(Tool::Gofmt, Path::new("x.go"), Some(empty.as_os_str())).unwrap_err();
        assert!(err.contains("gofmt"), "{err}");
        let err = resolve(Tool::Prettier, Path::new("x.ts"), Some(empty.as_os_str())).unwrap_err();
        assert!(err.contains("Node"), "{err}");
    }

    async fn run_if_available(path: &str, input: &str) -> Option<String> {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join(path);
        let tool = tool_for(&file)?;
        if resolve(tool, &file, None).is_err() {
            eprintln!("skip: no formatter for {path}");
            return None;
        }
        Some(format_text(&file, input).await.unwrap())
    }

    #[tokio::test]
    #[ignore = "may fetch the tool with npx; run with --include-ignored"]
    async fn prettier_formats_typescript() {
        if let Some(out) = run_if_available("a.ts", "const   a = {b:1}\n").await {
            assert_eq!(out, "const a = { b: 1 };\n");
        }
    }

    #[tokio::test]
    #[ignore = "may fetch the tool with npx; run with --include-ignored"]
    async fn sql_is_formatted_for_postgres() {
        if let Some(out) = run_if_available("q.sql", "select id,name from users where id=1").await {
            assert!(out.contains("SELECT") || out.contains("select"), "{out}");
            assert!(out.lines().count() > 1, "{out}");
        }
    }

    #[tokio::test]
    async fn rustfmt_formats_rust() {
        if let Some(out) = run_if_available("m.rs", "fn main(){let x=1;}").await {
            assert_eq!(out, "fn main() {\n    let x = 1;\n}\n");
        }
    }

    #[tokio::test]
    #[ignore = "may fetch the tool with npx; run with --include-ignored"]
    async fn syntax_errors_come_back_as_errors() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("bad.ts");
        if resolve(Tool::Prettier, &file, None).is_err() {
            return;
        }
        let err = format_text(&file, "const = ;").await.unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
