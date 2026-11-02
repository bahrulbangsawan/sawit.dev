//! Workers projects read from their files: Wrangler configs, `cloudflare.config.ts`, the Vite
//! plugin, monorepos, dev tool versions, dev scripts and upgrade commands.

use super::*;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A D1 binding's `preview_database_id` is read beside its `database_id`: `wrangler dev` uses
/// the preview database locally, and Miniflare names its file by it.
#[test]
fn a_d1_bindings_preview_database_id_is_read() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("wrangler.toml"),
        "name = \"api\"\n[[d1_databases]]\nbinding = \"DB\"\ndatabase_name = \"shop\"\n\
             database_id = \"prod-1\"\npreview_database_id = \"dev-1\"\n",
    );
    let b = &detect_project(dir.path())[0].bindings[0];
    assert_eq!(b.id.as_deref(), Some("prod-1"));
    assert_eq!(b.preview_id.as_deref(), Some("dev-1"));
}

fn d1(binding: &str, name: &str, id: Option<&str>) -> Binding {
    Binding {
        kind: BindingKind::D1,
        binding: binding.into(),
        name: Some(name.into()),
        id: id.map(Into::into),
        preview_id: None,
    }
}

/// A `wrangler.toml` with every kind Local Explorer shows, the D1 database's name and id,
/// the Worker's name and its crons; an `[env.staging]` binding isn't `wrangler dev`'s.
#[test]
fn a_wrangler_toml_gives_its_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("wrangler.toml"),
        r#"
name = "shop-api"
main = "src/index.ts"

[[kv_namespaces]]
binding = "CACHE"
id = "a1b2"

[[r2_buckets]]
binding = "FILES"
bucket_name = "shop-files"

[[d1_databases]]
binding = "DB"
database_name = "shop-db"
database_id = "1111-2222"

[durable_objects]
bindings = [{ name = "COUNTER", class_name = "Counter" }]

[[workflows]]
binding = "ORDERS"
name = "order-flow"
class_name = "OrderFlow"

[[send_email]]
name = "MAIL"

[triggers]
crons = ["0 * * * *"]

[[env.staging.d1_databases]]
binding = "STAGING_DB"
database_name = "staging"
database_id = "x"
"#,
    );
    let found = detect_project(root);
    assert_eq!(found.len(), 1);
    let w = &found[0];
    assert_eq!(w.name.as_deref(), Some("shop-api"));
    assert_eq!(w.tool, DevTool::Wrangler);
    assert_eq!(w.config, root.join("wrangler.toml"));
    let kinds: Vec<&str> = w.explorer_kinds().iter().map(|k| k.label()).collect();
    assert_eq!(
        kinds,
        [
            "D1",
            "KV",
            "R2",
            "Durable Objects",
            "Workflows",
            "send_email"
        ]
    );
    assert_eq!(w.bindings[0], d1("DB", "shop-db", Some("1111-2222")));
    let counter = w.bindings.iter().find(|b| b.binding == "COUNTER").unwrap();
    assert_eq!(counter.name.as_deref(), Some("Counter"));
    assert!(!w.bindings.iter().any(|b| b.binding == "STAGING_DB"));
    assert!(declares_crons(w));
}

/// `wrangler.jsonc` with comments, a URL whose `//` isn't one, and trailing commas.
#[test]
fn a_wrangler_jsonc_with_comments_gives_its_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("wrangler.jsonc"),
        r#"// The shop's API.
{
  /* Its name on Cloudflare. */
  "name": "shop-api", // trailing comment
  "vars": { "ORIGIN": "https://example.com" },
  "d1_databases": [
    { "binding": "DB", "database_name": "shop-db", "database_id": "1111-2222", },
  ],
  "kv_namespaces": [{ "binding": "CACHE", "id": "a1b2" }],
}
"#,
    );
    let w = &detect_project(root)[0];
    assert_eq!(w.name.as_deref(), Some("shop-api"));
    assert_eq!(w.bindings[0], d1("DB", "shop-db", Some("1111-2222")));
    assert_eq!(w.bindings[1].kind, BindingKind::Kv);
    assert!(!declares_crons(w));
    // Plain `wrangler.json` too.
    let other = tempfile::tempdir().unwrap();
    write(
        &other.path().join("wrangler.json"),
        r#"{"name":"x","r2_buckets":[{"binding":"B","bucket_name":"b"}]}"#,
    );
    let w = &detect_project(other.path())[0];
    assert_eq!(w.explorer_kinds(), [&BindingKind::R2]);
}

