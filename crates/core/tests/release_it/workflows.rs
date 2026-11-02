//! The workflows: release.yml's Publish step against a stand-in `gh`, how every workflow keeps
//! its token, and NOTICE's list of git repositories.

use std::fs;
use std::process::{Command, Output};

use crate::{checkout, ok, script, stderr};

/// The shell script of the step `name` in `.github/workflows/release.yml`: its `run: |` block,
/// dedented. The workflow keeps it inline, so it runs for any tag, also one from before a script
/// file it could call.
fn workflow_step(name: &str) -> String {
    let workflow = fs::read_to_string(checkout().join(".github/workflows/release.yml")).unwrap();
    let mut lines = workflow.lines();
    lines
        .by_ref()
        .find(|line| line.trim() == format!("- name: {name}"))
        .unwrap_or_else(|| panic!("release.yml has no step {name:?}"));
    let run = lines
        .by_ref()
        .find(|line| line.trim() == "run: |")
        .expect("the step runs a script");
    let indent = run.len() - run.trim_start().len();
    let mut body = String::new();
    for line in lines {
        let blank = line.trim().is_empty();
        if !blank && line.len() - line.trim_start().len() <= indent {
            break;
        }
        body.push_str(line.get(indent + 2..).unwrap_or(""));
        body.push('\n');
    }
    body
}

/// release.yml's Publish step, for tag v0.0.8, run in a temporary folder that has the build's
/// zip and DMG and the notes, against a stand-in `gh` that records each call and keeps one
/// release in a file, there from the start when `exists`; what it printed and the calls.
fn publish(exists: bool) -> (Output, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (calls, release) = (root.join("calls"), root.join("release"));
    if exists {
        fs::write(&release, "v0.0.8").unwrap();
    }
    let gh = format!(
        r#"printf '%s\n' "$*" >>'{calls}'
case "$1 $2" in
"release view") [ -e '{release}' ] || {{ echo "release not found" >&2; exit 1; }} ;;
"release create")
    [ ! -e '{release}' ] || {{ echo "HTTP 422: Validation Failed (Release.tag_name already_exists)" >&2; exit 1; }}
    echo "$3" >'{release}' ;;
"release upload") [ -e '{release}' ] || {{ echo "release not found" >&2; exit 1; }} ;;
*) echo "unexpected: gh $*" >&2; exit 1 ;;
esac
"#,
        calls = calls.display(),
        release = release.display(),
    );
    fs::create_dir_all(root.join("bin")).unwrap();
    script(&root.join("bin/gh"), &gh);
    fs::create_dir_all(root.join("target/release")).unwrap();
    for file in [
        "target/release/Sawit-0.0.8-macos-arm64.zip",
        "target/release/Sawit.dmg",
        "notes.md",
    ] {
        fs::write(root.join(file), "").unwrap();
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let out = Command::new("sh")
        .args(["-euc", &workflow_step("Publish")])
        .current_dir(root)
        .env("PATH", format!("{}:{path}", root.join("bin").display()))
        .env("TAG", "v0.0.8")
        .output()
        .unwrap();
    (out, fs::read_to_string(calls).unwrap_or_default())
}

/// A tag's first run publishes its release, with its notes and the zip and DMG.
#[test]
fn publishing_a_tag_creates_its_release() {
    let (out, calls) = publish(false);
    ok(out);
    assert!(
        calls.contains(
            "release create v0.0.8 --title Sawit v0.0.8 --notes-file notes.md \
             target/release/Sawit-0.0.8-macos-arm64.zip target/release/Sawit.dmg\n"
        ),
        "{calls}"
    );
}

