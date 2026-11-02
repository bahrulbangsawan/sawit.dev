//! `scripts/install.sh`, run with nothing but stand-ins on its `PATH`: from Sawit's own terminal
//! it leaves Sawit running; from another, it quits Sawit first.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use crate::{checkout, script, stderr};

/// `scripts/install.sh`, run by `/bin/sh` with `TERM_PROGRAM` set to `term` while Sawit runs:
/// what it printed, and what it asked `osascript`, one call a line.
///
/// Its `PATH` holds only stand-ins and a few harmless tools, in a temporary folder: the OS
/// checks find an Apple Silicon Mac, `curl` serves a release and an empty zip, `ditto` unpacks a
/// Sawit.app, `pgrep` always finds Sawit running, `sleep` returns at once, and `osascript` records
/// what it is asked. Sawit never quits, and there is no `rm`, `mv` or `xattr` on that `PATH`, so
/// the script replaces no app: it gives up waiting for Sawit to quit.
fn run_installer(term: &str) -> (Output, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let asked = root.join("asked");
    let curl = r#"case "$*" in
*api.github.com*) echo '  "browser_download_url": "https://github.com/o/r/releases/download/v0.0.8/Sawit-0.0.8-macos-arm64.zip",' ;;
*) while [ $# -gt 0 ]; do [ "$1" != -o ] || exec /usr/bin/touch "$2"; shift; done; exit 1 ;;
esac
"#;
    let ditto = r#"app="$4/Sawit.app/Contents/MacOS"
/bin/mkdir -p "$app" && /usr/bin/touch "$app/sawit" && /bin/chmod +x "$app/sawit"
"#;
    let osascript = format!("printf '%s\\n' \"$*\" >>'{}'\n", asked.display());
    for (name, body) in [
        ("uname", "echo Darwin\n"),
        ("sysctl", "echo 1\n"),
        ("curl", curl),
        ("ditto", ditto),
        ("pgrep", "echo 4242\n"),
        ("sleep", "exit 0\n"),
        ("osascript", &osascript),
    ] {
        script(&bin.join(name), body);
    }
    for tool in [
        "/usr/bin/sed",
        "/usr/bin/head",
        "/usr/bin/mktemp",
        "/bin/mkdir",
    ] {
        let name = Path::new(tool).file_name().unwrap();
        std::os::unix::fs::symlink(tool, bin.join(name)).unwrap();
    }
    let out = Command::new("/bin/sh")
        .arg(checkout().join("scripts/install.sh"))
        .current_dir(&root)
        .env_clear()
        .env("PATH", &bin)
        .env("HOME", &root)
        .env("TMPDIR", &root)
        .env("TERM_PROGRAM", term)
        .output()
        .unwrap();
    (out, fs::read_to_string(asked).unwrap_or_default())
}

/// The install command, pasted into Sawit's own terminal, asked Sawit to quit so that it could
/// replace it. Quitting ends Sawit's terminals, and the script with them: nothing was installed,
/// and the terminal was gone. Sawit's terminals say whose they are (`TERM_PROGRAM=sawit`), so the
/// script now leaves Sawit running, before downloading anything, and says where to run it.
#[test]
fn the_installer_run_in_sawits_own_terminal_leaves_sawit_running() {
    let (out, asked) = run_installer("sawit");
    let said = stderr(&out);
    assert_eq!(asked, "", "it asked Sawit to quit: {said}");
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("another terminal"), "{said}");
    assert!(said.contains("Restart to Update"), "{said}");
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(!printed.contains("Downloading"), "{printed}");
}

/// From any other terminal, the install command still quits Sawit before it replaces it.
#[test]
fn the_installer_run_from_another_terminal_quits_sawit_first() {
    let (out, asked) = run_installer("Apple_Terminal");
    let (printed, said) = (String::from_utf8_lossy(&out.stdout), stderr(&out));
    assert!(
        asked.contains("quit app \"Sawit\""),
        "{asked:?}: {printed}{said}"
    );
    assert!(printed.contains("Quitting the running Sawit"), "{printed}");
    assert!(said.contains("Sawit is still running"), "{said}");
}
