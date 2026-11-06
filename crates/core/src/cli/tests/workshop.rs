//! `sawit permissions`, `sawit shell` and `sawit prompts`: their words in both languages, flags,
//! errors, and the folders they don't shadow.

use crate::kebun::{
    CangkangAction, CangkangArgs, IzinAction, IzinArgs, MantraAction, MantraArgs, SUBCOMMANDS,
};
use crate::permissions::Bucket;

use super::*;

fn parse_in(args: &[&str], cwd: &Path) -> std::result::Result<Invocation, String> {
    parse_args(args.iter().map(OsString::from), cwd)
}

#[test]
fn permissions_list_add_remove_and_move() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    let izin = |words: &[&str]| match parse_in(words, cwd) {
        Ok(Invocation::Izin(a)) => Ok(a),
        Ok(other) => panic!("{words:?}: {other:?}"),
        Err(e) => Err(e),
    };
    let list = izin(&["permissions"]).unwrap();
    assert_eq!(
        list,
        IzinArgs {
            action: IzinAction::List,
            file: "user".into(),
            project: cwd.to_path_buf(),
            yes: false
        }
    );
    assert_eq!(izin(&["izin", "lihat"]).unwrap().action, IzinAction::List);
    let add = izin(&["permissions", "add", "allow", "Bash(npm test)", "-y"]).unwrap();
    assert_eq!(
        add.action,
        IzinAction::Add(Bucket::Allow, "Bash(npm test)".into())
    );
    assert!(add.yes);
    let added = izin(&[
        "izin", "tambah", "tolak", "Read", "--berkas", "local", "--ya",
    ])
    .unwrap();
    assert_eq!(added.action, IzinAction::Add(Bucket::Deny, "Read".into()));
    assert_eq!((added.file.as_str(), added.yes), ("local", true));
    assert_eq!(
        izin(&["permissions", "move", "Read", "ask"])
            .unwrap()
            .action,
        IzinAction::Move("Read".into(), Bucket::Ask)
    );
    assert_eq!(
        izin(&["izin", "pindah", "Read", "izinkan"]).unwrap().action,
        IzinAction::Move("Read".into(), Bucket::Allow)
    );
    assert_eq!(
        izin(&["permissions", "rm", "Read"]).unwrap().action,
        IzinAction::Remove("Read".into())
    );
    std::fs::create_dir(cwd.join("shop")).unwrap();
    assert_eq!(
        std::fs::canonicalize(izin(&["permissions", "--project", "shop"]).unwrap().project)
            .unwrap(),
        std::fs::canonicalize(cwd.join("shop")).unwrap()
    );
    for bad in [
        &["permissions", "add", "maybe", "Read"][..],
        &["permissions", "add", "allow"],
        &["permissions", "remove"],
        &["permissions", "move", "Read"],
        &["permissions", "grant", "Read"],
        &["permissions", "--file"],
        &["permissions", "--project", "nope"],
        &["permissions", "--force"],
    ] {
        assert!(izin(bad).is_err(), "{bad:?}");
    }
    // `sawit ./permissions` is still a folder.
    assert!(matches!(
        parse_in(&["./permissions"], cwd),
        Ok(Invocation::Open { .. })
    ));
}

#[test]
fn shell_files_status_upload_and_download() {
    assert_eq!(
        CangkangArgs::parse(&[]).unwrap().unwrap().action,
        CangkangAction::Status
    );
    let Ok(Invocation::Cangkang(up)) = parse(&[
        "shell",
        "upload",
        "~/.zshrc",
        ".zprofile",
        "--overwrite",
        "-y",
    ]) else {
        panic!()
    };
    assert_eq!(up.action, CangkangAction::Upload);
    assert_eq!(up.files, [".zshrc", ".zprofile"]);
    assert!(up.overwrite && up.yes && !up.allow_secrets);
    let Ok(Invocation::Cangkang(down)) = parse(&["cangkang", "unduh", ".zshrc", "--timpa"]) else {
        panic!()
    };
    assert_eq!(
        (down.action, down.overwrite),
        (CangkangAction::Download, true)
    );
    let Ok(Invocation::Cangkang(s)) = parse(&["cangkang", "cek"]) else {
        panic!()
    };
    assert_eq!(s.action, CangkangAction::Status);
    assert!(parse(&["shell", "upload"]).is_err(), "names the files");
    assert!(parse(&["shell", "status", ".zshrc"]).is_err());
    assert!(parse(&["shell", "push", ".zshrc", "--everything"]).is_err());
    assert!(matches!(
        CangkangArgs::parse(&["unggah", ".zshrc", "--allow-secrets"]),
        Some(Ok(CangkangArgs {
            allow_secrets: true,
            ..
        }))
    ));
    // A word that isn't an action is a path.
    assert!(matches!(
        parse(&["shell", "scripts"]),
        Ok(Invocation::Open { .. })
    ));
}

#[test]
fn prompts_backup_and_restore() {
    let Ok(Invocation::Mantra(b)) = parse(&["prompts", "backup", "--repo", "my-prompts"]) else {
        panic!()
    };
    assert_eq!(
        b,
        MantraArgs {
            action: MantraAction::Backup,
            repo: Some("my-prompts".into()),
            overwrite: vec![],
            yes: false
        }
    );
    let Ok(Invocation::Mantra(r)) = parse(&[
        "mantra",
        "pulihkan",
        "--overwrite",
        "abc",
        "--timpa",
        "def",
        "-y",
    ]) else {
        panic!()
    };
    assert_eq!(r.action, MantraAction::Restore);
    assert_eq!(r.overwrite, ["abc", "def"]);
    assert!(r.yes);
    assert!(parse(&["prompts", "backup", "--repo"]).is_err());
    assert!(parse(&["prompts", "restore", "--all"]).is_err());
    assert!(matches!(parse(&["prompts"]), Ok(Invocation::Open { .. })));
    assert!(matches!(
        parse(&["prompts", "notes.md"]),
        Ok(Invocation::Open { .. })
    ));
}

#[test]
fn the_new_words_are_reserved_and_in_the_help() {
    for word in [
        "permissions",
        "izin",
        "shell",
        "cangkang",
        "prompts",
        "mantra",
    ] {
        assert!(SUBCOMMANDS.contains(&word), "{word}");
    }
    for said in [
        "sawit permissions add allow",
        "sawit shell upload .zshrc",
        "sawit prompts restore",
        "permissions = izin",
        "shell = cangkang",
        "prompts = mantra",
    ] {
        assert!(USAGE.contains(said), "no {said:?} in the help");
    }
}
