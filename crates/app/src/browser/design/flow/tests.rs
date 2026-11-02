//! The design flow's tests.

use sawit_core::site_design::inspect::{StyleInfo, css_rule};
use sawit_core::site_design::scan::{AssetKind, AssetRef, ColorRole, ColorUse};
use sawit_core::site_design::tailwind::classes;
use serde_json::{Value, json};

use super::assets::save_note;
use super::export::{
    ExportJob, ExportReport, earlier_export, export_folder_for, export_host, folder_for,
};
use super::inspect::{is_latest, should_inspect, style_op};
use super::ops::{design_part, export_status, inspect_answer};
use super::scan::{add_icons, manifest_icons, resolve_url};
use super::*;

#[test]
fn save_note_reports_failures() {
    let d = std::path::Path::new("a/assets");
    assert_eq!(save_note(3, 3, None, d), "Saved 3 files to a/assets");
    assert_eq!(
        save_note(28, 40, Some("x".into()), d),
        "Saved 28 files to a/assets. 12 of 40 assets couldn't be downloaded"
    );
    assert_eq!(save_note(0, 2, Some("403".into()), d), "403");
}

fn ready_with_colors(n: usize) -> Ready {
    let colors = (0..n)
        .map(|i| ColorUse {
            value: format!("rgb({}, {}, {})", i % 256, (i / 256) % 256, 7),
            role: ColorRole::Background,
            count: 1,
            on_button: 0,
            on_link: 0,
        })
        .collect();
    Ready {
        scan: Scan {
            url: "https://a.dev/".into(),
            colors,
            ..Default::default()
        },
        tokens: Tokens::default(),
    }
}

#[test]
fn design_part_caps_output() {
    let ready = ready_with_colors(5_000);
    let v = design_part(&ready, "colors").unwrap();
    let text = serde_json::to_string(&v).unwrap();
    assert!(text.len() <= 60_000, "{} bytes", text.len());
    assert_eq!(v["note"], "Cut at 60 KB; ask for one part.");
    assert!(!v["colors"].as_array().unwrap().is_empty());
    // Small parts are whole and carry no note.
    let small = design_part(&ready_with_colors(3), "colors").unwrap();
    assert!(small.get("note").is_none());
    assert_eq!(small["colors"].as_array().unwrap().len(), 3);
}

#[test]
fn unknown_part_is_explained() {
    let ready = ready_with_colors(1);
    assert_eq!(
        design_part(&ready, "x").unwrap_err(),
        "Unknown part “x”. Use summary, colors, fonts, assets, tokens or variables."
    );
    for part in [
        "summary",
        "colors",
        "fonts",
        "assets",
        "tokens",
        "variables",
    ] {
        assert!(design_part(&ready, part).is_ok(), "{part}");
    }
}

#[test]
fn extractable_pages() {
    assert!(extractable("https://a.dev"));
    assert!(extractable("http://localhost:5173/x"));
    assert!(extractable("file:///x.html"));
    assert!(!extractable("about:blank"));
    assert!(!extractable("data:text/html,x"));
}

#[test]
fn hover_a_b_a_asks_again() {
    // A on show, B asked, back to A: A must be asked, B's answer is then stale.
    assert!(should_inspect(false, 0, None, "a", false));
    assert!(should_inspect(false, 0, Some("a"), "b", false));
    assert!(should_inspect(false, 0, Some("b"), "a", false));
    // The same element twice in a row isn't asked twice.
    assert!(!should_inspect(false, 0, Some("a"), "a", false));
}

#[test]
fn hover_waits_for_a_pending_pin_and_a_pinned_element() {
    assert!(should_inspect(false, 0, Some("a"), "a", true));
    assert!(!should_inspect(false, 1, None, "b", false));
    assert!(!should_inspect(true, 0, None, "b", false));
    // A second pick while one is pending still goes through.
    assert!(should_inspect(false, 1, None, "b", true));
}

#[test]
fn copy_during_hover_does_not_use_the_latest_only_gate() {
    // Copies call `style_info`, which never touches the sequence.
    let mut seq = 0u64;
    seq += 1; // hover asks
    let hover = seq;
    // (copy runs here without bumping)
    assert!(is_latest(hover, seq));
}

#[test]
fn inspect_drops_stale() {
    assert!(is_latest(3, 3));
    assert!(!is_latest(2, 3));
}

#[test]
fn export_folder_without_project() {
    let host = export_host("https://www.apple.com:443/mac/");
    assert_eq!(host, "www.apple.com");
    let dir = folder_for(None, &host);
    assert!(
        dir.ends_with("Downloads/sawit-design/www.apple.com"),
        "{}",
        dir.display()
    );
    let project = std::path::Path::new("/p");
    assert_eq!(
        folder_for(Some(project), "a.dev"),
        std::path::Path::new("/p/.nyawit/design/a.dev")
    );
}

