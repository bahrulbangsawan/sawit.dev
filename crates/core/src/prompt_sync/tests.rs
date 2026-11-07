//! Prompts on GitHub: the Markdown files, the README, and a back up / delete / restore round trip
//! with GitHub stood in for by the files a backup would send.

use std::collections::BTreeMap;

use super::*;
use crate::github::{FileState, RemoteTree, SyncBase, blob_sha};
use crate::prompts::{Library, Prompt};

fn library() -> Library {
    let mut lib = Library::default();
    let mut review = Prompt::new(
        "Review: \"{{file}}\"",
        "Review {{ file }} for {{language}} bugs.\n\n---\nKeep {{language}} idioms.\n",
        &["code", "Review"],
    );
    review.favorite = true;
    lib.add(review, 100);
    lib.add(Prompt::new::<&str>("Plain", "No variables here.", &[]), 200);
    lib.mark_used(&lib.items[1].id.clone(), 300);
    lib
}

/// A prompt as Markdown: front matter GitHub shows as a table (name, tags, variables), the body
/// as written; read back, it is the same prompt (use counts stay on this Mac).
#[test]
fn a_prompt_round_trips_through_markdown() {
    let lib = library();
    let review = &lib.items[0];
    let text = to_markdown(review);
    assert!(text.starts_with("---\nid: \""));
    assert!(text.contains("\nname: \"Review: \\\"{{file}}\\\"\"\n"));
    assert!(text.contains("\ntags: [\"code\",\"review\"]\n"));
    assert!(text.contains("\nvariables: [\"file\",\"language\"]\n"));
    let back = from_markdown(&text).unwrap();
    assert_eq!(back, *review);
    assert_eq!(crate::prompts::variables(&back.body), ["file", "language"]);
    // A body with a `---` line of its own survives too.
    assert!(back.body.contains("\n---\nKeep"));
    assert!(from_markdown("no front matter").is_err());
    assert_eq!(file_path(review), format!("prompts/{}.md", review.id));
    let readme = readme(&lib);
    assert!(readme.contains(&format!("- [Plain](prompts/{}.md)", lib.items[1].id)));
    assert!(readme.contains("#code #review"));
}

/// Done when: back up, delete locally, restore, with the variables intact. GitHub is stood in
/// for by the files a backup sends and the tree they make.
#[test]
fn prompts_round_trip_back_up_delete_restore() {
    let mut lib = library();
    let files = library_files(&lib);
    let tree = RemoteTree {
        commit: "c1".into(),
        files: files
            .iter()
            .map(|(p, b)| (p.clone(), blob_sha(b)))
            .collect(),
    };
    // Backed up: the state records both prompts.
    let mut base = SyncBase::default();
    let synced: Vec<_> = tree
        .files
        .iter()
        .map(|(p, s)| (p.clone(), Some(s.clone())))
        .collect();
    base.record("c1", 1, &synced);
    assert!(
        plan_prompts(&lib, &tree, &base)
            .iter()
            .all(|p| p.state == FileState::Same)
    );

    let gone = lib.items[0].clone();
    assert!(lib.remove(&gone.id));
    // Deleted here since the last sync: a restore doesn't bring it back, a backup deletes it.
    let plan = plan_prompts(&lib, &tree, &base);
    let row = plan.iter().find(|p| p.id == gone.id).unwrap();
    assert_eq!(row.state, FileState::DeletedHere);
    assert!(row.state.uploads() && !row.state.downloads());
    // A Mac that never synced (a fresh install) restores it.
    let base = SyncBase::default();
    let plan = plan_prompts(&lib, &tree, &base);
    let restoring: Vec<_> = plan.iter().filter(|p| p.state.downloads()).collect();
    assert_eq!(restoring.len(), 1);
    assert_eq!(restoring[0].state, FileState::OnlyOnGitHub);
    let contents: BTreeMap<String, Vec<u8>> = files.clone().into_iter().collect();
    let (out, _) = apply_restore(&mut lib, &plan, &contents, &[]).unwrap();
    assert_eq!(out.synced_ids, std::slice::from_ref(&gone.id));
    let back = lib.get(&gone.id).unwrap();
    assert_eq!(back.body, gone.body);
    assert_eq!(crate::prompts::variables(&back.body), ["file", "language"]);
    assert_eq!(
        (back.title.as_str(), back.favorite),
        (gone.title.as_str(), true)
    );
    // The one here keeps its use count.
    assert_eq!(lib.get(&lib.items[0].id.clone()).unwrap().used_count, 1);
}

