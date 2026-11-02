//! Names, folders and references.

use std::path::Path;

use crate::screens::{
    component_name, has_scheme, is_name, mime, name_for, resolve, screens_dir, segments,
};

/// A screen's folder is named after its title, in letters, digits and dashes, and a name taken
/// gets a number.
#[test]
fn names_come_from_titles() {
    assert_eq!(name_for("Pricing page", &[]), "pricing-page");
    assert_eq!(name_for("  Sign in / up!  ", &[]), "sign-in-up");
    assert_eq!(name_for("Écran d'accueil", &[]), "cran-d-accueil");
    assert_eq!(name_for("???", &[]), "screen");
    let taken = vec!["login".to_string(), "login-2".to_string()];
    assert_eq!(name_for("Login", &taken), "login-3");
    let long = "a".repeat(200);
    assert!(name_for(&long, &[]).len() <= 48);
    assert!(is_name("login-2") && is_name("404"));
    for bad in [
        "",
        "-x",
        "Login",
        "a_b",
        "a.b",
        "a/b",
        "..",
        &"a".repeat(49),
    ] {
        assert!(!is_name(bad), "{bad}");
    }
}

/// A project's canvas keeps its screens in `.nyawit/screens`; any other canvas file in a folder
/// named after it.
#[test]
fn screens_sit_beside_their_canvas() {
    assert_eq!(
        screens_dir(Path::new("/p/.nyawit/canvas.sawit")),
        Path::new("/p/.nyawit/screens")
    );
    assert_eq!(
        screens_dir(Path::new("/d/flow.sawit")),
        Path::new("/d/flow.screens")
    );
}

/// References resolve from the file that makes them, stay in the folder, and leave other places
/// alone.
#[test]
fn references_resolve_inside_the_folder() {
    assert_eq!(
        resolve("index.html", "css/a.css").as_deref(),
        Some("css/a.css")
    );
    assert_eq!(
        resolve("css/a.css", "../img/x.png").as_deref(),
        Some("img/x.png")
    );
    assert_eq!(
        resolve("css/a.css", "/img/x.png?v=2#y").as_deref(),
        Some("img/x.png")
    );
    assert_eq!(
        resolve("index.html", "my%20logo.png").as_deref(),
        Some("my logo.png")
    );
    for out in [
        "../x",
        "a/../../x",
        "%2e%2e/x",
        "https://a.dev/x",
        "//cdn/x",
        "data:x",
        "#top",
    ] {
        assert_eq!(resolve("index.html", out), None, "{out}");
    }
    assert_eq!(segments("index.html", "/"), Some(Vec::new()));
    assert_eq!(segments("index.html", "a\\b"), None);
    assert!(
        has_scheme("mailto:x")
            && has_scheme("data:")
            && !has_scheme("a/b:c")
            && !has_scheme("x.png")
    );
}

#[test]
fn files_have_their_media_types() {
    assert_eq!(mime("index.html"), "text/html; charset=utf-8");
    assert_eq!(mime("a/B.SVG"), "image/svg+xml");
    assert_eq!(mime("font.woff2"), "font/woff2");
    assert_eq!(mime("noext"), "application/octet-stream");
}

#[test]
fn components_are_named_in_pascal_case() {
    assert_eq!(component_name("pricing-page"), "PricingPage");
    assert_eq!(component_name("404"), "Screen404");
    assert_eq!(component_name("login-2"), "Login2");
}
