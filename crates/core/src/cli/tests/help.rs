//! `sawit --help`: every word, alias and flag named, in lines a terminal shows whole.

use super::*;

/// `sawit --help` names `cabut`, its alias and an example.
#[test]
fn help_names_cabut() {
    for said in [
        "sawit cabut <tool ...> [-y]",
        "cabut = uninstall",
        "sawit cabut bun",
    ] {
        assert!(USAGE.contains(said), "no {said:?} in the help");
    }
}

/// `sawit --help` names the lists shown only when asked, with their
/// aliases.
#[test]
fn help_names_the_mcp_and_config_lists() {
    for said in [
        "tools|mcp|skills|config|keys]",
        "sawit list mcp",
        "(colokan)",
        "sawit list config",
        "(configs, konfig)",
        "sawit list explorer",
        "(cf, jelajah)",
    ] {
        assert!(USAGE.contains(said), "no {said:?} in the help");
    }
}

/// `sawit --help` names every word `sawit` takes as a command, every target of `list` and
/// `bakar` and every flag, each alias among them: it left out `ls`, most targets, `--yes`
/// and `--`.
#[test]
fn the_help_names_every_word_and_alias() {
    let named: std::collections::HashSet<&str> = USAGE
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .collect();
    for word in crate::kebun::SUBCOMMANDS {
        assert!(named.contains(word), "--help doesn't name {word}");
    }
    for target in [
        "all",
        "semua",
        "instances",
        "instance",
        "kebun",
        "db",
        "dbs",
        "database",
        "databases",
        "blok",
        "ports",
        "port",
        "lahan",
        "connections",
        "connection",
        "conn",
        "koneksi",
        "konsesi",
        "tools",
        "tool",
        "alat",
        "perkakas",
        "mcp",
        "colokan",
        "skills",
        "skill",
        "bibit",
        "config",
        "configs",
        "konfig",
        "keys",
        "key",
        "keymap",
        "shortcuts",
        "pintasan",
        "explorer",
        "explorers",
        "cloudflare",
        "cf",
        "jelajah",
        "penjelajah",
    ] {
        assert!(ListWhat::parse(Some(target)).is_ok(), "{target}");
        assert!(named.contains(target), "--help doesn't name list {target}");
    }
    for target in ["ports", "port", "lahan", "instances", "instance", "kebun"] {
        assert!(BakarArgs::parse(&[target]).is_ok(), "{target}");
        assert!(named.contains(target), "--help doesn't name bakar {target}");
    }
    for flag in [
        "-y",
        "--yes",
        "--ya",
        "--ulang",
        "--restart",
        "--",
        "-",
        "-n",
        "--new",
        "-r",
        "--reuse",
        "-w",
        "--wait",
        "--foreground",
        "-v",
        "--version",
        "-h",
        "--help",
    ] {
        assert!(named.contains(flag), "--help doesn't name {flag}");
    }
    // The lines a terminal shows whole.
    for line in USAGE.lines() {
        assert!(line.chars().count() <= 81, "too wide: {line}");
    }
}
