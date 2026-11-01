//! The port against upstream: golden commands, upstream's CLI compatibility cases, and
//! `upstream.json`, which running upstream's own TypeScript at [`UPSTREAM_COMMIT`] wrote
//! (`scripts/better-t-stack-fixture/`): random stacks with everything the builder says about
//! them, and random walks of clicks and tag removals.

use serde_json::Value;

use super::*;

/// The fixture: what upstream's builder answered.
fn fixture() -> Value {
    serde_json::from_str(include_str!("upstream.json")).expect("upstream.json is JSON")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect()
}

/// A stack as the fixture writes it: the categories in [`Stack`]'s field order, lists joined
/// by commas, then the name. A list of web or native frontends keeps its last one, as
/// upstream's `sanitizeSingleSelection` does.
fn decode(line: &str) -> Stack {
    let f: Vec<&str> = line.split('|').collect();
    assert_eq!(f.len(), 18, "{line}");
    let last = |ids: &str| {
        ids.split(',')
            .rfind(|id| *id != "none")
            .unwrap_or("none")
            .to_string()
    };
    let list = |ids: &str| -> Vec<String> {
        ids.split(',')
            .filter(|id| *id != "none" && !id.is_empty())
            .map(String::from)
            .collect()
    };
    let parse = |category: Category, id: &str| {
        let mut stack = Stack::default();
        assert!(stack.set(category, id), "{category:?} {id}");
        stack
    };
    let web = parse(Category::WebFrontend, &last(f[0])).web_frontend;
    let native = parse(Category::NativeFrontend, &last(f[1])).native_frontend;
    Stack {
        project_name: f[17].to_string(),
        web_frontend: web,
        native_frontend: native,
        backend: Backend::parse(f[2]).expect(f[2]),
        runtime: Runtime::parse(f[3]).expect(f[3]),
        api: Api::parse(f[4]).expect(f[4]),
        database: Database::parse(f[5]).expect(f[5]),
        orm: Orm::parse(f[6]).expect(f[6]),
        db_setup: DbSetup::parse(f[7]).expect(f[7]),
        web_deploy: Deploy::parse(f[8]).expect(f[8]),
        server_deploy: Deploy::parse(f[9]).expect(f[9]),
        auth: Auth::parse(f[10]).expect(f[10]),
        payments: Payments::parse(f[11]).expect(f[11]),
        package_manager: Pm::parse(f[12]).expect(f[12]),
        addons: list(f[13])
            .iter()
            .map(|id| Addon::parse(id).expect(id))
            .collect(),
        examples: list(f[14])
            .iter()
            .map(|id| Example::parse(id).expect(id))
            .collect(),
        git: f[15] == "true",
        install: f[16] == "true",
    }
}

/// [`decode`]'s line for `stack`.
fn encode(stack: &Stack) -> String {
    let mut parts: Vec<String> = [
        Category::WebFrontend,
        Category::NativeFrontend,
        Category::Backend,
        Category::Runtime,
        Category::Api,
        Category::Database,
        Category::Orm,
        Category::DbSetup,
        Category::WebDeploy,
        Category::ServerDeploy,
        Category::Auth,
        Category::Payments,
        Category::PackageManager,
        Category::Addons,
        Category::Examples,
        Category::Git,
        Category::Install,
    ]
    .iter()
    .map(|&c| stack.picked(c).join(","))
    .collect();
    parts.push(stack.project_name.clone());
    parts.join("|")
}

fn changes(resolved: &Resolved) -> Vec<String> {
    resolved
        .changes
        .iter()
        .map(|c| format!("{}: {}", c.category.id(), c.message))
        .collect()
}

/// Upstream's command for the same stack, which says `@latest` where Sawit pins the version.
fn pinned(upstream: &str) -> String {
    upstream.replacen("@latest", &format!("@{CLI_VERSION}"), 1)
}

