//! Code mode: the API made from the tools, the script runner's sandbox and limits, and a client
//! that sees only `search_api` and `run` doing a chain of calls in one run.

use std::time::Duration;

use rmcp::ServiceExt as _;
use rmcp::model::{CallToolRequestParams, ContentBlock};
use sawit_core::instances::AppPaths;
use serde_json::{Value, json};

use super::script::{self, Limits};
use super::{Api, CODE_INSTRUCTIONS, CODE_MODE_TOOLS, ToolsMode};
use crate::Sawit;

fn quick() -> Limits {
    Limits {
        time: Duration::from_secs(5),
        memory: 32 << 20,
    }
}

fn run(code: &str) -> script::Outcome {
    let names = vec!["echo".to_string(), "boom".to_string()];
    let host: script::Host = Box::new(|name: &str, args: Value| match name {
        "echo" => Ok(json!({ "got": args })),
        "boom" => Err("it broke".into()),
        other => Err(format!("no {other}")),
    });
    script::run(code, &names, host, quick())
}

#[test]
fn the_setting_picks_the_tools() {
    assert_eq!(ToolsMode::parse(None), ToolsMode::All);
    assert_eq!(ToolsMode::parse(Some("all")), ToolsMode::All);
    assert_eq!(ToolsMode::parse(Some(" Code ")), ToolsMode::Code);
    assert_eq!(ToolsMode::parse(Some("code-mode")), ToolsMode::Code);
    assert_eq!(ToolsMode::parse(Some("nonsense")), ToolsMode::All);
}

#[test]
fn code_mode_lists_two_tools_and_keeps_every_other_callable() {
    let paths = AppPaths {
        root: std::env::temp_dir(),
    };
    let all = Sawit::with_mode(paths.clone(), ToolsMode::All).listed_tools();
    let code = Sawit::with_mode(paths, ToolsMode::Code).listed_tools();
    let names: Vec<&str> = code.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(names, ["run", "search_api"]);
    assert!(all.len() > 150, "{}", all.len());
    // The API is every listed tool but code mode's own.
    assert_eq!(
        Sawit::api().functions.len(),
        all.len() - CODE_MODE_TOOLS.len()
    );
    assert!(CODE_INSTRUCTIONS.chars().count() < 4000);
    assert!(CODE_INSTRUCTIONS.contains("canvas_design_guide"));
}

#[test]
fn every_function_has_a_typed_signature() {
    let api = Sawit::api();
    for f in &api.functions {
        let sig = f.signature();
        assert!(sig.contains(&format!("{}(", f.name)), "{sig}");
        assert!(!sig.contains("Record<string, any>): any") || f.schema.get("properties").is_none());
        // Every argument the schema has is in the signature.
        if let Some(props) = f.schema.get("properties").and_then(Value::as_object) {
            for name in props.keys() {
                assert!(
                    sig.contains(&format!("{name}?:")) || sig.contains(&format!("{name}:")),
                    "{} lacks {name}:\n{sig}",
                    f.name
                );
            }
        }
    }
    let commit = api.get("git_commit").expect("git_commit").signature();
    assert!(commit.contains("git_commit(args: {"), "{commit}");
    assert!(commit.contains("repo: string;"), "{commit}");
    assert!(commit.contains("amend?: boolean;"), "{commit}");
    assert!(commit.contains("destructive"), "{commit}");
}

#[test]
fn search_finds_functions_by_what_they_do() {
    let api = Sawit::api();
    let first = |q: &str| api.search(q).first().map(|f| f.name.clone());
    assert_eq!(first("git commit").as_deref(), Some("git_commit"));
    assert_eq!(first("terminal_read").as_deref(), Some("terminal_read"));
    assert_eq!(
        first("read terminal output").as_deref(),
        Some("terminal_read")
    );
    assert_eq!(
        first("screenshot browser").as_deref(),
        Some("browser_screenshot")
    );
    let git: Vec<String> = api.search("git").iter().map(|f| f.name.clone()).collect();
    assert!(git.iter().take(9).all(|n| n.starts_with("git_")), "{git:?}");
    assert!(api.search("zzqx qqzv").is_empty());
    let overview = api.overview();
    for f in &api.functions {
        assert!(overview.contains(&f.name), "{} not in the overview", f.name);
    }
    // An overview of 160 names stays far below the schemas it replaces.
    assert!(overview.len() < 8_000, "{}", overview.len());
}

#[test]
fn areas_group_names_by_their_noun() {
    use super::api::area_of;
    assert_eq!(area_of("git_status"), "git");
    assert_eq!(area_of("list_prompts"), "prompts");
    assert_eq!(area_of("list_ssh_hosts"), "ssh");
    assert_eq!(area_of("canvas_screen_write"), "canvas");
    assert_eq!(area_of("query"), "query");
}

#[test]
fn a_script_returns_its_value_and_its_console() {
    let out =
        run("const a = sawit.echo({n: 1}); console.log('saw', a.got.n); return {n: a.got.n + 1};");
    assert_eq!(out.error, None);
    assert_eq!(out.value, Some(json!({ "n": 2 })));
    assert_eq!(out.logs, ["saw 1"]);
    assert_eq!(out.calls, 1);

    // One expression returns its value, with or without await or a semicolon.
    assert_eq!(
        run("sawit.echo({x: 'y'})").value,
        Some(json!({ "got": { "x": "y" } }))
    );
    assert_eq!(
        run("await sawit.echo({});").value,
        Some(json!({ "got": {} }))
    );
    assert_eq!(
        run("sawit.call('echo', {k: true}).got.k").value,
        Some(json!(true))
    );
    assert_eq!(run("let x = 1;").value, None);
    // async functions, Promise.all and JSON work.
    let out = run(
        "const f = async (n) => sawit.echo({n}).got.n * 2;\nconst all = await Promise.all([f(1), f(2)]);\nreturn JSON.parse(JSON.stringify(all));",
    );
    assert_eq!(out.value, Some(json!([2, 4])), "{:?}", out.error);
}