/// Changed on both sides: a restore lists it and leaves it, until Keep GitHub's names it; a
/// backup's plan never sends it either.
#[test]
fn a_prompt_changed_on_both_sides_is_shown_not_overwritten() {
    let mut lib = library();
    let id = lib.items[0].id.clone();
    let path = format!("prompts/{id}.md");
    let mut base = SyncBase::default();
    base.files.insert(
        path.clone(),
        blob_sha(to_markdown(&lib.items[0]).as_bytes()),
    );
    let mut theirs = lib.items[0].clone();
    theirs.body = "GitHub's {{file}}".into();
    let theirs_text = to_markdown(&theirs).into_bytes();
    let tree = RemoteTree {
        commit: "c2".into(),
        files: [(path.clone(), blob_sha(&theirs_text))].into(),
    };
    lib.update(&id, 500, |p| p.body = "Mine {{file}}".into());
    let plan = plan_prompts(&lib, &tree, &base);
    let row = plan.iter().find(|p| p.path == path).unwrap();
    assert_eq!(row.state, FileState::ChangedOnBoth);
    assert!(!row.state.uploads());
    let contents = BTreeMap::from([(path.clone(), theirs_text)]);
    let (out, _) = apply_restore(&mut lib, &plan, &contents, &[]).unwrap();
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(lib.get(&id).unwrap().body, "Mine {{file}}");
    let (out, _) = apply_restore(&mut lib, &plan, &contents, std::slice::from_ref(&id)).unwrap();
    assert!(out.conflicts.is_empty());
    assert_eq!(lib.get(&id).unwrap().body, "GitHub's {{file}}");
}

/// A prompt deleted on another Mac (gone from GitHub, unchanged here since the last sync) is
/// deleted here by a restore, and the sync forgets it; one changed here meanwhile is a conflict.
#[test]
fn a_delete_on_github_carries_over_on_restore() {
    let mut lib = library();
    let files = library_files(&lib);
    let mut base = SyncBase::default();
    let synced: Vec<_> = files
        .iter()
        .map(|(p, b)| (p.clone(), Some(blob_sha(b))))
        .collect();
    base.record("c1", 1, &synced);
    let (gone, kept) = (lib.items[0].id.clone(), lib.items[1].id.clone());
    let gone_path = format!("prompts/{gone}.md");
    let tree = RemoteTree {
        commit: "c2".into(),
        files: files
            .iter()
            .filter(|(p, _)| **p != gone_path)
            .map(|(p, b)| (p.clone(), blob_sha(b)))
            .collect(),
    };
    let plan = plan_prompts(&lib, &tree, &base);
    let row = plan.iter().find(|p| p.id == gone).unwrap();
    assert_eq!(row.state, FileState::DeletedOnGitHub);
    let (out, synced) = apply_restore(&mut lib, &plan, &BTreeMap::new(), &[]).unwrap();
    assert_eq!(out.deleted, std::slice::from_ref(&gone));
    assert!(lib.get(&gone).is_none() && lib.get(&kept).is_some());
    base.record("c2", 2, &synced);
    assert!(!base.files.contains_key(&gone_path), "the sync forgot it");

    // Changed here after the last sync, deleted on GitHub: neither wins on its own.
    let mut lib = library();
    let id = lib.items[0].id.clone();
    let mut base = SyncBase::default();
    base.record("c1", 1, &synced_all(&lib));
    lib.update(&id, 900, |p| p.body = "edited".into());
    let tree = RemoteTree {
        commit: "c2".into(),
        files: BTreeMap::new(),
    };
    let row = plan_prompts(&lib, &tree, &base)
        .into_iter()
        .find(|p| p.id == id)
        .unwrap();
    assert_eq!(row.state, FileState::ChangedOnBoth);
}

fn synced_all(lib: &Library) -> Vec<(String, Option<String>)> {
    library_files(lib)
        .iter()
        .map(|(p, b)| (p.clone(), Some(blob_sha(b))))
        .collect()
}

#[test]
fn settings_default_to_a_private_sawit_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(SETTINGS);
    assert_eq!(BackupSettings::load(&path).repo, "sawit-prompts");
    let s = BackupSettings {
        repo: "my-prompts".into(),
        auto: true,
    };
    s.save(&path).unwrap();
    assert_eq!(BackupSettings::load(&path), s);
}