/// The default stack and each preset, with each package manager: exactly upstream's command,
/// apart from the pinned version.
#[test]
fn the_default_stack_and_every_preset_make_upstreams_command() {
    let fixture = fixture();
    let golden = fixture["golden"].as_array().expect("goldens");
    assert_eq!(golden.len(), 15);
    for case in golden {
        let name = case["name"].as_str().unwrap();
        let pm = Pm::parse(case["packageManager"].as_str().unwrap()).unwrap();
        let start = match name {
            "default" => Stack::default(),
            id => Preset::find(id).expect(id).stack(),
        };
        let resolved = resolve(&Stack {
            package_manager: pm,
            ..start
        });
        assert_eq!(
            changes(&resolved),
            strings(&case["changes"]),
            "{name} {pm:?}"
        );
        assert_eq!(encode(&resolved.stack), case["stack"].as_str().unwrap());
        assert_eq!(
            command(&resolved.stack),
            pinned(case["command"].as_str().unwrap()),
            "{name} {pm:?}"
        );
    }
    // Spelled out, for the three a reader looks for first.
    assert_eq!(
        command(&Stack::default()),
        "bun create better-t-stack@3.49.0 my-better-t-app --yes"
    );
    let t3 = resolve(&Preset::find("t3").unwrap().stack()).stack;
    assert_eq!(
        command(&t3),
        "bun create better-t-stack@3.49.0 my-better-t-app --frontend next --backend self \
         --runtime none --api trpc --auth better-auth --payments none --database postgres \
         --orm prisma --db-setup none --package-manager bun --git --web-deploy none \
         --server-deploy none --install --addons biome turborepo --examples none"
    );
    let npm = Stack {
        package_manager: Pm::Npm,
        ..Stack::default()
    };
    assert!(
        command(&npm).starts_with(
            "npx create-better-t-stack@3.49.0 my-better-t-app --frontend tanstack-router"
        ),
        "{}",
        command(&npm)
    );
}

/// Upstream's CLI compatibility cases (`apps/cli/test/compatibility.test.ts`) the builder can
/// express: each stack the CLI rejects comes out of the rules adjusted as upstream's builder
/// adjusts it, and the three the CLI accepts come out as they went in.
#[test]
fn upstreams_cli_compatibility_cases_resolve_as_upstreams_builder_does() {
    let fixture = fixture();
    let cases = fixture["compat"].as_array().expect("compat cases");
    assert_eq!(cases.len(), 45);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = decode(case["input"].as_str().unwrap());
        let resolved = resolve(&input);
        assert_eq!(
            encode(&resolved.stack),
            case["resolved"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(changes(&resolved), strings(&case["changes"]), "{name}");
        assert_eq!(
            command(&resolved.stack),
            pinned(case["command"].as_str().unwrap()),
            "{name}"
        );
        match case["kind"].as_str().unwrap() {
            "accepted" => assert_eq!(resolved.stack, input, "{name} kept every pick"),
            _ => assert_ne!(
                encode(&resolved.stack),
                case["input"].as_str().unwrap(),
                "{name} was left as the CLI rejects it"
            ),
        }
    }
}

/// Random stacks: the resolved stack, its changes, every disabled reason, the tags, the desktop
/// note, the name's error and the command are upstream's.
#[test]
fn random_stacks_resolve_and_disable_as_upstream_does() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 60);
    for (ix, case) in cases.iter().enumerate() {
        let input = decode(case["input"].as_str().unwrap());
        let resolved = resolve(&input);
        let at = format!("case {ix}: {}", case["input"]);
        assert_eq!(
            encode(&resolved.stack),
            case["resolved"].as_str().unwrap(),
            "{at}"
        );
        assert_eq!(changes(&resolved), strings(&case["changes"]), "{at}");
        let reasons: Vec<String> = disabled(&resolved.stack)
            .into_iter()
            .map(|(c, id, reason)| format!("{}/{id}: {reason}", c.id()))
            .collect();
        assert_eq!(reasons, strings(&case["disabled"]), "{at}");
        let tags: Vec<String> = selected(&resolved.stack)
            .into_iter()
            .map(|(c, o)| format!("{}/{}", c.id(), o.id))
            .collect();
        assert_eq!(tags, strings(&case["selected"]), "{at}");
        assert_eq!(
            resolved.notes.first().map(String::as_str),
            case["note"].as_str(),
            "{at}"
        );
        assert_eq!(
            validate_project_name(&resolved.stack.project_name).as_deref(),
            case["nameError"].as_str(),
            "{at}"
        );
        assert_eq!(
            command(&resolved.stack),
            pinned(case["command"].as_str().unwrap()),
            "{at}"
        );
    }
}

