//! Finding a project's scripts, and keeping the saved commands.

use super::*;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn package_scripts_use_the_lockfiles_tool_in_file_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{"name":"web","scripts":{"dev":"vite","build":"vite build","lint:fix":"eslint --fix ."}}"#,
    );
    write(&root.join("pnpm-lock.yaml"), "");
    let groups = scan(root);
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(
        (g.label.as_str(), g.tool.as_str(), g.rel.as_str()),
        ("web", "pnpm", "")
    );
    let commands: Vec<_> = g.scripts.iter().map(|s| s.command.as_str()).collect();
    assert_eq!(
        commands,
        ["pnpm run dev", "pnpm run build", "pnpm run lint:fix"]
    );
    assert_eq!(g.scripts[0].body, "vite");
}

#[test]
fn package_manager_field_wins_and_npm_is_the_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("yarn.lock"), "");
    let m: Value = serde_json::from_str(r#"{"packageManager":"bun@1.2.0"}"#).unwrap();
    assert_eq!(package_manager(root, root, &m), "bun");
    assert_eq!(package_manager(root, root, &Value::Null), "yarn");
    let other = tempfile::tempdir().unwrap();
    assert_eq!(
        package_manager(other.path(), other.path(), &Value::Null),
        "npm"
    );
}

#[test]
fn workspaces_are_scanned_with_the_root_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{"name":"mono","workspaces":["apps/*","!apps/legacy"],"scripts":{"dev":"turbo dev"}}"#,
    );
    write(&root.join("bun.lock"), "");
    write(
        &root.join("apps/web/package.json"),
        r#"{"name":"@m/web","scripts":{"dev":"next dev"}}"#,
    );
    write(
        &root.join("apps/api/package.json"),
        r#"{"scripts":{"start":"node ."}}"#,
    );
    write(
        &root.join("apps/legacy/package.json"),
        r#"{"scripts":{"x":"y"}}"#,
    );
    write(&root.join("apps/notes/readme.md"), "");
    let groups = scan(root);
    let labels: Vec<_> = groups.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(labels, ["mono", "apps/api", "@m/web"]);
    assert_eq!(groups[2].scripts[0].command, "bun run dev");
    assert_eq!(groups[2].rel, "apps/web");
}

#[test]
fn pnpm_workspace_yaml_lists_packages() {
    let yaml =
        "packages:\n  - 'apps/*'\n  - \"tools/cli\" # cli\n  - '!**/test/**'\ncatalog:\n  - nope\n";
    assert_eq!(pnpm_packages(yaml), ["apps/*", "tools/cli", "!**/test/**"]);
}

#[test]
fn make_targets_skip_specials_patterns_and_variables() {
    let text = ".PHONY: dev\nCC := gcc\nX = 1\ndev: deps\n\tgo run .\nbuild test: \n%.o: %.c\n\t$(CC)\n# lint:\n";
    assert_eq!(make_targets(text), ["dev", "build", "test"]);
}

#[test]
fn deno_jsonc_tasks_and_cargo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("deno.jsonc"),
        "{\n // tasks\n \"tasks\": { \"start\": \"deno run -A main.ts\", \"x\": { \"command\": \"echo\" }, },\n}",
    );
    write(&root.join("Cargo.toml"), "[package]\n");
    let groups = scan(root);
    assert_eq!(groups[0].tool, "deno");
    assert_eq!(groups[0].scripts[0].command, "deno task start");
    assert_eq!(groups[0].scripts[1].body, "echo");
    assert_eq!(groups[1].scripts[0].command, "cargo run");
}

#[test]
fn odd_script_names_are_quoted() {
    assert_eq!(run_command("npm", "dev:web"), "npm run dev:web");
    assert_eq!(run_command("npm", "it's on"), r"npm run 'it'\''s on'");
}

#[test]
fn saved_command_lines_follow_the_mode() {
    let cmds = vec![
        " docker compose up -d ".into(),
        "".into(),
        "pnpm dev".into(),
    ];
    let mut c = SavedCommand::new(" Up ", cmds, RunMode::Sequence);
    assert_eq!(c.name, "Up");
    assert_eq!(c.lines(), ["docker compose up -d && pnpm dev"]);
    c.mode = RunMode::Parallel;
    assert_eq!(c.lines(), ["docker compose up -d", "pnpm dev"]);
}

#[test]
fn saved_commands_round_trip_per_project() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("run-commands.json");
    let a = Path::new("/p/a");
    let mut saved = SavedCommands::default();
    let cmd = SavedCommand::new("dev", vec!["bun dev".into()], RunMode::Parallel);
    let id = cmd.id.clone();
    saved.upsert(a, cmd);
    saved.save(&file).unwrap();
    let loaded = SavedCommands::load(&file).unwrap();
    assert_eq!(loaded, saved);
    assert_eq!(loaded.for_project(a)[0].mode, RunMode::Parallel);
    assert!(loaded.for_project(Path::new("/p/b")).is_empty());
    let mut loaded = loaded;
    assert!(loaded.remove(a, &id));
    assert!(loaded.projects.is_empty());
    assert_eq!(
        SavedCommands::load(&dir.path().join("missing.json")).unwrap(),
        SavedCommands::default()
    );
}

/// The app opens a project at its folder as it is on disk (through the links, in the letter
/// case there), and keys its saved commands so; sawit-mcp does too. Commands saved for a
/// project opened through a link, before projects were opened so, are under the link's name:
/// they join the folder's own as the file is read, the folder's first, and the Run panel and
/// list_scripts still find them (review-mcp M1).
#[test]
fn commands_saved_under_a_links_name_are_the_folders() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    std::fs::create_dir(&project).unwrap();
    let project = project.canonicalize().unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&project, &link).unwrap();
    let (own, linked, both) = (
        SavedCommand::new("dev", vec!["bun dev".into()], RunMode::Sequence),
        SavedCommand::new("up", vec!["docker compose up".into()], RunMode::Sequence),
        SavedCommand::new("test", vec!["bun test".into()], RunMode::Sequence),
    );
    let mut saved = SavedCommands::default();
    saved.upsert(&project, own.clone());
    saved.upsert(&project, both.clone());
    saved.upsert(&link, linked.clone());
    saved.upsert(&link, both.clone());
    let elsewhere = Path::new("/no/such/folder");
    saved.upsert(elsewhere, own.clone());
    let file = dir.path().join("run-commands.json");
    saved.save(&file).unwrap();

    let loaded = SavedCommands::load(&file).unwrap();
    assert_eq!(loaded.for_project(&project), [own.clone(), both, linked]);
    assert!(loaded.for_project(&link).is_empty());
    assert_eq!(loaded.for_project(elsewhere), [own]);
    assert_eq!(loaded.projects.len(), 2);
}
