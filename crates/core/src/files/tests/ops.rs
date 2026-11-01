//! Creating, copying, moving and deleting, and free names for copies.

use std::fs;
use std::path::PathBuf;

use crate::files::ops::same_file;
use crate::files::{copy_entry, create, free_name, list_dir, move_entry, remove};

#[test]
fn free_name_counts_copies_before_the_extension() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    assert_eq!(free_name(p, "a.rs", false), p.join("a.rs"));
    fs::write(p.join("a.rs"), "").unwrap();
    assert_eq!(free_name(p, "a.rs", false), p.join("a copy.rs"));
    fs::write(p.join("a copy.rs"), "").unwrap();
    assert_eq!(free_name(p, "a.rs", false), p.join("a copy 2.rs"));
    fs::write(p.join(".env"), "").unwrap();
    assert_eq!(free_name(p, ".env", false), p.join(".env copy"));
    fs::create_dir(p.join("v1.2")).unwrap();
    assert_eq!(free_name(p, "v1.2", true), p.join("v1.2 copy"));
}

#[test]
fn copies_and_moves_folders_without_replacing() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    fs::create_dir_all(p.join("src/inner")).unwrap();
    fs::write(p.join("src/inner/x.txt"), "x").unwrap();
    copy_entry(&p.join("src"), &p.join("dup")).unwrap();
    assert_eq!(fs::read_to_string(p.join("dup/inner/x.txt")).unwrap(), "x");
    assert!(copy_entry(&p.join("src"), &p.join("dup")).is_err());
    assert!(copy_entry(&p.join("src"), &p.join("src/inner/again")).is_err());
    assert!(move_entry(&p.join("dup"), &p.join("src")).is_err());
    move_entry(&p.join("dup"), &p.join("moved")).unwrap();
    assert!(p.join("moved/inner/x.txt").exists() && !p.join("dup").exists());
    remove(&p.join("moved")).unwrap();
    assert!(!p.join("moved").exists());
}

/// Duplicate and Paste copy a folder's files, links and folders, and leave out the pipes and
/// sockets in it (FILES-12). Opening a pipe with no writer, to copy it, waited for ever; a
/// socket can't be opened, which failed the copy halfway and left a partial copy. A pipe or
/// socket on its own isn't copied: nothing is made, and the copy says why.
#[test]
fn copying_a_folder_leaves_its_pipes_and_sockets_out() {
    use std::os::unix::fs::OpenOptionsExt as _;

    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    fs::create_dir_all(src.join("run")).unwrap();
    fs::write(src.join("a.txt"), "a").unwrap();
    std::os::unix::fs::symlink("a.txt", src.join("link")).unwrap();
    let pipe = src.join("run/pipe");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success()
    );
    let _socket = std::os::unix::net::UnixListener::bind(src.join("run/app.sock")).unwrap();

    let copy = d.path().join("copy");
    let (tx, rx) = std::sync::mpsc::channel();
    let (from, to) = (src.clone(), copy.clone());
    std::thread::spawn(move || tx.send(copy_entry(&from, &to).map_err(|e| e.to_string())));
    let copied = rx.recv_timeout(std::time::Duration::from_secs(5));
    // A copy that opened the pipe waits for a writer: this one lets it go.
    let _ = fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&pipe);
    copied.expect("the copy waited on the pipe").unwrap();
    assert_eq!(fs::read_to_string(copy.join("a.txt")).unwrap(), "a");
    assert!(
        fs::symlink_metadata(copy.join("link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(copy.join("run").is_dir());
    assert!(fs::symlink_metadata(copy.join("run/pipe")).is_err());
    assert!(fs::symlink_metadata(copy.join("run/app.sock")).is_err());

    for name in ["pipe", "app.sock"] {
        let to = d.path().join(format!("{name} copy"));
        let refused = copy_entry(&src.join("run").join(name), &to).unwrap_err();
        assert!(refused.to_string().contains("can't be copied"), "{refused}");
        assert!(fs::symlink_metadata(&to).is_err(), "{name}");
    }
}

/// A folder copy that fails part way, here on a file it may not read, leaves nothing behind:
/// no half-made copy that nothing records and nothing can undo.
#[test]
fn a_folder_copy_that_fails_leaves_nothing_behind() {
    use std::os::unix::fs::PermissionsExt as _;

    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    fs::create_dir_all(src.join("inner")).unwrap();
    fs::write(src.join("inner/a.txt"), "a").unwrap();
    fs::write(src.join("inner/secret"), "s").unwrap();
    fs::set_permissions(src.join("inner/secret"), fs::Permissions::from_mode(0o000)).unwrap();
    let copy = d.path().join("copy");
    assert!(copy_entry(&src, &copy).is_err());
    assert!(fs::symlink_metadata(&copy).is_err());
    // Let the temp folder go.
    fs::set_permissions(src.join("inner/secret"), fs::Permissions::from_mode(0o600)).unwrap();
}

/// A rename that changes only the case of a name goes ahead (the volume holding the temp
/// folder ignores case, as macOS's does by default: one file has both names).
#[test]
fn a_rename_that_changes_only_the_case_goes_ahead() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    fs::write(p.join("README.md"), "text").unwrap();
    move_entry(&p.join("README.md"), &p.join("readme.md")).unwrap();
    let names: Vec<_> = list_dir(p).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["readme.md"]);
    assert_eq!(fs::read_to_string(p.join("readme.md")).unwrap(), "text");
}