/// `cloudflare.config.ts` read as far as it is literals: the Worker's name, its bindings by
/// their keys (values and secrets left out), the D1 database's name and id, and its crons.
/// Commented-out bindings don't count, and with no Wrangler declared `cf dev` serves it.
#[test]
fn a_cloudflare_config_ts_is_read_without_running_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("cloudflare.config.ts"),
        r#"import { bindings, defineConfig, triggers } from "cf/config";

// The API Worker; see https://developers.cloudflare.com/cf/.
export default defineConfig({
  worker: {
    name: "example-worker",
    compatibilityDate: "2026-10-01",
    env: {
      API_ORIGIN: bindings.text("https://api.example.com"),
      API_KEY: bindings.secret(),
      DATABASE: bindings.d1({ name: "application-db", id: "9f1c-42" }),
      CACHE: bindings.kv({ id: "kv-1" }),
      "UPLOADS": bindings.r2({ bucket: "uploads" }),
      MAIL: bindings.sendEmail(),
      ORDERS: bindings.workflow({ name: "order-workflow", worker: "example-worker", exportName: "OrderWorkflow" }),
      // OLD: bindings.d1({ name: "old" }),
      /* GONE: bindings.kv({ id: "x" }), */
      JOBS: bindings.queue<Job>({ name: "application-jobs" }),
    },
    triggers: [triggers.scheduled({ schedule: "*/5 * * * *" })],
  },
});
"#,
    );
    let w = &detect_project(root)[0];
    assert_eq!(w.tool, DevTool::CfCli);
    assert_eq!(w.name.as_deref(), Some("example-worker"));
    let keys: Vec<&str> = w.bindings.iter().map(|b| b.binding.as_str()).collect();
    assert_eq!(
        keys,
        ["DATABASE", "CACHE", "UPLOADS", "MAIL", "ORDERS", "JOBS"]
    );
    assert_eq!(
        w.bindings[0],
        d1("DATABASE", "application-db", Some("9f1c-42"))
    );
    assert_eq!(w.bindings[1].id.as_deref(), Some("kv-1"));
    assert_eq!(w.bindings[2].name.as_deref(), Some("uploads"));
    assert_eq!(w.bindings[4].name.as_deref(), Some("order-workflow"));
    assert_eq!(w.bindings[5].kind, BindingKind::Other("queue".into()));
    assert!(declares_crons(w));

    // A name worked out when it runs isn't guessed; `defineWorker` is read too.
    write(
        &root.join("cloudflare.config.ts"),
        r#"export const api = defineWorker({ name: isStaging ? "a" : "b", env: { DB: bindings.d1() } });"#,
    );
    let w = &detect_project(root)[0];
    assert_eq!(w.name, None);
    assert_eq!(
        w.bindings,
        [Binding {
            kind: BindingKind::D1,
            binding: "DB".into(),
            name: None,
            id: None,
            preview_id: None,
        }]
    );
}

