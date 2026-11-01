//! The words each subcommand takes, and which arguments name a database.

use std::path::{Path, PathBuf};

use super::*;

#[test]
fn query_shapes() {
    assert_eq!(q(&["kyzn"]), qa("kyzn", None, None));
    assert_eq!(q(&["kyzn", "leads"]), qa("kyzn", Some("leads"), None));
    assert_eq!(
        q(&["kyzn", "leads", "select * from leads where phone is null"]),
        qa(
            "kyzn",
            Some("leads"),
            Some("select * from leads where phone is null")
        )
    );
    assert_eq!(q(&["kyzn", "select 1"]), qa("kyzn", None, Some("select 1")));
    assert_eq!(
        q(&["kyzn", "crm.leads"]),
        qa("kyzn", Some("crm.leads"), None)
    );
}

#[test]
fn not_query_shapes() {
    assert_eq!(q(&[]), None);
    assert_eq!(q(&["-n", "kyzn"]), None);
    assert_eq!(q(&["src/main.rs"]), None);
    assert_eq!(q(&["main.rs"]), None);
    assert_eq!(q(&["a", "b", "c", "d"]), None);
    assert_eq!(q(&["kyzn", "a b", "select 1"]), None);
    // An existing folder is opened, not queried.
    let args = [OsString::from("tmp")];
    assert_eq!(query_args(&args, Path::new("/")), None);
}

/// `-y` (`--yes`, `--ya`) anywhere runs SQL that writes without asking; it isn't a table,
/// and with it a name that is no database is said to be none, not opened as a file.
#[test]
fn a_query_takes_yes_anywhere() {
    let yes = |db: &str, table: Option<&str>, sql: Option<&str>| {
        qa(db, table, sql).map(|q| QueryArgs { yes: true, ..q })
    };
    assert_eq!(
        q(&["kyzn", "delete from leads", "-y"]),
        yes("kyzn", None, Some("delete from leads"))
    );
    assert_eq!(
        q(&["--yes", "kyzn", "leads", "id = 3"]),
        yes("kyzn", Some("leads"), Some("id = 3"))
    );
    assert_eq!(q(&["kyzn", "--ya"]), yes("kyzn", None, None));
    assert!(q(&["kyzn", "-y"]).unwrap().explicit());
    // Other options are still not a query's.
    assert_eq!(q(&["-y"]), None);
    assert_eq!(q(&["kyzn", "-n"]), None);
    assert_eq!(q(&["kyzn", "-y", "-r"]), None);
}

#[test]
fn subcommands_are_not_databases() {
    assert_eq!(q(&["list"]), None);
    assert_eq!(q(&["bakar", "ports"]), None);
    assert_eq!(q(&["ls", "db"]), None);
    assert_eq!(q(&["daftar"]), None);
    assert_eq!(q(&["stop", "ports"]), None);
}

#[test]
fn list_words() {
    assert_eq!(ListWhat::parse(None), Ok(ListWhat::All));
    assert_eq!(ListWhat::parse(Some("db")), Ok(ListWhat::Databases));
    assert_eq!(ListWhat::parse(Some("lahan")), Ok(ListWhat::Ports));
    assert_eq!(ListWhat::parse(Some("kebun")), Ok(ListWhat::Instances));
    assert_eq!(ListWhat::parse(Some("konsesi")), Ok(ListWhat::Connections));
    assert_eq!(ListWhat::parse(Some("explorer")), Ok(ListWhat::Explorer));
    assert_eq!(ListWhat::parse(Some("jelajah")), Ok(ListWhat::Explorer));
    // Asking every dev server takes a moment: only when asked.
    assert!(!ListWhat::All.shows(ListWhat::Explorer));
    assert!(ListWhat::parse(Some("hutan")).is_err());
}

