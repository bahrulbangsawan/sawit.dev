//! The words `sawit` takes as commands, in both languages, and the folders they don't shadow.

use std::os::unix::ffi::OsStringExt;

use super::*;

#[test]
fn subcommands() {
    assert_eq!(parse(&["list"]), Ok(Invocation::List(ListWhat::All)));
    assert_eq!(
        parse(&["ls", "ports"]),
        Ok(Invocation::List(ListWhat::Ports))
    );
    assert!(parse(&["list", "hutan"]).is_err());
    let Ok(Invocation::Bakar(bakar)) = parse(&["bakar", "ports", "-y"]) else {
        panic!()
    };
    assert!(bakar.yes && bakar.targets.is_empty());
    assert!(parse(&["bakar"]).is_err());
    // `sawit ./list` still opens a folder named list.
    assert!(matches!(parse(&["./list"]), Ok(Invocation::Open { .. })));
    assert_eq!(
        parse(&["ls", "alat"]),
        Ok(Invocation::List(ListWhat::Tools))
    );
    assert_eq!(
        parse(&["list", "colokan"]),
        Ok(Invocation::List(ListWhat::Mcp))
    );
    assert_eq!(
        parse(&["ls", "konfig"]),
        Ok(Invocation::List(ListWhat::Config))
    );
}

/// `sawit list keys` lists the key bindings under each of its names, `sawit --help` names
/// them all with an example, and `sawit ./keys` still opens a folder named keys.
#[test]
fn list_keys_has_its_names_and_its_help() {
    for word in ["keys", "key", "keymap", "shortcuts", "pintasan"] {
        assert_eq!(
            parse(&["list", word]),
            Ok(Invocation::List(ListWhat::Keys)),
            "{word}"
        );
    }
    assert_eq!(
        parse(&["ls", "pintasan"]),
        Ok(Invocation::List(ListWhat::Keys))
    );
    assert!(matches!(parse(&["./keys"]), Ok(Invocation::Open { .. })));
    assert!(matches!(parse(&["keys"]), Ok(Invocation::Open { .. })));
    for said in [
        "sawit list [instances|db|ports|connections|tools|mcp|skills|config|keys]",
        "sawit list keys ",
        "Sawit must be running",
        "key, keymap, shortcuts, pintasan",
    ] {
        assert!(USAGE.contains(said), "the help doesn't say {said:?}");
    }
}

#[test]
fn job_subcommands() {
    let Ok(Invocation::DbJob(job)) = parse(&["export", "kyzn", "leads", "leads.csv"]) else {
        panic!()
    };
    assert_eq!(job.kind, crate::kebun::DbJobKind::Export);
    assert_eq!(job.path, PathBuf::from("/nope/leads.csv"));
    assert!(!job.yes);
    let Ok(Invocation::DbJob(job)) = parse(&["dump", "kyzn", "k.dump", "-y"]) else {
        panic!()
    };
    assert_eq!(job.kind, crate::kebun::DbJobKind::Backup);
    assert!(job.yes);
    assert!(matches!(
        parse(&["impor", "kyzn", "leads", "a.csv"]),
        Ok(Invocation::DbJob(_))
    ));
    assert!(parse(&["panen", "kyzn"]).is_err());
    assert!(parse(&["tanam"]).is_err());
    let id = crate::dev_tools::TOOLS[0].id;
    let Ok(Invocation::Pasang(p)) = parse(&["install", id]) else {
        panic!()
    };
    assert_eq!(p.tools, [id]);
    assert!(parse(&["pasang"]).is_err());
    assert!(parse(&["pasang", "nope-tool"]).is_err());
    // The escape still opens folders with these names.
    assert!(matches!(parse(&["./panen"]), Ok(Invocation::Open { .. })));
    assert!(matches!(parse(&["./pasang"]), Ok(Invocation::Open { .. })));
    let Ok(Invocation::Pupuk(p)) = parse(&["upgrade", id, "-y"]) else {
        panic!()
    };
    assert_eq!(p.tools, [id]);
    assert!(p.yes);
    assert!(matches!(parse(&["pupuk"]), Ok(Invocation::Pupuk(_))));
    assert!(parse(&["pupuk", "nope-tool"]).is_err());
    assert!(matches!(parse(&["./pupuk"]), Ok(Invocation::Open { .. })));
    let Ok(Invocation::Cabut(c)) = parse(&["uninstall", id, "-y"]) else {
        panic!()
    };
    assert_eq!(c.tools, [id]);
    assert!(c.yes);
    let Ok(Invocation::Cabut(c)) = parse(&["cabut", id]) else {
        panic!()
    };
    assert!(!c.yes);
    assert!(parse(&["cabut"]).is_err());
    assert!(parse(&["cabut", "nope-tool"]).is_err());
    assert!(parse(&["cabut", id, "--force"]).is_err());
    assert!(matches!(parse(&["./cabut"]), Ok(Invocation::Open { .. })));
    assert!(matches!(
        parse(&["./uninstall"]),
        Ok(Invocation::Open { .. })
    ));
}

/// `sawit nyalakan` (`start`) starts instances; `./start` is still a folder.
#[test]
fn nyalakan_starts_instances() {
    assert_eq!(
        parse(&["start"]),
        Ok(Invocation::Nyalakan(NyalakanArgs::default()))
    );
    let Ok(Invocation::Nyalakan(args)) = parse(&["nyalakan", "kyzn", "--ulang", "-y"]) else {
        panic!()
    };
    assert_eq!(args.names, ["kyzn"]);
    assert!(args.again && args.yes);
    assert!(parse(&["start", "--nope"]).is_err());
    assert!(matches!(parse(&["./start"]), Ok(Invocation::Open { .. })));
    assert!(matches!(
        parse(&["./nyalakan"]),
        Ok(Invocation::Open { .. })
    ));
}