/// The Vite plugin marks a project with no config of its own; with a `wrangler.jsonc` it is
/// still the plugin that serves it, at the version in `node_modules`.
#[test]
fn the_vite_plugin_marks_a_worker() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{"name":"web","scripts":{"dev":"vite","build":"vite build"},"devDependencies":{"@cloudflare/vite-plugin":"^1.50.0","vite":"^7"}}"#,
    );
    write(
        &root.join("node_modules/@cloudflare/vite-plugin/package.json"),
        r#"{"name":"@cloudflare/vite-plugin","version":"1.52.3"}"#,
    );
    let w = &detect_project(root)[0];
    assert_eq!(w.tool, DevTool::VitePlugin);
    assert_eq!(w.config, root.join("package.json"));
    assert_eq!(w.version.as_deref(), Some("1.52.3"));
    assert_eq!(support(w), Support::Ready { cron: false });
    assert_eq!(
        dev_scripts(w),
        [DevScript {
            name: "dev".into(),
            body: "vite".into()
        }]
    );
    write(&root.join("wrangler.jsonc"), r#"{"name":"web"}"#);
    let w = &detect_project(root)[0];
    assert_eq!(
        (w.tool, w.config_name().as_str()),
        (DevTool::VitePlugin, "wrangler.jsonc")
    );
    // A plain Vite app is no Worker.
    let plain = tempfile::tempdir().unwrap();
    write(
        &plain.path().join("package.json"),
        r#"{"devDependencies":{"vite":"^7"}}"#,
    );
    assert!(detect_project(plain.path()).is_empty());
}

