//! Local Explorer calls are refused before anything is sent when they write unconfirmed or leave
//! the API, go only to an explorer the probe found, and show their answers cut and laid out.

use super::*;

fn sawit() -> Sawit {
    Sawit::new(sawit_core::instances::AppPaths {
        root: std::env::temp_dir(),
    })
}

fn said(r: &CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| c.as_text())
        .map(|t| t.text.clone())
        .collect()
}

/// A write is refused before any dev server is looked for, let alone called; so is a path
/// that leaves the API, and a URL.
#[tokio::test]
async fn writes_and_paths_out_of_the_api_are_refused_first() {
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let a: ExplorerRequestArgs = serde_json::from_value(serde_json::json!({
            "port": 1,
            "method": method,
            "path": "storage/kv/namespaces/x/values/k",
        }))
        .unwrap();
        let r = sawit().local_explorer_request_tool(a).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).contains("confirm: true"), "{}", said(&r));
    }
    for path in [
        "../../etc/passwd",
        "%2e%2e/x",
        "http://example.com/x",
        "//evil/x",
    ] {
        let a: ExplorerRequestArgs =
            serde_json::from_value(serde_json::json!({ "port": 1, "path": path })).unwrap();
        let r = sawit().local_explorer_request_tool(a).await;
        assert_eq!(r.is_error, Some(true));
        assert!(said(&r).starts_with("Refused path"), "{}", said(&r));
    }
}

/// A call goes to an explorer the probe found: a port that serves none is refused, and with no
/// port the project's one is picked, or the only one on this Mac.
#[test]
fn calls_go_to_an_explorer_found() {
    let spec = explorer::parse_spec(br#"{"openapi":"3.0.3","paths":{}}"#).unwrap();
    let at = |port: u16, cwd: &str| Explorer {
        port,
        addr: format!("127.0.0.1:{port}").parse().unwrap(),
        pid: u32::from(port),
        label: "wrangler dev".into(),
        cwd: Some(cwd.into()),
        spec: spec.clone(),
    };
    let both = [at(8787, "/w/shop/api"), at(5173, "/w/blog")];
    assert_eq!(pick(&both, Some(5173), None).unwrap().port, 5173);
    let refused = pick(&both, Some(3000), None).unwrap_err();
    assert!(refused.contains("Nothing on port 3000"), "{refused}");
    assert_eq!(pick(&both, None, Some("/w/shop")).unwrap().port, 8787);
    assert!(pick(&both, None, Some("/w/other")).is_err());
    assert!(
        pick(&[], None, Some("/w/shop"))
            .unwrap_err()
            .contains("No Local Explorer is running")
    );
    let two = [at(8787, "/w/shop/api"), at(8788, "/w/shop/mail")];
    assert!(
        pick(&two, None, Some("/w/shop"))
            .unwrap_err()
            .contains("pass the port")
    );
}

/// Long answers are cut with a note, JSON is laid out, binary bodies aren't dumped.
#[test]
fn answers_are_shown_cut_and_laid_out() {
    let r = |body: Vec<u8>, kind: &str| explorer::Response {
        status: 200,
        content_type: Some(kind.into()),
        body,
        truncated: false,
    };
    let json = answer_text(&r(br#"{"result":[1]}"#.to_vec(), "application/json"));
    assert!(
        json.starts_with("HTTP 200 · application/json · 14 bytes\n{\n"),
        "{json}"
    );
    let big = answer_text(&r(vec![b'x'; SHOWN * 2], "text/plain"));
    assert!(big.contains("answer cut"), "{}", &big[big.len() - 200..]);
    assert!(big.len() < SHOWN + 500);
    let binary = answer_text(&r(vec![0xff, 0xfe, 0x00], "application/octet-stream"));
    assert!(binary.ends_with("(binary body, not shown)"));
    assert_eq!(
        body_of(Some(&serde_json::json!({"sql": "select 1"})), None),
        Some((br#"{"sql":"select 1"}"#.to_vec(), "application/json".into()))
    );
    assert_eq!(
        body_of(Some(&Value::String("hello".into())), None)
            .unwrap()
            .1,
        "text/plain; charset=utf-8"
    );
}
