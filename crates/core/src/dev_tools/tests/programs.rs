//! Finding programs where installers put them.

use std::path::Path;

use crate::dev_tools::programs::{INSTALL_DIRS, PRELUDE, SYSTEM_DIRS, find_program_in};

/// The MCP page looks programs up where the Dev Tools page finds tools,
/// not only on the app's `PATH`, which misses what `~/.zshrc` adds.
#[test]
fn programs_are_found_where_installers_put_them() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let program = |path: &Path, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    program(&home.join(".local/bin/claude"), "");
    let nvm = home.join(".nvm");
    for v in ["v18.20.4", "v22.11.0", "v23.1.0"] {
        program(&nvm.join(format!("versions/node/{v}/bin/npx")), "");
    }
    std::fs::create_dir_all(nvm.join("alias/lts")).unwrap();
    std::fs::write(nvm.join("alias/default"), "lts/*\n").unwrap();
    std::fs::write(nvm.join("alias/lts/*"), "lts/jod\n").unwrap();
    std::fs::write(nvm.join("alias/lts/jod"), "v22.11.0\n").unwrap();
    // Capistrano's `cap`, a Ruby script: not Cap's.
    program(&home.join(".local/bin/cap"), "#!/usr/bin/env ruby\n");
    let empty = std::ffi::OsString::new();
    let find = |name| find_program_in(name, Some(&empty), home, &nvm, &[]);
    assert_eq!(find("claude"), Some(home.join(".local/bin/claude")));
    assert_eq!(
        find("npx"),
        Some(nvm.join("versions/node/v22.11.0/bin/npx"))
    );
    assert_eq!(find("cap"), None);
    assert_eq!(find("codex"), None);
    // The default named by number, and an nvm with no default: the newest.
    std::fs::write(nvm.join("alias/default"), "18").unwrap();
    assert_eq!(
        find("npx"),
        Some(nvm.join("versions/node/v18.20.4/bin/npx"))
    );
    std::fs::remove_file(nvm.join("alias/default")).unwrap();
    assert_eq!(find("npx"), Some(nvm.join("versions/node/v23.1.0/bin/npx")));
    // PATH comes first.
    program(&home.join("bin/claude"), "");
    let path = std::env::join_paths([home.join("bin")]).unwrap();
    assert_eq!(
        find_program_in("claude", Some(&path), home, &nvm, &[]),
        Some(home.join("bin/claude"))
    );
    // Every folder searched is one detection searches too.
    for d in INSTALL_DIRS.iter().chain(&SYSTEM_DIRS) {
        let d = d.replace("~/", "$HOME/");
        assert!(PRELUDE.contains(&format!("{d}:")), "{d}");
    }
}