#[test]
fn bakar_words() {
    assert_eq!(
        BakarArgs::parse(&["ports", "3000", "-y"]),
        Ok(BakarArgs {
            what: BakarWhat::Ports,
            targets: vec!["3000".into()],
            yes: true
        })
    );
    assert_eq!(
        BakarArgs::parse(&["instances"]),
        Ok(BakarArgs {
            what: BakarWhat::Instances,
            targets: vec![],
            yes: false
        })
    );
    assert!(BakarArgs::parse(&[]).is_err());
    assert!(BakarArgs::parse(&["hutan"]).is_err());
    assert!(BakarArgs::parse(&["ports", "dev"]).is_err());
    assert!(BakarArgs::parse(&["ports", "--force"]).is_err());
}

#[test]
fn list_tools_only_when_asked() {
    assert_eq!(ListWhat::parse(Some("perkakas")), Ok(ListWhat::Tools));
    assert_eq!(ListWhat::parse(Some("tools")), Ok(ListWhat::Tools));
    assert!(ListWhat::Tools.shows(ListWhat::Tools));
    assert!(!ListWhat::All.shows(ListWhat::Tools));
    assert!(ListWhat::All.shows(ListWhat::Ports));
}

/// `sawit list keys` and its aliases list the key bindings, and a bare `sawit list` leaves
/// them out: they need the running app.
#[test]
fn list_keys_only_when_asked() {
    for word in ["keys", "key", "keymap", "shortcuts", "pintasan"] {
        assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Keys), "{word}");
    }
    assert!(ListWhat::Keys.shows(ListWhat::Keys));
    assert!(!ListWhat::Keys.shows(ListWhat::Ports));
    assert!(!ListWhat::All.shows(ListWhat::Keys));
    let unknown = ListWhat::parse(Some("tombol")).unwrap_err();
    assert!(unknown.contains("keys"), "{unknown}");
}

#[test]
fn list_mcp_and_config_only_when_asked() {
    for word in ["mcp", "colokan"] {
        assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Mcp), "{word}");
    }
    for word in ["config", "configs", "konfig"] {
        assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Config), "{word}");
    }
    for what in [ListWhat::Mcp, ListWhat::Config] {
        assert!(what.shows(what));
        assert!(!what.shows(ListWhat::Ports), "{what:?}");
        assert!(!ListWhat::All.shows(what), "{what:?}");
    }
    let unknown = ListWhat::parse(Some("hutan")).unwrap_err();
    assert!(unknown.contains("mcp, skills, config"), "{unknown}");
}

#[test]
fn list_skills_only_when_asked() {
    for word in ["skills", "skill", "bibit"] {
        assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Skills), "{word}");
    }
    assert!(ListWhat::Skills.shows(ListWhat::Skills));
    assert!(!ListWhat::All.shows(ListWhat::Skills));
    assert!(!ListWhat::Skills.shows(ListWhat::Mcp));
}

#[test]
fn db_job_words() {
    let cwd = Path::new("/w");
    assert_eq!(DbJob::kind("panen"), Some(DbJobKind::Export));
    assert_eq!(DbJob::kind("dump"), Some(DbJobKind::Backup));
    assert_eq!(DbJob::kind("tanam"), Some(DbJobKind::Import));
    assert_eq!(DbJob::kind("hutan"), None);
    assert_eq!(
        DbJob::parse(DbJobKind::Export, &["kyzn", "leads", "out.xlsx", "-y"], cwd),
        Ok(DbJob {
            kind: DbJobKind::Export,
            db: "kyzn".into(),
            object: Some("leads".into()),
            path: PathBuf::from("/w/out.xlsx"),
            yes: true,
        })
    );
    assert!(DbJob::parse(DbJobKind::Export, &["kyzn", "leads", "out.json"], cwd).is_err());
    assert!(DbJob::parse(DbJobKind::Export, &["kyzn", "out.csv"], cwd).is_err());
    let backup = DbJob::parse(DbJobKind::Backup, &["kyzn", "/b/kyzn.dump"], cwd).unwrap();
    assert_eq!(backup.object, None);
    assert_eq!(backup.path, PathBuf::from("/b/kyzn.dump"));
    let table = DbJob::parse(DbJobKind::Backup, &["kyzn", "crm.leads", "x.sql"], cwd).unwrap();
    assert_eq!(table.object.as_deref(), Some("crm.leads"));
    assert!(DbJob::parse(DbJobKind::Import, &["kyzn", "leads.csv"], cwd).is_err());
    assert!(
        DbJob::parse(
            DbJobKind::Import,
            &["kyzn", "leads", "a.csv", "--force"],
            cwd
        )
        .is_err()
    );
}