/// RELEASE-11: release.yml can be run by hand to rebuild a tag, but its publish step always
/// created the release, which GitHub refuses once the tag has one: a rebuild helped only after a
/// run that failed before publishing. Now a release that exists gets the new assets in place of
/// its old ones, and keeps its notes.
#[test]
fn a_rebuild_replaces_the_assets_of_a_release_that_exists() {
    let (out, calls) = publish(true);
    let said = stderr(&out);
    assert!(out.status.success(), "{said}\n{calls}");
    assert!(
        calls.contains(
            "release upload v0.0.8 --clobber \
             target/release/Sawit-0.0.8-macos-arm64.zip target/release/Sawit.dmg\n"
        ),
        "{calls}"
    );
    assert!(!calls.contains("release create"), "{calls}");
}

/// The step of a workflow whose `uses:` line is `lines[at]`: from its `- ` line to the next line
/// indented no deeper than that dash.
fn step_at(lines: &[&str], at: usize) -> String {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let start = (0..=at)
        .rev()
        .find(|&i| lines[i].trim_start().starts_with("- "))
        .expect("a step");
    let dash = indent(lines[start]);
    let end = (start + 1..lines.len())
        .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) <= dash)
        .unwrap_or(lines.len());
    lines[start..end].join("\n")
}

/// RELEASE-9: a workflow's token reaches the repository, and its jobs hold more: the release
/// job's token can write to the repository, and the site job deploys the `install.sh` users pipe
/// into `sh`. So each workflow says what its token may do rather than taking the repository's
/// default, pins each action it runs to a commit, since a tag can be moved to other code, and
/// has checkout leave no token in `.git/config`, where every later step could read it. The
/// site's workflow came later and had none of this.
#[test]
fn every_workflow_pins_its_actions_and_keeps_no_token_on_disk() {
    let mut checked = Vec::new();
    for entry in fs::read_dir(checkout().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            continue;
        }
        let workflow = fs::read_to_string(&path).unwrap();
        assert!(
            workflow
                .lines()
                .any(|line| line.starts_with("permissions:")),
            "{name} doesn't say what its token may do"
        );
        let lines: Vec<&str> = workflow.lines().collect();
        let mut checkouts = 0;
        for (at, line) in lines.iter().enumerate() {
            let Some(action) = line.trim().trim_start_matches("- ").strip_prefix("uses: ") else {
                continue;
            };
            if action.starts_with("./") {
                continue;
            }
            let commit = action.split_once('@').map_or("", |(_, at)| at);
            let commit = commit.split_whitespace().next().unwrap_or("");
            assert!(
                commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit()),
                "{name}: {action} isn't pinned to a commit"
            );
            if action.starts_with("actions/checkout@") {
                checkouts += 1;
                let step = step_at(&lines, at);
                assert!(
                    step.contains("persist-credentials: false"),
                    "{name}: checkout leaves the token on disk:\n{step}"
                );
            }
        }
        assert!(checkouts > 0, "{name} has no checkout step");
        checked.push(name);
    }
    for workflow in ["release.yml", "site.yml"] {
        assert!(checked.iter().any(|name| name == workflow), "{checked:?}");
    }
}

/// RELEASE-8: NOTICE goes into Sawit.app with the licence texts of what the binary carries. A
/// crate from a git repository brings code no crates.io release vouches for, so NOTICE names
/// each such repository: craft-libs, compiled in through FigCraft since the native canvas, was
/// missing.
#[test]
fn notice_names_every_git_repository_the_build_uses() {
    let lock = fs::read_to_string(checkout().join("Cargo.lock")).unwrap();
    let notice = fs::read_to_string(checkout().join("NOTICE")).unwrap();
    let mut repositories: Vec<&str> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("source = \"git+"))
        .filter_map(|url| url.split(['?', '#', '"']).next())
        .collect();
    repositories.sort_unstable();
    repositories.dedup();
    assert!(!repositories.is_empty(), "Cargo.lock has no git source");
    let missing: Vec<&str> = repositories
        .into_iter()
        .filter(|repository| !notice.contains(repository))
        .collect();
    assert!(missing.is_empty(), "NOTICE doesn't name {missing:?}");
}