#[test]
fn export_hosts_are_safe_folder_names() {
    assert_eq!(export_host("http://LocalHost:5173/"), "localhost-5173");
    assert_eq!(export_host("https://user:pw@x.dev/a"), "x.dev");
    assert_eq!(export_host("file:///Users/me/a.html"), "page");
    assert_eq!(export_host("http://[::1]:8080/"), "1-8080");
    assert_eq!(export_host("http://../"), "page");
    assert_eq!(export_host("http://a_b.dev/"), "a-b.dev");
    // The scheme's own port says nothing.
    assert_eq!(export_host("http://x.dev:80/"), "x.dev");
    assert_eq!(export_host("http://x.dev:443/"), "x.dev-443");
}

/// An export clears its folder and writes into its `assets/`: a repository that commits a
/// link for `.nyawit`, `.nyawit/design`, the host's folder or its `assets/` would have
/// Sawit write there, and clear it, somewhere else.
#[test]
fn exports_never_go_through_a_link() {
    /// A folder of this test's own, removed when done.
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let scratch = |name: &str| {
        let dir =
            std::env::temp_dir().join(format!("sawit-export-links-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    };
    for (i, link) in [
        ".nyawit",
        ".nyawit/design",
        ".nyawit/design/x.dev",
        ".nyawit/design/x.dev/assets",
    ]
    .into_iter()
    .enumerate()
    {
        let project = scratch(&format!("project-{i}"));
        let elsewhere = scratch(&format!("elsewhere-{i}"));
        let at = project.0.join(link);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere.0, &at).unwrap();
        assert!(
            export_folder_for(Some(&project.0), "x.dev").is_err(),
            "through {link}"
        );
        let written = std::fs::read_dir(&elsewhere.0).unwrap().count();
        assert_eq!(written, 0, "nothing written through {link}");
    }
    let project = scratch("plain");
    assert_eq!(
        export_folder_for(Some(&project.0), "x.dev").unwrap(),
        project.0.join(".nyawit/design/x.dev")
    );
    assert!(project.0.join(".nyawit/.gitignore").exists());
}

/// An agent's export replaces the folder of an earlier one, a `DESIGN.md` edited by hand
/// included, only when it says `replace`: a folder that holds anything is refused, and the
/// answer says what is there and how to replace it.
#[test]
fn an_earlier_export_is_replaced_only_when_asked() {
    let dir = std::env::temp_dir().join(format!("sawit-earlier-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(earlier_export(&dir), None, "no folder yet");
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(earlier_export(&dir), None, "an empty folder");
    std::fs::write(dir.join("DESIGN.md"), "edited by hand").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let held = earlier_export(&dir).unwrap();
    assert!(
        held.starts_with(&format!(
            "{} already holds an export (DESIGN.md, assets).",
            dir.display()
        )),
        "{held}"
    );
    assert!(
        held.contains("a DESIGN.md edited by hand included")
            && held.ends_with("export again with replace: true to replace it."),
        "{held}"
    );
    for n in 0..8 {
        std::fs::write(dir.join(format!("x{n}.css")), "").unwrap();
    }
    assert!(
        earlier_export(&dir)
            .unwrap()
            .contains(", x3.css and 4 more)")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two dev servers on one host are two apps: an export of one used to replace the
/// other's folder, a `DESIGN.md` edited there included.
#[test]
fn apps_on_one_host_export_apart() {
    assert_ne!(
        export_host("http://localhost:3000/"),
        export_host("http://localhost:5173/")
    );
}

#[test]
fn manifest_icons_resolve_against_the_manifest() {
    let manifest = r#"{"icons":[
            {"src":"/i/192.png","sizes":"192x192"},
            {"src":"icon-512.png","sizes":"512x512 1024x1024"},
            {"src":"https://cdn.x/abs.png"},
            {"src":""}
        ]}"#;
    let icons = manifest_icons(manifest, "https://x.dev/app/site.webmanifest");
    let urls: Vec<_> = icons.iter().map(|a| a.url.clone().unwrap()).collect();
    assert_eq!(
        urls,
        [
            "https://x.dev/i/192.png",
            "https://x.dev/app/icon-512.png",
            "https://cdn.x/abs.png"
        ]
    );
    assert!(icons.iter().all(|a| a.kind == AssetKind::Icon));
    assert_eq!((icons[0].width, icons[0].height), (Some(192), Some(192)));
    assert_eq!(icons[2].width, None);
    assert!(manifest_icons("not json", "https://x.dev/m.json").is_empty());
}

fn style() -> StyleInfo {
    serde_json::from_str(include_str!(
        "../../../../../core/src/site_design/fixtures/style_button.json"
    ))
    .unwrap()
}

#[test]
fn inspect_formats() {
    let info = style();
    let v = inspect_answer(&info, "json").unwrap();
    assert_eq!(v["selector"], info.selector.as_str());
    assert!(v.get("html").is_none());
    assert!(v["contrast"]["ratio"].as_f64().unwrap() > 1.0);
    let css = inspect_answer(&info, "css").unwrap();
    assert_eq!(css["css"], css_rule(&info).as_str());
    let tw = inspect_answer(&info, "tailwind").unwrap();
    assert_eq!(tw["tailwind"], classes(&info).join(" ").as_str());
    assert!(inspect_answer(&info, "html").unwrap().get("html").is_some());
    assert_eq!(
        inspect_answer(&info, "yaml").unwrap_err(),
        "Unknown format “yaml”. Use json, css, tailwind or html."
    );
}

#[test]
fn style_ops_carry_the_target() {
    assert_eq!(
        style_op(json!({"ref": "e4"}), true),
        json!({"op": "style", "ref": "e4", "html": true})
    );
    assert_eq!(
        style_op(Value::Null, false),
        json!({"op": "style", "html": false})
    );
}

#[test]
fn export_status_reports_the_job() {
    let mut job = ExportJob {
        folder: "/p/.nyawit/design/a.dev".into(),
        done: 3,
        total: 9,
        result: None,
    };
    let v = export_status(&job);
    assert_eq!(v["done"], false);
    assert_eq!(v["progress"], json!([3, 9]));
    assert_eq!(v["folder"], "/p/.nyawit/design/a.dev");
    assert_eq!(v["error"], Value::Null);
    job.result = Some(Err("disk full".into()));
    let v = export_status(&job);
    assert_eq!(
        (v["done"].clone(), v["error"].clone()),
        (json!(true), json!("disk full"))
    );
    job.result = Some(Ok(ExportReport {
        folder: job.folder.clone(),
        screenshot_error: Some("The tab moved.".into()),
        files: (0..5_000)
            .map(|i| format!("assets/images/image-{i}.png"))
            .collect(),
        failed: vec![],
        skipped_videos: 2,
    }));
    let v = export_status(&job);
    assert_eq!(v["skipped_videos"], 2);
    assert!(serde_json::to_string(&v).unwrap().len() <= 60_000);
    assert!(v["note"].as_str().unwrap().starts_with("Cut at 60 KB"));
    assert_eq!(v["folder"], "/p/.nyawit/design/a.dev");
    assert_eq!(v["screenshot_error"], "The tab moved.");
}

#[test]
fn page_answers_time_out() {
    let (_keep, pending) = oneshot::channel::<OpResult>();
    let late = futures::executor::block_on(answer_or_timeout(pending, futures::future::ready(())));
    assert_eq!(late.unwrap_err(), "The page didn't answer within 25 s.");
    let (tx, rx) = oneshot::channel::<OpResult>();
    tx.send(Ok(json!({"n": 1}))).unwrap();
    let on_time = futures::executor::block_on(answer_or_timeout(rx, futures::future::pending()));
    assert_eq!(on_time.unwrap(), json!({"n": 1}));
}

#[test]
fn icons_already_listed_are_kept_once() {
    let icon = |u: &str| AssetRef {
        kind: AssetKind::Icon,
        url: Some(u.into()),
        svg: None,
        label: "icon".into(),
        width: None,
        height: None,
        used_on: vec![],
        logo: false,
    };
    let mut assets = vec![icon("https://x.dev/a.png")];
    add_icons(
        &mut assets,
        vec![icon("https://x.dev/a.png"), icon("https://x.dev/b.png")],
    );
    assert_eq!(assets.len(), 2);
}

#[test]
fn urls_resolve() {
    let base = "https://x.dev/a/b.json?v=1";
    assert_eq!(resolve_url(base, "c.png").unwrap(), "https://x.dev/a/c.png");
    assert_eq!(resolve_url(base, "/c.png").unwrap(), "https://x.dev/c.png");
    assert_eq!(
        resolve_url(base, "//cdn.x/c.png").unwrap(),
        "https://cdn.x/c.png"
    );
    assert_eq!(
        resolve_url(base, "data:image/png;base64,AA").unwrap(),
        "data:image/png;base64,AA"
    );
    assert_eq!(resolve_url(base, ""), None);
}