/// A monorepo: each workspace package with a Worker is one, with Wrangler hoisted to the
/// root's `node_modules`; a folder beside them with its own config counts as `cf` lays out
/// several Workers; a package with no Worker doesn't.
#[test]
fn a_monorepos_packages_are_each_a_worker() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{"name":"mono","private":true}"#,
    );
    write(
        &root.join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n  - packages/*\n",
    );
    write(&root.join("pnpm-lock.yaml"), "");
    write(
        &root.join("apps/api/package.json"),
        r#"{"name":"@m/api","scripts":{"dev":"wrangler dev --persist-to ../../.state"},"devDependencies":{"wrangler":"^4"}}"#,
    );
    write(
        &root.join("apps/api/wrangler.toml"),
        "name = \"api\"\n[[d1_databases]]\nbinding = \"DB\"\ndatabase_name = \"main\"\ndatabase_id = \"1\"\n",
    );
    write(
        &root.join("apps/site/package.json"),
        r#"{"name":"@m/site","scripts":{"dev":"next dev"}}"#,
    );
    write(
        &root.join("packages/ui/package.json"),
        r#"{"name":"@m/ui"}"#,
    );
    write(
        &root.join("node_modules/wrangler/package.json"),
        r#"{"version":"4.100.2"}"#,
    );
    write(
        &root.join("mail/cloudflare.config.ts"),
        r#"export default defineConfig({ worker: { name: "mail" } });"#,
    );
    let found = detect_project(root);
    let names: Vec<_> = found.iter().map(|w| w.name.as_deref().unwrap()).collect();
    assert_eq!(names, ["api", "mail"]);
    let api = &found[0];
    assert_eq!(api.root, root.join("apps/api"));
    assert_eq!(api.version.as_deref(), Some("4.100.2"));
    assert_eq!(api.persist_to, Some(root.join("apps/api/../../.state")));
    assert_eq!(
        support(api),
        Support::TooOld {
            installed: Version::new(4, 100, 2),
            needed: Version::new(4, 118, 0)
        }
    );
    assert_eq!(upgrade_command(api, root), "pnpm add -D wrangler@latest");
    assert_eq!(dev_scripts(api)[0].name, "dev");
}

/// The minimums from Cloudflare's docs: Wrangler 4.118.0 and the Vite plugin 1.50.0 for
/// Local Explorer, 4.139.0 and 1.60.0 for Cron Triggers; a pre-release comes before its
/// release, and `cf` or a version not installed says nothing.
#[test]
fn versions_are_compared_with_the_minimums() {
    let v = |s: &str| Version::parse(s).unwrap();
    assert!(v("4.118.0") >= v("4.118.0"));
    assert!(v("4.117.9") < v("4.118.0"));
    assert!(v("4.118.0-beta.1") < v("4.118.0"));
    assert!(v("4.118.0-beta.1") > v("4.117.99"));
    assert!(v("4.200") > v("4.139.0"));
    assert_eq!(v("v1.50.0"), Version::new(1, 50, 0));
    assert_eq!(v("1.50.0+build.7"), Version::new(1, 50, 0));
    assert_eq!(Version::parse("latest"), None);
    assert_eq!(Version::parse("1.2.3.4"), None);
    assert_eq!(v("4.118.0").to_string(), "4.118.0");

    let at = |tool: DevTool, version: Option<&str>| {
        support(&WorkersProject {
            root: PathBuf::from("/p"),
            config: PathBuf::from("/p/wrangler.toml"),
            tool,
            version: version.map(Into::into),
            name: None,
            bindings: vec![],
            persist_to: None,
        })
    };
    assert_eq!(
        at(DevTool::Wrangler, Some("4.117.0")),
        Support::TooOld {
            installed: Version::new(4, 117, 0),
            needed: Version::new(4, 118, 0)
        }
    );
    assert_eq!(
        at(DevTool::Wrangler, Some("4.118.0")),
        Support::Ready { cron: false }
    );
    assert_eq!(
        at(DevTool::Wrangler, Some("4.139.0")),
        Support::Ready { cron: true }
    );
    assert_eq!(
        at(DevTool::VitePlugin, Some("1.49.9")),
        Support::TooOld {
            installed: Version::new(1, 49, 9),
            needed: Version::new(1, 50, 0)
        }
    );
    assert_eq!(
        at(DevTool::VitePlugin, Some("1.59.0")),
        Support::Ready { cron: false }
    );
    assert_eq!(
        at(DevTool::VitePlugin, Some("1.60.0")),
        Support::Ready { cron: true }
    );
    assert_eq!(at(DevTool::Wrangler, None), Support::Unknown);
    assert_eq!(at(DevTool::CfCli, Some("1.0.0")), Support::Unknown);
}

/// The scripts that start a Workers dev server, and the ones that only build or preview.
#[test]
fn dev_server_scripts_are_told_from_builds() {
    let yes = |body: &str| starts_dev_server(body, DevTool::Wrangler);
    assert!(yes("wrangler dev"));
    assert!(yes("wrangler dev --port 8787 --persist-to .state"));
    assert!(yes("npm run build && wrangler dev"));
    assert!(yes("concurrently \"wrangler dev\" \"tsc -w\""));
    assert!(yes("cf dev"));
    assert!(yes("vite"));
    assert!(yes("vite --port 3000"));
    assert!(yes("vite dev"));
    assert!(yes("tsc && vite"));
    assert!(yes("./node_modules/.bin/wrangler dev"));
    assert!(!yes("wrangler deploy"));
    assert!(!yes("vite build"));
    assert!(!yes("vite preview"));
    assert!(!yes("vite build && wrangler deploy"));
    assert!(!yes("react-router dev"));
    assert!(starts_dev_server("react-router dev", DevTool::VitePlugin));
}

/// Each package manager's own way to bring a dev dependency up to date.
#[test]
fn the_upgrade_uses_the_projects_package_manager() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("wrangler.toml"), "name = \"w\"\n");
    let command = |root: &Path| upgrade_command(&detect_project(root)[0], root);
    assert_eq!(command(root), "npm install -D wrangler@latest");
    write(&root.join("bun.lock"), "");
    assert_eq!(command(root), "bun add -d wrangler@latest");
    write(
        &root.join("package.json"),
        r#"{"packageManager":"yarn@4.1.0","devDependencies":{"@cloudflare/vite-plugin":"1"}}"#,
    );
    assert_eq!(command(root), "yarn add -D @cloudflare/vite-plugin@latest");
    write(&root.join("package.json"), r#"{"packageManager":"pnpm@9"}"#);
    write(&root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n");
    assert_eq!(command(root), "pnpm add -D wrangler@latest -w");
}
