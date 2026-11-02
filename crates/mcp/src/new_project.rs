//! The New Project page: Better-T-Stack's stack builder (its options, presets, compatibility
//! rules and command, ported in `sawit_core::stack_builder`), and creating a project with it,
//! in a Sawit terminal when the app runs, else here.

use std::sync::{Arc, Mutex};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::{self, Command};
use sawit_core::new_project;
use sawit_core::stack_builder::{self, Category, Choices, PRESETS, Resolved, Stack};
use serde_json::{Map, Value, json};

use crate::app::pretty;
use crate::{Sawit, expand_home, fail, ok, tri};

/// Lines of the CLI's output a headless create returns at most.
const OUTPUT_LINES: usize = 200;

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct StackArgs {
    #[schemars(
        description = "Start from a preset instead of the default stack: mern, pern, t3 or uniwind (stack_options lists them)"
    )]
    pub template: Option<String>,
    #[schemars(
        description = "Project name (folder name), default my-better-t-app; spaces become dashes"
    )]
    pub name: Option<String>,
    #[schemars(
        description = "Frontends as create-better-t-stack's --frontend takes them: one web (tanstack-router, react-router, tanstack-start, next, nuxt, svelte, solid, astro) and one native (native-bare, native-uniwind, native-unistyles) at most, or [\"none\"]"
    )]
    pub frontend: Option<Vec<String>>,
    #[schemars(
        description = "hono, elysia, express, fastify, convex, self (the web frontend's own routes; or self-next, self-nuxt, …) or none"
    )]
    pub backend: Option<String>,
    #[schemars(description = "bun, node, workers or none")]
    pub runtime: Option<String>,
    #[schemars(description = "trpc, orpc or none")]
    pub api: Option<String>,
    #[schemars(description = "sqlite, postgres, mysql, mongodb or none")]
    pub database: Option<String>,
    #[schemars(description = "drizzle, prisma, mongoose or none")]
    pub orm: Option<String>,
    #[schemars(
        description = "turso, d1, neon, prisma-postgres, mongodb-atlas, supabase, planetscale, docker or none"
    )]
    pub db_setup: Option<String>,
    #[schemars(description = "cloudflare, prisma, docker, vercel or none")]
    pub web_deploy: Option<String>,
    #[schemars(description = "cloudflare, prisma, docker, vercel or none")]
    pub server_deploy: Option<String>,
    #[schemars(description = "better-auth, clerk or none")]
    pub auth: Option<String>,
    #[schemars(description = "polar or none")]
    pub payments: Option<String>,
    #[schemars(
        description = "Addons (any number; [\"none\"] for none): pwa, tauri, electrobun, starlight, fumadocs, lefthook, husky, biome, oxlint, turborepo, nx, vite-plus, ultracite, opentui, wxt, skills, mcp, evlog, axiom"
    )]
    pub addons: Option<Vec<String>>,
    #[schemars(description = "Examples (any number; [\"none\"] for none): todo, ai")]
    pub examples: Option<Vec<String>>,
    #[schemars(description = "Initialize a git repository (default true)")]
    pub git: Option<bool>,
    #[schemars(description = "Install dependencies (default true)")]
    pub install: Option<bool>,
    #[schemars(
        description = "npm, pnpm or bun (default bun): the project's package manager, which also runs the CLI"
    )]
    pub package_manager: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StackCreateArgs {
    #[schemars(
        description = "Absolute folder (or ~/…) to create the project in; the project is <location>/<name>, which must not exist yet"
    )]
    pub location: String,
    #[serde(flatten)]
    pub stack: StackArgs,
    #[schemars(
        description = "With the app running: true (the default) opens the project in Sawit once it is made, as Create Project does; false makes it without opening it, as Create Project Only does (the page then offers Open Project). Without the app, nothing is opened either way"
    )]
    pub open: Option<bool>,
    #[schemars(
        description = "Must be true: creating downloads create-better-t-stack and the project's packages from the internet, and runs their install scripts"
    )]
    pub confirm: bool,
}

impl StackArgs {
    /// The choices as `sawit_core::stack_builder` takes them.
    fn choices(&self) -> Choices {
        Choices {
            template: self.template.clone(),
            name: self.name.clone(),
            frontend: self.frontend.clone(),
            backend: self.backend.clone(),
            runtime: self.runtime.clone(),
            api: self.api.clone(),
            database: self.database.clone(),
            orm: self.orm.clone(),
            db_setup: self.db_setup.clone(),
            web_deploy: self.web_deploy.clone(),
            server_deploy: self.server_deploy.clone(),
            auth: self.auth.clone(),
            payments: self.payments.clone(),
            addons: self.addons.clone(),
            examples: self.examples.clone(),
            git: self.git,
            install: self.install,
            package_manager: self.package_manager.clone(),
        }
    }
}

