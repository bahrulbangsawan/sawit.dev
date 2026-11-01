//! Tests of the export's files, on the fixtures.

use super::*;
use crate::site_design::tokens::derive;

fn scan() -> Scan {
    serde_json::from_str(include_str!("../fixtures/scan.json")).unwrap()
}

fn fixture() -> Tokens {
    derive(&scan(), "2026-10-30")
}

fn mono() -> Tokens {
    let s: Scan = serde_json::from_str(include_str!("../fixtures/mono.json")).unwrap();
    derive(&s, "2026-10-30")
}

#[test]
fn design_md_front_matter() {
    let md = design_md(&fixture());
    assert!(md.starts_with(
        "---\nversion: alpha\nname: Apple design system\ndescription: Extracted from "
    ));
    assert!(md.contains("colors:\n  background: \"#ffffff\""));
    assert!(md.contains("  primary-600: \"#0071e3\""));
    assert!(md.contains("rounded:\n  sm: 4px"));
    assert!(
        md.contains("components:\n  button-primary:\n    backgroundColor: \"{colors.primary}\"")
    );
}

#[test]
fn design_md_sections_in_order() {
    let t = fixture();
    let md = design_md(&t);
    let heads = [
        "## Overview",
        "## Colors",
        "## Typography",
        "## Layout",
        "## Elevation & Depth",
        "## Shapes",
        "## Components",
        "## Do's and Don'ts",
    ];
    let idx: Vec<usize> = heads.iter().map(|h| md.find(h).expect(h)).collect();
    assert!(idx.windows(2).all(|w| w[0] < w[1]), "{idx:?}");
    let flat = Tokens {
        shadows: vec![],
        ..t
    };
    assert!(!design_md(&flat).contains("## Elevation & Depth"));
}

#[test]
fn design_md_prose() {
    let md = design_md(&fixture());
    assert!(md.contains("- **Primary** `#0071e3`: used on 12 buttons; reserve it for actions."));
    assert!(
        md.contains("Body text is 17px/1.47 SF Pro Text; nothing on the page is set below 12px.")
    );
    assert!(md.contains("A light interface built on SF Pro Text, with 9 colors in use."));
    assert!(md.contains("- Do use `{colors.primary}` only for primary actions."));
    assert!(md.contains("- Don't set text below 12px."));
    assert!(md.contains("- Do keep spacing on the 4px grid."));
}

#[test]
fn mono_design_md_omits_what_was_not_seen() {
    let md = design_md(&mono());
    assert!(!md.contains("Do use `{colors.primary}`"));
    assert!(!md.contains("## Shapes") && !md.contains("keep spacing"));
}

#[test]
fn dtcg_shape() {
    let v = dtcg(&fixture());
    assert_eq!(v["color"]["primary"]["600"]["$value"], "#0071e3");
    assert_eq!(v["color"]["primary"]["600"]["$type"], "color");
    assert_eq!(v["color"]["background"]["$type"], "color");
    assert_eq!(v["font"]["size"]["base"]["$value"], "17px");
    assert_eq!(v["shadow"]["md"]["$value"]["blur"], "8px");
    assert_eq!(v["radius"]["sm"]["$value"], "4px");
}

#[test]
fn tailwind_theme_lines() {
    let css = tailwind_theme(&fixture());
    for l in [
        "--color-primary-600: #0071e3;",
        "--text-base: 17px;",
        "--spacing: 4px;",
        "--radius-full: 9999px;",
        "--breakpoint-sm: 734px;",
        "--breakpoint-md: 1068px;",
    ] {
        assert!(css.contains(l), "{l}");
    }
}

#[test]
fn shadcn_lines() {
    let css = shadcn_css(&fixture());
    assert!(css.contains("--primary: oklch(0.5629 0.1933 256.16);"));
    assert!(css.contains("--background: oklch(1 0 0);"));
    assert!(css.contains("Not seen on the page: destructive"));
    let m = shadcn_css(&mono());
    assert!(!m.contains("--primary:"));
    let note = m.split("Not seen on the page:").nth(1).unwrap();
    assert!(note.contains("primary"));
}

#[test]
fn variables_css_lists_them() {
    assert_eq!(
        variables_css(&[("--a".into(), "1px".into())]),
        ":root {\n  --a: 1px;\n}\n"
    );
}

#[test]
fn readme_has_the_license_line() {
    let r = readme(&fixture(), &["DESIGN.md".into()]);
    assert!(r.contains("Assets and fonts belong to their owners; check licenses before shipping."));
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sawit-export-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn write_folder_replaces_old_export() {
    let dir = temp("replace");
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("old.txt"), "x").unwrap();
    std::fs::write(dir.join("assets/x.png"), "x").unwrap();
    let files = write_folder(&dir, &fixture(), &scan(), Some(b"png"), &[]).unwrap();
    assert!(!dir.join("old.txt").exists());
    assert!(dir.join("assets/x.png").exists());
    assert!(files.contains(&"assets/x.png".to_string()));
    assert!(files.contains(&"screenshot.png".to_string()));
    assert!(files.iter().any(|f| f.starts_with("components/")));
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);
    for f in &files {
        assert!(dir.join(f).exists(), "{f}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_folder_without_assets() {
    let dir = temp("bare");
    let files = write_folder(&dir, &fixture(), &scan(), None, &[]).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("assets.json")).unwrap(),
        "[]"
    );
    assert!(!files.contains(&"screenshot.png".to_string()));
    for f in [
        "DESIGN.md",
        "tokens.json",
        "theme.css",
        "shadcn.css",
        "variables.css",
        "README.md",
    ] {
        assert!(files.contains(&f.to_string()), "{f}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