#[test]
fn a_failed_call_throws_and_can_be_caught() {
    let out = run("sawit.boom({})");
    let error = out.error.expect("an error");
    assert!(error.contains("it broke"), "{error}");
    let out = run("try { sawit.boom({}) } catch (e) { return e.message + ' in ' + e.tool }");
    assert_eq!(out.value, Some(json!("it broke in boom")));
    let out = run("return sawit.nothing_here()");
    assert!(out.error.unwrap().contains("not a function"));
    let out = run("const = ;");
    assert!(out.error.unwrap().contains("SyntaxError"));
}

#[test]
fn a_script_has_no_way_out_of_the_sandbox() {
    for code in [
        "return typeof fetch",
        "return typeof require",
        "return typeof setTimeout",
        "return typeof std",
        "return typeof os",
        "return typeof __call",
    ] {
        assert_eq!(run(code).value, Some(json!("undefined")), "{code}");
    }
    assert!(run("await import('os')").error.is_some());
    // `sawit` can't be replaced or changed.
    assert_eq!(
        run("try { sawit.echo = 1 } catch {} return typeof sawit.echo").value,
        Some(json!("function"))
    );
}

#[test]
fn a_script_stops_at_its_limits() {
    let names: Vec<String> = Vec::new();
    let limits = Limits {
        time: Duration::from_millis(300),
        memory: 16 << 20,
    };
    let started = std::time::Instant::now();
    let out = script::run(
        "while (true) {}",
        &names,
        Box::new(|_: &str, _| Ok(Value::Null)),
        limits,
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(out.error.unwrap().contains("out of time"));
    let out = script::run(
        "const a = []; while (true) a.push('x'.repeat(1 << 16));",
        &names,
        Box::new(|_: &str, _| Ok(Value::Null)),
        Limits {
            time: Duration::from_secs(10),
            memory: 16 << 20,
        },
    );
    assert!(out.error.unwrap().contains("memory"));
}

/// A client in code mode sees two tools, finds the prompt functions, and creates and fills a
/// prompt in one run: the intermediate answers never reach it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_in_code_mode_chains_calls_in_one_run() {
    let home = tempfile::tempdir().unwrap();
    let server = Sawit::with_mode(
        AppPaths {
            root: home.path().to_path_buf(),
        },
        ToolsMode::Code,
    );
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client");

    let info = client.peer_info().expect("server info");
    assert_eq!(info.instructions.as_deref(), Some(CODE_INSTRUCTIONS));
    let tools = client.list_all_tools().await.expect("tools");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(names, ["run", "search_api"]);

    let text = |r: rmcp::model::CallToolResult| -> (String, bool) {
        let t = r
            .content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        (t, r.is_error == Some(true))
    };
    let call = |name: &'static str, args: Value| {
        CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())
    };

    let (found, _) = text(
        client
            .call_tool(call("search_api", json!({"query": "fill prompt"})))
            .await
            .unwrap(),
    );
    assert!(found.contains("fill_prompt(args: {"), "{found}");

    let script = r#"
        const made = sawit.create_prompt({ title: "Greet", body: "Hello {{who}}" });
        const id = made.match(/id (\S+)\./)[1];
        console.log("made", id);
        return sawit.fill_prompt({ id, values: { who: "Sawit" } });
    "#;
    let (out, failed) = text(
        client
            .call_tool(call("run", json!({"code": script})))
            .await
            .unwrap(),
    );
    assert!(!failed, "{out}");
    assert!(out.starts_with("Hello Sawit"), "{out}");
    assert!(out.contains("Console:\nmade "), "{out}");

    // A tool that refuses still refuses from a script: the confirmation is the tool's.
    let (out, failed) = text(
        client
            .call_tool(call(
                "run",
                json!({"code": "sawit.delete_prompt({id: 'x', confirm: 'y'})"}),
            ))
            .await
            .unwrap(),
    );
    assert!(failed, "{out}");
    assert!(
        out.contains("The script failed after 1 Sawit call"),
        "{out}"
    );

    // The old tool names still answer, though they aren't listed.
    let (out, failed) = text(
        client
            .call_tool(call("list_prompts", json!({})))
            .await
            .unwrap(),
    );
    assert!(!failed && out.contains("Greet"), "{out}");
    client.cancel().await.unwrap();
}

/// What `tools/list` costs a client in each mode, for the audit note (bytes of its JSON, and
/// about a quarter of that in tokens). Run with `--nocapture` to see the numbers.
#[test]
fn tools_list_size_in_each_mode() {
    let paths = AppPaths {
        root: std::env::temp_dir(),
    };
    let size = |mode| {
        let tools = Sawit::with_mode(paths.clone(), mode).listed_tools();
        serde_json::to_string(&json!({ "tools": tools }))
            .unwrap()
            .len()
    };
    let (all, code) = (size(ToolsMode::All), size(ToolsMode::Code));
    println!(
        "tools/list: all {all} bytes (~{} tokens), code {code} bytes (~{} tokens)",
        all / 4,
        code / 4
    );
    assert!(
        code * 20 < all,
        "code mode lists {code} bytes against {all}"
    );
    let instructions = (crate::INSTRUCTIONS.len(), CODE_INSTRUCTIONS.len());
    println!(
        "instructions: all {} bytes, code {} bytes",
        instructions.0, instructions.1
    );
    let _ = Api::new(&[]);
}