/// Random walks of card clicks and tag removals from the default stack: after each, the stack
/// and what changed are upstream's.
#[test]
fn clicks_and_removals_walk_as_upstreams_builder_does() {
    let fixture = fixture();
    let walks = fixture["walks"].as_array().expect("walks");
    assert_eq!(walks.len(), 40);
    for (w, walk) in walks.iter().enumerate() {
        let mut stack = Stack {
            package_manager: Pm::parse(walk["packageManager"].as_str().unwrap()).unwrap(),
            ..Stack::default()
        };
        for (s, step) in walk["steps"].as_array().unwrap().iter().enumerate() {
            let category = Category::parse(step["category"].as_str().unwrap()).unwrap();
            let id = step["id"].as_str().unwrap();
            let op = step["op"].as_str().unwrap();
            let done = match op {
                "select" => select(&stack, category, id),
                _ => remove(&stack, category, id),
            };
            let at = format!("walk {w} step {s}: {op} {category:?} {id}");
            assert_eq!(encode(&done.stack), step["after"].as_str().unwrap(), "{at}");
            assert_eq!(changes(&done), strings(&step["changes"]), "{at}");
            stack = done.stack;
        }
    }
}

/// The catalog has every upstream category, in upstream's order, with upstream's options.
#[test]
fn every_upstream_category_and_option_is_there() {
    let ids: Vec<&str> = Category::ALL.iter().map(|c| c.id()).collect();
    assert_eq!(
        ids,
        [
            "webFrontend",
            "nativeFrontend",
            "backend",
            "runtime",
            "api",
            "database",
            "orm",
            "dbSetup",
            "webDeploy",
            "serverDeploy",
            "auth",
            "payments",
            "packageManager",
            "addons",
            "examples",
            "git",
            "install"
        ]
    );
    let count: usize = Category::ALL.iter().map(|c| options(*c).len()).sum();
    assert_eq!(count, 93);
    // Each option's id parses as the category's value, and none is missing from the catalog.
    for &category in Category::ALL {
        for o in options(category) {
            assert!(
                Stack::default().clone().set(category, o.id),
                "{category:?} {}",
                o.id
            );
        }
    }
    assert_eq!(options(Category::Addons).len(), Addon::ALL.len());
    assert_eq!(options(Category::Backend).len(), Backend::ALL.len());
    assert!(option(Category::WebDeploy, "vercel").unwrap().experimental);
    assert!(option(Category::DbSetup, "none").unwrap().default);
}

/// What Create Project adds to the command: npx doesn't ask before downloading, the CLI never
/// asks about the folder or which editor to open, and only the database setups that would ask
/// how to provision are set up by hand.
#[test]
fn the_run_command_never_stops_to_ask() {
    assert_eq!(
        run_command(&Stack::default()),
        "bun create better-t-stack@3.49.0 my-better-t-app --yes --directory-conflict error --open none"
    );
    let npm = Stack {
        package_manager: Pm::Npm,
        ..Stack::default()
    };
    assert!(run_command(&npm).starts_with("npx -y create-better-t-stack@3.49.0 my-better-t-app "));
    let neon = resolve(&Stack {
        database: Database::Postgres,
        db_setup: DbSetup::Neon,
        ..Stack::default()
    })
    .stack;
    assert!(run_command(&neon).ends_with(" --open none --manual-db"));
    // Alchemy provisions Neon for a Cloudflare deploy, without asking.
    let alchemy = resolve(&Stack {
        web_frontend: WebFrontend::Next,
        backend: Backend::SelfNext,
        runtime: Runtime::None,
        database: Database::Postgres,
        db_setup: DbSetup::Neon,
        web_deploy: Deploy::Cloudflare,
        ..Stack::default()
    })
    .stack;
    assert_eq!(alchemy.db_setup, DbSetup::Neon);
    assert!(!run_command(&alchemy).contains("--manual-db"));
    let docker = resolve(&Stack {
        database: Database::Postgres,
        db_setup: DbSetup::Docker,
        ..Stack::default()
    })
    .stack;
    assert!(!run_command(&docker).contains("--manual-db"));
}

