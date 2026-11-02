//! mesurer's bundle, the glue, the bridge protocol and the store. The glue's own behaviour, run in
//! QuickJS on a stand-in page, is in `tests/glue.rs`.

mod glue;

use std::path::Path;

use serde_json::{Value, json};

use super::*;
use crate::instances::AppPaths;

/// The bundle is the mesurer [`MESURER_VERSION`] names, at [`MESURER_COMMIT`], under mesurer's
/// copyright and the whole MIT permission text, with its LICENSE beside it; the gate carries the
/// same notice.
#[test]
fn the_bundle_is_the_mesurer_version_named() {
    let banner = format!(
        "/*! mesurer {MESURER_VERSION} (ibelick/mesurer@{MESURER_COMMIT}) | MIT License | Copyright (c) 2026 Julien Thibeaut"
    );
    assert!(MESURER_JS.starts_with(&banner), "{}", &MESURER_JS[..200]);
    assert!(MESURER_GATE_JS.starts_with(&banner));
    let licence = include_str!("../../../../../vendor/mesurer/LICENSE");
    assert!(licence.starts_with(
        "MIT License\n\nCopyright (c) 2026 Julien Thibeaut\n\nPermission is hereby granted"
    ));
    // The permission text, line by line, in the banner.
    let head = &MESURER_JS[..4000];
    for line in licence.lines().skip(4).filter(|l| !l.is_empty()) {
        assert!(
            head.contains(&format!(" * {line}")),
            "the banner has “{line}”"
        );
    }
    assert!(head.contains("React and React DOM 19.2.4 (MIT License"));
    assert!(head.contains("Tailwind CSS output (MIT License"));
    // React's own notices close the bundle.
    assert!(MESURER_JS[MESURER_JS.len() - 3000..].contains("@license React"));
    let info: Value = serde_json::from_str(include_str!(
        "../../../../../vendor/mesurer/bundle/BUILD.json"
    ))
    .unwrap();
    assert_eq!(info["mesurer"]["commit"], MESURER_COMMIT);
    assert_eq!(info["mesurer"]["version"], MESURER_VERSION);
    assert!(MESURER_JS.contains(&format!("\"{MESURER_COMMIT}\"")));
}

/// What a page that opens Annotate takes: the bundle under 850 000 bytes, the gate under 8 KiB,
/// the glue under 40 KiB.
#[test]
fn the_bundle_gate_and_glue_stay_small() {
    assert!(MESURER_JS.len() < 850_000, "{}", MESURER_JS.len());
    assert!(
        MESURER_GATE_JS.len() < 8 * 1024,
        "{}",
        MESURER_GATE_JS.len()
    );
    assert!(GLUE_JS.len() < 40 * 1024, "{}", GLUE_JS.len());
}

/// The glue is all there, in order, with nothing left for the app to fill in: everything it is
/// told comes with the mount call.
#[test]
fn the_glue_is_whole_and_has_no_placeholder() {
    for marker in ["__SAWIT_", "/*__", "{{", "TODO"] {
        assert!(!GLUE_JS.contains(marker), "{marker}");
    }
    let order = [
        "---- Bridge ----",
        "---- Host ----",
        "---- Persistence ----",
        "---- Clipboard ----",
        "---- Files",
        "---- Capture",
        "---- Colour sampling",
        "---- Comments",
        "---- Keys and chrome",
        "---- Mount ----",
    ];
    let at: Vec<usize> = order.iter().map(|m| GLUE_JS.find(m).expect(m)).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?}");
    assert!(GLUE_JS.contains(&format!("messageHandlers.{HANDLER}")));
    assert!(GLUE_JS.contains(&format!("\"{HOST_ID}\"")));
    assert!(GLUE_JS.contains("2147483646"));
    let script = world_script();
    assert!(script.starts_with(MESURER_JS) && script.trim_end().ends_with("void 0;"));
    assert_eq!(
        serde_json::from_str::<Value>(&mount_args(
            &json!({"theme": "dark"}),
            &json!({"settings": {}, "workspace": null})
        ))
        .unwrap(),
        json!({"props": {"theme": "dark"}, "snapshot": {"settings": {}, "workspace": null}})
    );
}