/// Two names are one file on disk only when they lead to one inode: two files never are, two
/// hard links to one file are, and a name with nothing there is no file.
#[test]
fn same_file_is_one_inode() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    fs::write(p.join("a.txt"), "a").unwrap();
    fs::write(p.join("b.txt"), "b").unwrap();
    fs::hard_link(p.join("a.txt"), p.join("link.txt")).unwrap();
    assert!(same_file(&p.join("a.txt"), &p.join("a.txt")));
    assert!(!same_file(&p.join("a.txt"), &p.join("b.txt")));
    assert!(same_file(&p.join("a.txt"), &p.join("link.txt")));
    assert!(!same_file(&p.join("a.txt"), &p.join("missing.txt")));
    // Two names of one file that differ in more than case are still two entries: the
    // rename is refused rather than leaving both names as they were.
    assert!(move_entry(&p.join("a.txt"), &p.join("link.txt")).is_err());
    assert!(p.join("a.txt").exists() && p.join("link.txt").exists());
}

/// On a volume that tells case apart, `README.md` and `readme.md` are two files, and renaming
/// one to the other's name replaced it for good (FILES-4): the names matched but for case,
/// and that was taken for a rename of the case alone. Now it is refused, and both are kept;
/// a rename to a case of the name that nothing has still goes ahead.
///
/// Makes a sparse case-sensitive APFS disk image (about 8 MB) in a temp folder and mounts it
/// there (`hdiutil`, no admin password):
/// `cargo test -p sawit-core -- --ignored case_sensitive`.
#[test]
#[ignore = "makes and mounts a disk image"]
fn renaming_to_another_files_name_in_other_case_replaces_nothing_on_a_case_sensitive_volume() {
    struct Mounted(PathBuf);
    impl Drop for Mounted {
        fn drop(&mut self) {
            let _ = std::process::Command::new("hdiutil")
                .args(["detach", "-force", "-quiet"])
                .arg(&self.0)
                .status();
        }
    }
    let hdiutil = |args: &[&std::ffi::OsStr]| {
        let status = std::process::Command::new("hdiutil")
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "hdiutil {args:?}");
    };
    let d = tempfile::tempdir().unwrap();
    let image = d.path().join("case.sparseimage");
    let mount = d.path().join("volume");
    fs::create_dir(&mount).unwrap();
    hdiutil(&[
        "create".as_ref(),
        "-size".as_ref(),
        "40m".as_ref(),
        "-fs".as_ref(),
        "Case-sensitive APFS".as_ref(),
        "-volname".as_ref(),
        "sawit-case".as_ref(),
        "-type".as_ref(),
        "SPARSE".as_ref(),
        "-quiet".as_ref(),
        d.path().join("case").as_os_str(),
    ]);
    hdiutil(&[
        "attach".as_ref(),
        "-nobrowse".as_ref(),
        "-noverify".as_ref(),
        "-noautoopen".as_ref(),
        "-quiet".as_ref(),
        "-mountpoint".as_ref(),
        mount.as_os_str(),
        image.as_os_str(),
    ]);
    let _mounted = Mounted(mount.clone());
    let p = mount.join("project");
    fs::create_dir(&p).unwrap();
    fs::write(p.join("README.md"), "upper").unwrap();
    fs::write(p.join("readme.md"), "lower").unwrap();

    let refused = move_entry(&p.join("README.md"), &p.join("readme.md"));
    assert_eq!(fs::read_to_string(p.join("readme.md")).unwrap(), "lower");
    assert_eq!(fs::read_to_string(p.join("README.md")).unwrap(), "upper");
    assert!(refused.unwrap_err().to_string().contains("already exists"));

    move_entry(&p.join("README.md"), &p.join("Readme.md")).unwrap();
    let mut names: Vec<_> = list_dir(&p).unwrap().into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, ["Readme.md", "readme.md"]);
}

#[test]
fn create_refuses_existing() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("new.txt");
    create(&f, false).unwrap();
    assert!(create(&f, false).is_err());
    create(&d.path().join("dir"), true).unwrap();
    assert!(d.path().join("dir").is_dir());
}