/// Upstream's name rules, and its quoting in the command.
#[test]
fn names_are_checked_and_quoted_as_upstream_does() {
    assert_eq!(validate_project_name("my-app"), None);
    assert_eq!(validate_project_name(""), None, "the default name");
    assert_eq!(validate_project_name("apps/web"), None);
    assert_eq!(
        validate_project_name(".hidden").as_deref(),
        Some("Project name cannot start with a dot (except for '.')")
    );
    assert_eq!(
        validate_project_name("-x").as_deref(),
        Some("Project name cannot start with a dash")
    );
    assert_eq!(
        validate_project_name("what?").as_deref(),
        Some("Project name contains invalid characters")
    );
    assert_eq!(
        validate_project_name("Node_Modules").as_deref(),
        Some("Project name is reserved")
    );
    assert!(
        validate_project_name("a\u{1}b").is_some(),
        "control characters"
    );
    assert_eq!(format_project_name("my  cool app"), "my-cool-app");
    let named = |name: &str| Stack {
        project_name: name.into(),
        ..Stack::default()
    };
    assert_eq!(
        command(&named("it's mine")),
        r"bun create better-t-stack@3.49.0 'it'\''s-mine' --yes"
    );
    assert_eq!(
        command(&named("café")),
        "bun create better-t-stack@3.49.0 'café' --yes"
    );
}

/// Picks by the CLI's flag names: a preset to start from, `self` as the web frontend's own
/// backend, `none` as an empty list, and clear errors for what isn't an option.
#[test]
fn choices_take_the_clis_flag_names() {
    let choices = Choices {
        template: Some("T3".into()),
        name: Some("shop".into()),
        frontend: Some(vec!["next".into(), "native-uniwind".into()]),
        backend: Some("self".into()),
        addons: Some(vec!["none".into()]),
        package_manager: Some("pnpm".into()),
        ..Default::default()
    };
    let stack = choices.stack().unwrap();
    assert_eq!(stack.backend, Backend::SelfNext);
    assert_eq!(stack.native_frontend, NativeFrontend::Uniwind);
    assert_eq!(stack.orm, Orm::Prisma, "from the preset");
    assert!(stack.addons.is_empty());
    assert_eq!(stack.package_manager, Pm::Pnpm);
    assert_eq!(stack.project_name, "shop");
    let error = |c: Choices| c.stack().unwrap_err();
    assert!(
        error(Choices {
            frontend: Some(vec!["next".into(), "svelte".into()]),
            ..Default::default()
        })
        .contains("one web frontend")
    );
    assert!(
        error(Choices {
            backend: Some("self".into()),
            ..Default::default()
        })
        .contains("fullstack")
    );
    assert!(
        error(Choices {
            database: Some("oracle".into()),
            ..Default::default()
        })
        .contains("sqlite, postgres")
    );
    assert!(
        error(Choices {
            package_manager: Some("yarn".into()),
            ..Default::default()
        })
        .contains("npm, pnpm, bun")
    );
    assert!(
        error(Choices {
            template: Some("lamp".into()),
            ..Default::default()
        })
        .contains("isn't a preset")
    );
}

/// A stack reads back from JSON with upstream's ids, and fields left out are the default's.
#[test]
fn a_stack_reads_and_writes_upstreams_ids() {
    let stack: Stack =
        serde_json::from_str(r#"{"backend": "convex", "addons": ["biome"]}"#).unwrap();
    assert_eq!(stack.backend, Backend::Convex);
    assert_eq!(stack.web_frontend, WebFrontend::TanstackRouter);
    let json = serde_json::to_value(&stack).unwrap();
    assert_eq!(json["addons"], serde_json::json!(["biome"]));
    assert_eq!(json["db_setup"], "none");
    assert!(serde_json::from_str::<Stack>(r#"{"orm": "sequelize"}"#).is_err());
}

/// Upstream's categories as its messages name them.
#[test]
fn categories_have_upstreams_display_names() {
    assert_eq!(Category::DbSetup.display_name(), "Db Setup");
    assert_eq!(Category::Api.display_name(), "Api");
    assert_eq!(Category::ServerDeploy.display_name(), "Server Deploy");
}