/// Every kind the glue posts reads back as itself, from its JSON and to it, and goes to its lane.
#[test]
fn every_message_kind_round_trips() {
    let cases = [
        (
            json!({"kind": "ready", "payload": {"version": "0.2.7", "url": "http://a.test/", "mount_ms": 42.5}}),
            Lane::Foundation,
        ),
        (
            json!({"kind": "log", "payload": {"level": "warn", "text": "x"}}),
            Lane::Foundation,
        ),
        (
            json!({"kind": "store.settings", "payload": {"theme": "dark"}}),
            Lane::Foundation,
        ),
        (
            json!({"kind": "store.workspace", "payload": {"toolMode": "none", "pages": {}}}),
            Lane::Foundation,
        ),
        (
            json!({"kind": "store.clear", "payload": {"what": "workspace"}}),
            Lane::Foundation,
        ),
        (
            json!({"kind": "store.clear", "payload": {"what": "settings"}}),
            Lane::Foundation,
        ),
        (
            json!({"kind": "clipboard.text", "payload": {"text": ".card > h2"}}),
            Lane::Foundation,
        ),
        (
            json!({"kind": "clipboard.image", "payload": {"png_base64": "iVBOR"}}),
            Lane::Capture,
        ),
        (
            json!({"kind": "file.save", "payload": {"name": "a.png", "mime": "image/png", "base64": "AA=="}}),
            Lane::Capture,
        ),
        (
            json!({"kind": "capture.visible", "payload": {}}),
            Lane::Capture,
        ),
        (
            json!({"kind": "capture.frame", "payload": {"max_width": 1280, "quality": 0.8}}),
            Lane::Capture,
        ),
        (
            json!({"kind": "capture.frame", "payload": {}}),
            Lane::Capture,
        ),
        (
            json!({"kind": "color.sample", "payload": {}}),
            Lane::Capture,
        ),
        (
            json!({"kind": "color.cancel", "payload": {}}),
            Lane::Capture,
        ),
        (
            json!({"kind": "comments.copied", "payload": {"text": "URL: …"}}),
            Lane::Agents,
        ),
        (
            json!({"kind": "comments.resolved", "payload": {}}),
            Lane::Agents,
        ),
        (
            json!({"kind": "ui.state", "payload": {"toolMode": "guides"}}),
            Lane::Keys,
        ),
        (
            json!({"kind": "link.open", "payload": {"url": "https://mesurer.dev"}}),
            Lane::Keys,
        ),
    ];
    for (wire, lane) in cases {
        let message = parse(&wire.to_string()).unwrap_or_else(|e| panic!("{wire}: {e}"));
        assert_eq!(message.kind(), wire["kind"], "{wire}");
        assert_eq!(message.lane(), lane, "{wire}");
        assert_eq!(serde_json::to_value(&message).unwrap(), wire);
    }
    // The page that posted it comes with it; an unknown kind or a missing field is refused.
    let posted = json!({"kind": "color.sample", "payload": {}, "url": "http://localhost:3000/a"});
    let envelope = parse_envelope(&posted.to_string()).unwrap();
    assert_eq!(envelope.url.as_deref(), Some("http://localhost:3000/a"));
    assert_eq!(envelope.message, FromMesurer::ColorSample {});
    assert!(
        parse(r#"{"kind":"nope","payload":{}}"#)
            .unwrap_err()
            .contains("“nope”")
    );
    assert!(parse(r#"{"kind":"clipboard.text","payload":{}}"#).is_err());
    assert!(parse("not json").is_err());
}

/// mesurer's page keys (`core/workspace.ts` `getPageKey`), as WebKit's `location` and
/// `URLSearchParams` give them: each expected key was computed by that function on a WHATWG URL.
#[test]
fn page_keys_are_mesurers() {
    let cases = [
        ("http://localhost:3000", "/"),
        ("http://localhost:3000/", "/"),
        ("http://localhost:3000/docs/", "/docs"),
        ("http://localhost:3000/docs///", "/docs"),
        ("http://localhost:3000/docs?b=2&a=1", "/docs?a=1&b=2"),
        (
            "http://localhost:3000/docs?b=2&a=1&a=0",
            "/docs?a=1&a=0&b=2",
        ),
        ("http://localhost:3000/?q=hello%20world", "/?q=hello+world"),
        (
            "http://localhost:3000/?q=a+b&x=%7E&y=*-._",
            "/?q=a+b&x=%7E&y=*-._",
        ),
        ("http://localhost:3000/?z&y=", "/?y=&z="),
        ("http://localhost:3000/?", "/"),
        (
            "http://localhost:3000/app#/settings/profile",
            "/app#/settings/profile",
        ),
        ("http://localhost:3000/app#section", "/app"),
        (
            "http://localhost:3000/app/?b=%C3%A9&a=%F0%9F%98%80#/x?y",
            "/app?a=%F0%9F%98%80&b=%C3%A9#/x?y",
        ),
        ("http://localhost:3000/?%41=1&B=2&a=3", "/?A=1&B=2&a=3"),
        (
            "file:///Users/me/site/index.html",
            "/Users/me/site/index.html",
        ),
        ("http://localhost:3000/a%20b/", "/a%20b"),
        ("http://localhost:3000/?k=a%26b&k2=%3D", "/?k=a%26b&k2=%3D"),
    ];
    for (url, key) in cases {
        assert_eq!(page_key(url), key, "{url}");
    }
}

#[test]
fn origin_keys_name_safe_files() {
    let cases = [
        ("http://localhost:3000/docs?x=1#y", "http_localhost_3000"),
        ("https://My.App.test/", "https_my_app_test"),
        ("https://my.app.test:443/x", "https_my_app_test"),
        ("http://127.0.0.1:5173", "http_127_0_0_1_5173"),
        ("http://[::1]:8080/", "http____1__8080"),
        ("http://user:pw@host.test:81/", "http_host_test_81"),
        ("file:///Users/me/site/index.html", "file"),
        ("about:blank", "about"),
        ("not a url", "page"),
        ("http://../../etc/passwd", "http___"),
    ];
    for (url, key) in cases {
        assert_eq!(origin_key(url), key, "{url}");
        assert!(
            key.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        );
    }
}

#[test]
fn files_are_where_the_plan_says() {
    let paths = AppPaths {
        root: "/data".into(),
    };
    assert_eq!(
        settings_path(&paths),
        Path::new("/data/browser/mesurer/settings.json")
    );
    assert_eq!(
        workspace_path(Some(Path::new("/p")), &paths, "http_localhost_3000"),
        Path::new("/p/.nyawit/browser/mesurer/http_localhost_3000.json")
    );
    assert_eq!(
        workspace_path(None, &paths, "file"),
        Path::new("/data/cache/browser/mesurer/file.json")
    );
}

/// A workspace is written whole and read back; a link, at the file or its folder, is neither
/// read nor written through, nor removed.
#[test]
fn the_store_round_trips_and_refuses_links() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("browser/mesurer/http_localhost_3000.json");
    assert_eq!(read_json(&file).unwrap(), None);
    let envelope = json!({"toolMode": "guides", "pages": {"/": {"guides": [{"id": "g"}]}}});
    write_json(&file, &envelope).unwrap();
    assert_eq!(read_json(&file).unwrap(), Some(envelope.clone()));
    write_json(&file, &json!({"pages": {}})).unwrap();
    assert_eq!(read_json(&file).unwrap(), Some(json!({"pages": {}})));
    // No temporary file is left beside it.
    let names: Vec<_> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");

    let target = dir.path().join("elsewhere.json");
    std::fs::write(&target, "{\"mine\":true}").unwrap();
    let link = dir.path().join("browser/mesurer/linked.json");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(read_json(&link).unwrap_err().contains("link"));
    assert!(write_json(&link, &json!({})).unwrap_err().contains("link"));
    assert!(remove_file(&link).unwrap_err().contains("link"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "{\"mine\":true}");

    // A linked folder is refused too.
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join("browser/linked")).unwrap();
    let through = dir.path().join("browser/linked/x.json");
    assert!(
        write_json(&through, &json!({}))
            .unwrap_err()
            .contains("link")
    );
    assert!(std::fs::read_dir(&real).unwrap().next().is_none());

    remove_file(&file).unwrap();
    assert_eq!(read_json(&file).unwrap(), None);
    remove_file(&file).unwrap();
}

/// A page's summary counts what mesurer has on it, and the session chrome.
#[test]
fn a_summary_counts_the_pages_markup() {
    let envelope = json!({
        "enabled": true, "minimized": false, "xrayVisible": true, "toolMode": "guides",
        "rulersVisible": true, "guideOrientation": "vertical",
        "pages": {
            "/": {
                "guides": [{}, {}], "arrows": [{}], "textAnnotations": [{}], "penStrokes": [{}, {}, {}],
                "layoutGuides": [{}], "heldDistances": [{}],
                "comments": [{"status": "open"}, {"status": "resolved"}, {"status": "open"}]
            },
            "/other": {"guides": [{}]}
        }
    });
    let summary = Summary::of(&envelope, "/");
    assert_eq!(
        summary,
        Summary {
            tool: "guides".into(),
            enabled: true,
            minimized: false,
            xray: true,
            rulers: true,
            comments_open: 2,
            arrows: 1,
            texts: 1,
            strokes: 3,
            guides: 2,
            layout_guides: 1,
            distances: 1,
        }
    );
    assert_eq!(summary.markup(), 11);
    assert_eq!(Summary::of(&envelope, "/none").markup(), 0);
    assert_eq!(
        Summary::of(&Value::Null, "/"),
        Summary {
            tool: "none".into(),
            ..Summary::default()
        }
    );
    assert_eq!(chrome_of(&envelope)["toolMode"], "guides");
    assert!(chrome_of(&envelope).get("pages").is_none());
    assert_eq!(page_artifacts(&envelope, "/other"), json!({"guides": [{}]}));
    assert_eq!(page_artifacts(&envelope, "/x"), Value::Null);
}

/// `{"op": "mesurer", "action": …}` with its arguments beside the action: answered by the app.
#[test]
fn the_mesurer_op_reads_with_its_arguments() {
    use crate::browser::BrowserOp;
    let op: BrowserOp =
        serde_json::from_value(json!({"op": "mesurer", "action": "set", "xray": true})).unwrap();
    let BrowserOp::Mesurer { action, args } = &op else {
        panic!("{op:?}")
    };
    assert_eq!(action, "set");
    assert_eq!(args.get("xray"), Some(&json!(true)));
    assert!(op.is_native() && !op.shows_tab());
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        json!({"op": "mesurer", "action": "set", "xray": true})
    );
    let state: BrowserOp =
        serde_json::from_value(json!({"op": "mesurer", "action": "state"})).unwrap();
    assert_eq!(
        state,
        BrowserOp::Mesurer {
            action: "state".into(),
            args: Default::default()
        }
    );
}
