//! The release pipeline's scripts, run against throwaway repositories: `.githooks/pre-push`, which
//! makes each push of main a release, and `scripts/release.sh`, which cuts it. Each test gets a
//! clone of a small workspace, released at v0.0.7 and pushed to a bare `origin`, both in a
//! temporary folder, with the hook and the script as they are in this checkout. `cargo` and
//! `osascript` are stand-ins first on the `PATH`; git reads no global or system config. Also here:
//! release.yml's Publish step, run against a stand-in `gh` ([`workflows::publish`]); every
//! workflow, read for how it keeps its token; and `scripts/install.sh`, which installs a release,
//! run with nothing but stand-ins on its `PATH` ([`installer::run_installer`]).

mod installer;
mod push;
mod release_sh;
mod repo;
mod workflows;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

/// How long the release push the hook leaves running in the background gets to land.
const LANDS_WITHIN: Duration = Duration::from_secs(30);

/// This checkout's root, where the hook and the script under test are.
fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Writes an executable shell script.
fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Copies one of this checkout's scripts into `dir`, executable.
fn install(from: &str, dir: &Path) {
    let to = dir.join(from);
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(checkout().join(from), &to).unwrap();
    fs::set_permissions(&to, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The output's stdout, after checking the command succeeded.
fn ok(out: Output) -> String {
    assert!(
        out.status.success(),
        "failed ({}): {}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}