#[test]
fn pasang_words() {
    let id = crate::dev_tools::TOOLS[0].id;
    assert_eq!(
        PasangArgs::parse(&[id, "-y"]),
        Ok(PasangArgs {
            tools: vec![id.into()],
            yes: true
        })
    );
    assert!(PasangArgs::parse(&[]).is_err());
    assert!(PasangArgs::parse(&["-y"]).is_err());
    assert!(PasangArgs::parse(&["sawit-palsu"]).is_err());
    assert!(PasangArgs::parse(&[id, "--force"]).is_err());
}

#[test]
fn pupuk_words() {
    let id = crate::dev_tools::TOOLS[0].id;
    assert_eq!(
        PupukArgs::parse(&[]),
        Ok(PupukArgs {
            tools: vec![],
            yes: false
        })
    );
    assert_eq!(
        PupukArgs::parse(&["--ya", id]),
        Ok(PupukArgs {
            tools: vec![id.into()],
            yes: true
        })
    );
    assert!(PupukArgs::parse(&["sawit-palsu"]).is_err());
    assert!(PupukArgs::parse(&["--force"]).is_err());
}

#[test]
fn cabut_words() {
    let id = crate::dev_tools::TOOLS[0].id;
    assert_eq!(
        CabutArgs::parse(&[id, "-y"]),
        Ok(CabutArgs {
            tools: vec![id.into()],
            yes: true
        })
    );
    assert_eq!(
        CabutArgs::parse(&["--ya", id, "bun"]),
        Ok(CabutArgs {
            tools: vec![id.into(), "bun".into()],
            yes: true
        })
    );
    assert_eq!(CabutArgs::parse(&["bun"]).map(|a| a.yes), Ok(false));
    // Something to uproot must be named: no "every tool".
    assert!(CabutArgs::parse(&[]).is_err());
    assert!(CabutArgs::parse(&["-y"]).is_err());
    assert!(CabutArgs::parse(&["sawit-palsu"]).is_err());
    assert!(CabutArgs::parse(&[id, "--force"]).is_err());
}

#[test]
fn nyalakan_words() {
    assert_eq!(NyalakanArgs::parse(&[]), Ok(NyalakanArgs::default()));
    assert_eq!(
        NyalakanArgs::parse(&["kyzn", "--ulang", "shop", "-y", "kyzn"]),
        Ok(NyalakanArgs {
            names: vec!["kyzn".into(), "shop".into()],
            again: true,
            yes: true,
        })
    );
    assert_eq!(
        NyalakanArgs::parse(&["--restart", "--ya"]),
        Ok(NyalakanArgs {
            names: vec![],
            again: true,
            yes: true,
        })
    );
    assert!(NyalakanArgs::parse(&["kyzn", "--force"]).is_err());
    assert!(NyalakanArgs::parse(&["-u"]).is_err());
    // Its words are never databases.
    assert_eq!(q(&["nyalakan", "kyzn"]), None);
    assert_eq!(q(&["start", "kyzn"]), None);
    assert_eq!(q(&["nyalakan"]), None);
}

#[test]
fn job_words_are_not_databases() {
    assert_eq!(q(&["panen", "kyzn", "a.csv"]), None);
    assert_eq!(q(&["backup", "kyzn"]), None);
    assert_eq!(q(&["pasang", "bun"]), None);
    assert_eq!(q(&["pupuk", "bun"]), None);
    assert_eq!(q(&["cabut", "bun"]), None);
    assert_eq!(q(&["uninstall", "bun"]), None);
}
