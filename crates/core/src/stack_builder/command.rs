//! The project's name, as upstream formats and checks it, and the `create-better-t-stack`
//! command for a stack, as upstream's builder writes it and as Create Project runs it.

use super::{Category, DEFAULT_NAME, Pm, Stack, rules};

/// Upstream's `formatProjectName`: whitespace runs become dashes, and an empty name is
/// [`DEFAULT_NAME`].
pub fn format_project_name(name: &str) -> String {
    if name.is_empty() {
        return DEFAULT_NAME.into();
    }
    let mut out = String::new();
    let mut in_space = false;
    for c in name.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push('-');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// What is wrong with project name `name` (as [`format_project_name`] makes it), if anything:
/// upstream's `ProjectNameSchema` on its last path part, and the control characters the CLI
/// refuses.
pub fn validate_project_name(name: &str) -> Option<String> {
    let formatted = format_project_name(name);
    if formatted.chars().any(char::is_control) {
        return Some("Project name can't contain control characters".into());
    }
    let base = formatted
        .split(['/', '\\'])
        .rfind(|p| !p.is_empty())
        .unwrap_or("");
    if base.is_empty() {
        return Some("Project name cannot be empty".into());
    }
    if base.chars().count() > 255 {
        return Some("Project name must be less than 255 characters".into());
    }
    if base != "." && base.starts_with('.') {
        return Some("Project name cannot start with a dot (except for '.')".into());
    }
    if base != "." && base.starts_with('-') {
        return Some("Project name cannot start with a dash".into());
    }
    if base.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Some("Project name contains invalid characters".into());
    }
    if base.eq_ignore_ascii_case("node_modules") {
        return Some("Project name is reserved".into());
    }
    None
}

/// Upstream's `quoteShellArgument`: as it is when it is only letters, digits and `_./-`, else
/// in single quotes.
fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c));
    if plain {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// Whether `stack` is upstream's default in everything but its name: the command is then the
/// runner, the name and `--yes`.
fn is_default_but_name(stack: &Stack) -> bool {
    let sorted = |mut ids: Vec<&'static str>| {
        ids.sort_unstable();
        ids
    };
    let default = Stack::default();
    Category::ALL
        .iter()
        .all(|&c| sorted(stack.picked(c)) == sorted(default.picked(c)))
}

/// The frontends the CLI's `--frontend` takes: the web and the native one that aren't none, or
/// `none` alone (upstream's `getStackFrontends`).
pub(crate) fn frontends(stack: &Stack) -> Vec<&'static str> {
    let picked: Vec<&'static str> = [stack.web_frontend.id(), stack.native_frontend.id()]
        .into_iter()
        .filter(|id| *id != "none")
        .collect();
    if picked.is_empty() {
        vec!["none"]
    } else {
        picked
    }
}

/// The command that creates `stack`'s project, exactly as upstream's builder writes it
/// (`generateStackCommand`), with the CLI pinned to [`CLI_VERSION`](super::CLI_VERSION) instead
/// of `@latest`. The stack's package manager picks the runner: `bun create`, `pnpm create` or
/// `npx`.
pub fn command(stack: &Stack) -> String {
    let base = stack.package_manager.runner();
    let name = quote(&format_project_name(&stack.project_name));
    if is_default_but_name(stack) {
        return format!("{base} {name} --yes");
    }
    let list = |ids: Vec<&'static str>| match ids.join(" ") {
        joined if joined.is_empty() => "none".to_string(),
        joined => joined,
    };
    let flags = [
        format!("--frontend {}", frontends(stack).join(" ")),
        format!("--backend {}", rules::core_backend(stack.backend)),
        format!("--runtime {}", stack.runtime.id()),
        format!("--api {}", stack.api.id()),
        format!("--auth {}", stack.auth.id()),
        format!("--payments {}", stack.payments.id()),
        format!("--database {}", stack.database.id()),
        format!("--orm {}", stack.orm.id()),
        format!("--db-setup {}", stack.db_setup.id()),
        format!("--package-manager {}", stack.package_manager.id()),
        if stack.git { "--git" } else { "--no-git" }.to_string(),
        format!("--web-deploy {}", stack.web_deploy.id()),
        format!("--server-deploy {}", stack.server_deploy.id()),
        if stack.install {
            "--install"
        } else {
            "--no-install"
        }
        .to_string(),
        format!(
            "--addons {}",
            list(stack.addons.iter().map(|a| a.id()).collect())
        ),
        format!(
            "--examples {}",
            list(stack.examples.iter().map(|e| e.id()).collect())
        ),
    ];
    format!("{base} {name} {}", flags.join(" "))
}

/// The flags [`run_command`] adds to [`command`] so the CLI never stops to ask: the folder
/// conflict (Sawit checks there is none first) and the "Open project with?" picker, and, for
/// the database setups that ask how to provision (and that Alchemy doesn't manage), manual
/// setup: the CLI then writes the `.env` and prints the steps.
pub fn quiet_flags(stack: &Stack) -> Vec<&'static str> {
    let mut flags = vec!["--directory-conflict", "error", "--open", "none"];
    if rules::asks_how_to_provision(stack) {
        flags.push("--manual-db");
    }
    flags
}

/// What Create Project runs for `stack`: [`command`], with npx told not to ask before it
/// downloads the CLI (`npx -y`), then [`quiet_flags`].
pub fn run_command(stack: &Stack) -> String {
    let command = command(stack);
    let command = match stack.package_manager {
        Pm::Npm => command.replacen("npx ", "npx -y ", 1),
        _ => command,
    };
    format!("{command} {}", quiet_flags(stack).join(" "))
}