/// Every category with its options, the presets and the default stack, as `stack_options`
/// answers.
pub fn options_json() -> Value {
    let categories: Vec<Value> = Category::ALL
        .iter()
        .map(|&c| {
            json!({
                "id": c.id(),
                "label": c.label(),
                "multiple": c.multiple(),
                "options": stack_builder::options(c),
            })
        })
        .collect();
    let presets: Vec<Value> = PRESETS
        .iter()
        .map(|p| {
            let stack = stack_builder::resolve(&p.stack()).stack;
            json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "stack": stack,
                "command": stack_builder::command(&stack),
            })
        })
        .collect();
    json!({
        "cli": format!("create-better-t-stack {}", stack_builder::CLI_VERSION),
        "upstream_commit": stack_builder::UPSTREAM_COMMIT,
        "categories": categories,
        "presets": presets,
        "default_stack": Stack::default(),
        "default_command": stack_builder::command(&Stack::default()),
    })
}

/// What `stack_check` answers for `resolved`: the stack, what the rules changed, the notes,
/// every disabled option with its reason, the name's problem, and the commands.
pub fn check_json(resolved: &Resolved) -> Value {
    let mut disabled = Map::new();
    for (category, id, reason) in stack_builder::disabled(&resolved.stack) {
        let entry = disabled
            .entry(category.id().to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(map) = entry.as_object_mut() {
            map.insert(id.to_string(), Value::String(reason));
        }
    }
    json!({
        "stack": resolved.stack,
        "changes": resolved.changes,
        "notes": resolved.notes,
        "name_error": stack_builder::validate_project_name(&resolved.stack.project_name),
        "disabled": disabled,
        "command": stack_builder::command(&resolved.stack),
        "run_command": stack_builder::run_command(&resolved.stack),
    })
}

impl Sawit {
    pub async fn stack_check_tool(&self, a: StackArgs) -> CallToolResult {
        let stack = tri!(a.choices().stack());
        ok(pretty(&check_json(&stack_builder::resolve(&stack))))
    }

    pub async fn stack_create_tool(&self, a: StackCreateArgs) -> CallToolResult {
        if !a.confirm {
            return fail(
                "Nothing was created: creating downloads create-better-t-stack and the project's packages and runs their install scripts. Call again with confirm=true.",
            );
        }
        let location = expand_home(a.location.trim());
        let stack = tri!(a.stack.choices().stack());
        let resolved = stack_builder::resolve(&stack);
        // Checked here too, so an agent hears it before the app is involved.
        let dir = tri!(new_project::target(&location, &resolved.stack));
        if app_control::is_running(&self.paths) {
            let command = Command::NewProject {
                location,
                stack: resolved.stack.clone(),
                open: a.open.unwrap_or(true),
            };
            let mut reply = tri!(self.app_call(None, command).await);
            if !resolved.changes.is_empty() {
                reply["changes"] = json!(resolved.changes);
            }
            return ok(pretty(&reply));
        }
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        let made = new_project::create(&location, &resolved.stack, move |line| {
            let mut lines = sink.lock().unwrap_or_else(|e| e.into_inner());
            lines.push(line);
            if lines.len() > OUTPUT_LINES {
                lines.remove(0);
            }
        })
        .await;
        let output = lines.lock().unwrap_or_else(|e| e.into_inner()).join("\n");
        let command = new_project::shell_line(&location, &resolved.stack);
        match made {
            Ok(dir) => ok(format!(
                "Created {} (the Sawit app isn't running, so it ran here).\n$ {command}\n{output}\n\nOpen it with app_open or `sawit {}`.",
                dir.display(),
                dir.display()
            )),
            Err(why) => fail(format!(
                "{why}\n$ {command}\n{output}\n\n{}",
                if new_project::created(&dir) {
                    format!("The project is at {} all the same.", dir.display())
                } else {
                    "Nothing was created.".into()
                }
            )),
        }
    }
}

#[tool_router(router = new_project_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List the New Project page's stack builder (Better-T-Stack's, create-better-t-stack 3.49.0): every category in order with its options (id, name, description, whether it is the default or experimental, whether the category takes several), the presets (MERN, PERN, T3, Uniwind) with their stacks and commands, and the default stack. Works without the app"
    )]
    async fn stack_options(&self) -> CallToolResult {
        ok(pretty(&options_json()))
    }

    #[tool(
        description = "Check a stack as the New Project page does: give any choices by create-better-t-stack's flag names (the rest are the default stack's, or a preset's with template). Returns the stack the compatibility rules resolve it to, what they changed and why, notes (desktop builds), every disabled option with its reason, the project name's problem if any, the command the page shows and the command Create Project runs. Works without the app"
    )]
    async fn stack_check(&self, Parameters(a): Parameters<StackArgs>) -> CallToolResult {
        self.stack_check_tool(a).await
    }

    #[tool(
        description = "Create a project from a stack (choices as in stack_check) in <location>/<name>, which must not exist. With the Sawit app running, the New Project page shows the stack and runs create-better-t-stack in a terminal tab of its own, and Sawit opens the folder as a project when it is made (open=false makes it without opening it, as Create Project Only does): returns the folder, the command and the terminal and pane (terminal_read shows progress). Without the app, runs it here and returns the CLI's output (it can take minutes: it installs dependencies). Downloads and runs install scripts: needs confirm=true"
    )]
    async fn stack_create(&self, Parameters(a): Parameters<StackCreateArgs>) -> CallToolResult {
        self.stack_create_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(result: &CallToolResult) -> String {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    }

    fn sawit() -> Sawit {
        Sawit::new(sawit_core::instances::AppPaths {
            root: std::env::temp_dir()
                .join(format!("sawit-mcp-new-project-{}", std::process::id())),
        })
    }

    /// Every category and option, the four presets with their commands, and the default stack.
    #[test]
    fn the_options_list_everything_the_page_shows() {
        let options = options_json();
        let categories = options["categories"].as_array().unwrap();
        assert_eq!(categories.len(), 17);
        assert_eq!(categories[0]["id"], "webFrontend");
        assert_eq!(categories[0]["options"][0]["id"], "tanstack-router");
        assert_eq!(categories[0]["options"][0]["default"], true);
        let presets = options["presets"].as_array().unwrap();
        let ids: Vec<&str> = presets.iter().map(|p| p["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["mern", "pern", "t3", "uniwind"]);
        assert!(
            presets[2]["command"]
                .as_str()
                .unwrap()
                .contains("--frontend next --backend self")
        );
        assert_eq!(
            options["default_command"],
            "bun create better-t-stack@3.49.0 my-better-t-app --yes"
        );
    }

    /// A stack checked: what the rules change, what's disabled and why, and the commands.
    #[tokio::test]
    async fn a_checked_stack_says_what_changed_and_what_is_disabled() {
        let r = sawit()
            .stack_check_tool(StackArgs {
                backend: Some("convex".into()),
                name: Some("shop".into()),
                ..Default::default()
            })
            .await;
        assert_ne!(r.is_error, Some(true), "{}", said(&r));
        let v: Value = serde_json::from_str(&said(&r)).unwrap();
        assert_eq!(v["stack"]["backend"], "convex");
        assert_eq!(v["stack"]["orm"], "none");
        assert!(
            v["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["message"] == "Orm set to 'none' (managed by convex)"),
            "{v}"
        );
        assert_eq!(
            v["disabled"]["orm"]["prisma"],
            "Convex provides its own orm"
        );
        assert!(v["command"].as_str().unwrap().starts_with(
            "bun create better-t-stack@3.49.0 shop --frontend tanstack-router --backend convex"
        ));
        assert!(
            v["run_command"]
                .as_str()
                .unwrap()
                .ends_with("--directory-conflict error --open none")
        );
        let bad = sawit()
            .stack_check_tool(StackArgs {
                orm: Some("sequelize".into()),
                ..Default::default()
            })
            .await;
        assert_eq!(bad.is_error, Some(true));
        assert!(
            said(&bad).contains("isn't one of the ORM options"),
            "{}",
            said(&bad)
        );
    }

    /// Nothing is created without confirm, nor over a folder that exists.
    #[tokio::test]
    async fn a_create_refuses_before_anything_runs() {
        let sawit = sawit();
        let dir = std::env::temp_dir().join(format!("sawit-mcp-stack-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("taken")).unwrap();
        let create = |name: &str, confirm: bool| StackCreateArgs {
            location: dir.display().to_string(),
            stack: StackArgs {
                name: Some(name.into()),
                ..Default::default()
            },
            open: None,
            confirm,
        };
        let r = sawit.stack_create_tool(create("shop", false)).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("confirm=true"), "{}", said(&r));
        let r = sawit.stack_create_tool(create("taken", true)).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("already exists"), "{}", said(&r));
        assert!(!dir.join("shop").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `open` reads beside the stack's flags: left out it is Create Project's (the app opens the
    /// project), and false is Create Project Only's.
    #[test]
    fn a_create_says_whether_it_opens_the_project() {
        let args = |json: serde_json::Value| -> StackCreateArgs {
            serde_json::from_value(json).expect("the arguments read")
        };
        let only =
            args(json!({"location": "/tmp", "name": "shop", "open": false, "confirm": true}));
        assert_eq!(only.open, Some(false));
        assert_eq!(only.stack.name.as_deref(), Some("shop"));
        let default = args(json!({"location": "/tmp", "frontend": ["next"], "confirm": true}));
        assert_eq!(default.open, None);
        assert_eq!(default.stack.frontend, Some(vec!["next".to_string()]));
    }
}
