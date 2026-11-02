//! Paths with a line and column, and the options before them.

use super::*;

#[test]
fn targets_with_positions() {
    assert_eq!(target("a.rs", &[]), at("/p/a.rs", None, None));
    assert_eq!(target("a.rs:12", &[]), at("/p/a.rs", Some(12), None));
    assert_eq!(target("a.rs:12:5", &[]), at("/p/a.rs", Some(12), Some(5)));
    assert_eq!(target("a.rs:12:5:", &[]), at("/p/a.rs", Some(12), Some(5)));
    assert_eq!(target("/x/b.sql:3", &[]), at("/x/b.sql", Some(3), None));
    // Only two numbers are taken; the rest is part of the name.
    assert_eq!(target("a:1:2:3", &[]), at("/p/a:1", Some(2), Some(3)));
    assert_eq!(target(":12", &[]), at("/p/:12", None, None));
    assert_eq!(target("a.rs:x", &[]), at("/p/a.rs:x", None, None));
}

#[test]
fn existing_names_with_colons_are_not_split() {
    assert_eq!(
        target("notes:2024", &["/p/notes:2024"]),
        at("/p/notes:2024", None, None)
    );
}

#[test]
fn dot_segments_are_removed() {
    assert_eq!(
        absolute(Path::new("/nope/a/b"), Path::new("../c/./d.rs")),
        PathBuf::from("/nope/a/c/d.rs")
    );
    assert_eq!(
        absolute(Path::new("/nope"), Path::new("/abs/x")),
        PathBuf::from("/abs/x")
    );
}

#[test]
fn flags() {
    assert_eq!(parse(&["-h"]), Ok(Invocation::Help));
    assert_eq!(parse(&["--version"]), Ok(Invocation::Version));
    let Ok(Invocation::Open {
        request,
        stdin,
        foreground,
    }) = parse(&["-nw", "a.rs:3", "-", "--foreground", "--", "-b"])
    else {
        panic!()
    };
    assert!(request.new_window && request.wait && !request.reuse);
    assert!(stdin && foreground);
    assert_eq!(
        request.targets,
        vec![at("/nope/a.rs", Some(3), None), at("/nope/-b", None, None)]
    );
    assert!(parse(&["-psn_0_1234"]).is_ok());
}

#[test]
fn bad_flags() {
    assert!(parse(&["--nope"]).is_err());
    assert!(parse(&["-x"]).is_err());
    assert!(parse(&["-n", "-r"]).is_err());
    assert!(parse(&["-w"]).is_err());
    assert!(parse(&["-w", "/"]).is_err());
    assert!(parse(&["-w", "-"]).is_ok());
}