/// `sawit new` (`semai`) opens the New Project page, or with flags prints or creates a
/// stack; `./new` and `./semai` are still folders, and the help says how.
#[test]
fn semai_sows_a_new_project() {
    assert_eq!(parse(&["new"]), Ok(Invocation::Semai(Box::default())));
    let Ok(Invocation::Semai(args)) = parse(&["semai", "shop", "--template", "t3", "--buat"])
    else {
        panic!()
    };
    assert_eq!(args.choices.name.as_deref(), Some("shop"));
    assert!(args.create);
    assert!(parse(&["new", "--orm", "sequelize"]).is_err());
    assert!(parse(&["new", "--nope"]).is_err());
    assert!(matches!(parse(&["./new"]), Ok(Invocation::Open { .. })));
    assert!(matches!(parse(&["./semai"]), Ok(Invocation::Open { .. })));
    for said in [
        "sawit new [name]",
        "sawit new shop --template t3",
        "--create",
        "new = semai",
    ] {
        assert!(USAGE.contains(said), "no {said:?} in the help");
    }
    assert!(crate::kebun::SUBCOMMANDS.contains(&"new"));
    assert!(crate::kebun::SUBCOMMANDS.contains(&"semai"));
}

/// `sawit browse` (`intip`) opens this project's dev server, a port or an address in Sawit's
/// browser, and so does a URL on its own. With anything else a URL is refused, where it was
/// opened as an empty file `http:/…` at the port's line.
#[test]
fn browse_opens_pages() {
    let address = |a: &str| Ok(Invocation::Browse(BrowseTarget::Address(a.into())));
    assert_eq!(
        parse(&["browse"]),
        Ok(Invocation::Browse(BrowseTarget::DevServer))
    );
    assert_eq!(parse(&["intip", "5173"]), address("http://localhost:5173"));
    assert_eq!(
        parse(&["browse", "example.com/a"]),
        address("example.com/a")
    );
    assert_eq!(
        parse(&["http://localhost:5173/x"]),
        address("http://localhost:5173/x")
    );
    assert_eq!(parse(&["HTTPS://x.dev"]), address("HTTPS://x.dev"));
    for bad in [
        &["browse", "0"][..],
        &["browse", "70000"],
        &["browse", ""],
        &["browse", "a.dev", "b.dev"],
        &["intip", "-y"],
        &["-n", "http://x.dev"],
        &["a.rs", "https://x.dev"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
    // Past `--` it is a path, and `./browse` is a folder.
    assert!(matches!(
        parse(&["--", "http://x.dev"]),
        Ok(Invocation::Open { .. })
    ));
    assert!(matches!(parse(&["./browse"]), Ok(Invocation::Open { .. })));
    assert!(matches!(parse(&["./intip"]), Ok(Invocation::Open { .. })));
}

/// `sawit github token` hands the sign-in over (`--header`, `--exec` with a program whose
/// path needn't be UTF-8); `sawit github` alone, or `./github`, is still a folder; `github` is
/// never a database; and the help says how, and what it exposes.
#[test]
fn github_token_hands_the_sign_in_over() {
    use crate::github_token::TokenArgs;
    assert_eq!(
        parse(&["github", "token", "--header"]),
        Ok(Invocation::GitHub(TokenArgs::Header))
    );
    let odd = OsString::from_vec(b"/x/\xffnpx".to_vec());
    let Ok(Invocation::GitHub(TokenArgs::Exec(program))) = parse_args(
        [
            OsString::from("github"),
            "token".into(),
            "--exec".into(),
            odd.clone(),
            "-y".into(),
        ],
        Path::new("/nope"),
    ) else {
        panic!()
    };
    assert_eq!(program, [odd, OsString::from("-y")]);
    assert!(parse(&["github", "token"]).is_err());
    assert!(parse(&["github", "token", "--exec"]).is_err());
    assert!(matches!(parse(&["github"]), Ok(Invocation::Open { .. })));
    assert!(matches!(
        parse(&["./github", "token"]),
        Ok(Invocation::Open { .. })
    ));
    assert!(crate::kebun::SUBCOMMANDS.contains(&"github"));
    for said in [
        "sawit github token --header",
        "--exec <program>",
        "headersHelper",
        "SAWIT_GITHUB_AUTHORIZATION",
        "prints nothing and exits 1",
        "gets the token",
    ] {
        assert!(USAGE.contains(said), "no {said:?} in the help");
    }
}

/// `list` and `bakar` have their names in both languages: `daftar` and `stop`. Neither is a
/// database, and `./stop` is still a folder.
#[test]
fn list_and_bakar_in_both_languages() {
    assert_eq!(
        parse(&["daftar", "ports"]),
        Ok(Invocation::List(ListWhat::Ports))
    );
    let Ok(Invocation::Bakar(bakar)) = parse(&["stop", "lahan", "3000", "-y"]) else {
        panic!()
    };
    assert_eq!(bakar.targets, ["3000"]);
    assert!(bakar.yes);
    assert!(parse(&["stop"]).is_err());
    assert!(matches!(parse(&["./daftar"]), Ok(Invocation::Open { .. })));
    assert!(matches!(parse(&["./stop"]), Ok(Invocation::Open { .. })));
}
