//! The skills Sawit ships: built in from the repository's skills/ folder whole, and written out
//! into its data folder as this Sawit has them, rewriting only what changed.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use super::*;

/// Every file under `dir`, by its path there, with its bytes; hidden files left out, as the build
/// leaves them out.
fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, rel: &str, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let at = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            if entry.path().is_dir() {
                walk(&entry.path(), &at, out);
            } else {
                out.insert(at, std::fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, "", &mut out);
    out
}

fn embedded(name: &str) -> BTreeMap<String, Vec<u8>> {
    let files = bundled::files(name).unwrap_or_else(|| panic!("{name} isn't built in"));
    files
        .iter()
        .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
        .collect()
}

/// Canvas Design is built in as its folder in the repository is, every file of it: the build
/// reads the folder, so a file added there (a references/ folder, say) needs no list changed.
#[test]
fn the_built_in_skill_is_its_folder_in_the_repository() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let folder = repository.join("skills/sawit-canvas-design");
    assert_eq!(embedded("sawit-canvas-design"), files_under(&folder));
    assert!(bundled::names().any(|name| name == "sawit-canvas-design"));
    for file in ["SKILL.md", "CREDITS.md"] {
        assert!(
            bundled::text("sawit-canvas-design", file).is_some(),
            "{file}"
        );
    }
    assert_eq!(bundled::files("sawit-palsu"), None);
}

/// Its SKILL.md opens with the frontmatter agents read to pick it: its name, and a description
/// that says to use it to design, build or review UI on the canvas. A plain YAML value parses as
/// written only without ": " or " #" in it, or the skill wouldn't load.
#[test]
fn the_skill_md_has_the_frontmatter_agents_pick_it_by() {
    let text = bundled::text("sawit-canvas-design", "SKILL.md").unwrap();
    let rest = text.strip_prefix("---\n").expect("frontmatter first");
    let (front, _) = rest.split_once("\n---\n").expect("the frontmatter's end");
    // Each field with the lines indented under it (a folded or literal block's); comments and
    // blank lines aside.
    let mut fields: Vec<(&str, String)> = Vec::new();
    for line in front.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        match (
            line.starts_with([' ', '\t']),
            line.split_once(':'),
            fields.last_mut(),
        ) {
            (true, _, Some((_, value))) => value.push_str(&format!(" {}", line.trim())),
            (false, Some((key, value)), _) => fields.push((key, value.trim().to_string())),
            _ => panic!("not YAML frontmatter: {line:?}"),
        }
    }
    let field = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(field("name").as_deref(), Some("sawit-canvas-design"));
    assert_eq!(bundled::skill_name(text), Some("sawit-canvas-design"));
    let description = field("description").expect("a description");
    let plain = !description.starts_with(['"', '\'', '>', '|']);
    if plain {
        assert!(
            !description.contains(": ") && !description.contains(" #"),
            "{description}"
        );
    }
    let lower = description.to_lowercase();
    for word in ["design", "build", "review", "canvas"] {
        assert!(lower.contains(word), "no {word:?} in {description}");
    }
}

/// Writing it out leaves the folder exactly as Sawit ships it: a second time rewrites nothing, a
/// changed file is put back, and what Sawit doesn't ship is taken out, links included, never
/// what a link points at.
#[test]
fn writing_out_keeps_the_folder_as_sawit_ships_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("bundled-skills");
    let folder = bundled::write_out("sawit-canvas-design", &root).unwrap();
    assert_eq!(folder, root.join("sawit-canvas-design"));
    assert_eq!(files_under(&folder), embedded("sawit-canvas-design"));

    // Again: nothing is written, so every file is the same one.
    let skill = folder.join("SKILL.md");
    let credits = folder.join("CREDITS.md");
    let inode = |path: &Path| std::fs::metadata(path).unwrap().ino();
    let (before, credits_before) = (inode(&skill), inode(&credits));
    bundled::write_out("sawit-canvas-design", &root).unwrap();
    assert_eq!(inode(&skill), before);

    // Changed, and things added: the file comes back, the rest goes.
    std::fs::write(&skill, "edited").unwrap();
    std::fs::write(folder.join("extra.md"), "x").unwrap();
    std::fs::create_dir_all(folder.join("references/old")).unwrap();
    std::fs::write(folder.join("references/old/notes.md"), "x").unwrap();
    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "kept").unwrap();
    std::os::unix::fs::symlink(&outside, folder.join("linked.md")).unwrap();
    bundled::write_out("sawit-canvas-design", &root).unwrap();
    assert_eq!(files_under(&folder), embedded("sawit-canvas-design"));
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "kept");
    assert_eq!(inode(&credits), credits_before, "an unchanged file stays");
    // The files it writes through are gone too: nothing is left beside the folder.
    let beside: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(beside, ["sawit-canvas-design"]);

    // A file that is a link to the same text is made a file of its own, and a link in the
    // folder's place is taken out, not written through.
    std::fs::remove_file(&skill).unwrap();
    std::fs::write(
        &outside,
        bundled::text("sawit-canvas-design", "SKILL.md").unwrap(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside, &skill).unwrap();
    bundled::write_out("sawit-canvas-design", &root).unwrap();
    assert!(std::fs::symlink_metadata(&skill).unwrap().is_file());
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &folder).unwrap();
    bundled::write_out("sawit-canvas-design", &root).unwrap();
    assert!(std::fs::symlink_metadata(&folder).unwrap().is_dir());
    assert_eq!(files_under(&folder), embedded("sawit-canvas-design"));
    assert!(files_under(&elsewhere).is_empty());

    let unknown = bundled::write_out("sawit-palsu", &root).unwrap_err();
    assert!(unknown.to_string().contains("sawit-palsu"), "{unknown}");
}

/// A folder of the written-out skill that is a link (to a folder holding the same files) is made
/// a folder of its own, at either depth: what it holds is written there, and nothing of the folder
/// it pointed at is removed or changed.
#[test]
fn writing_out_through_a_linked_folder_writes_a_folder_of_its_own() {
    const FILES: &[bundled::File] = &[
        ("SKILL.md", b"---\nname: sawit-test\n---\n"),
        ("references/deep/note.md", b"note"),
        ("references/guide.md", b"guide"),
    ];
    let shipped: BTreeMap<String, Vec<u8>> = FILES
        .iter()
        .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
        .collect();
    for linked in ["references", "references/deep"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("bundled-skills");
        let folder = bundled::write_files("sawit-test", FILES, &root).unwrap();
        // The folder's files, and one more, in a folder elsewhere that the link points at.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::rename(folder.join(linked), &elsewhere).unwrap();
        std::fs::write(elsewhere.join("theirs.md"), "theirs").unwrap();
        std::os::unix::fs::symlink(&elsewhere, folder.join(linked)).unwrap();
        let before = files_under(&elsewhere);

        bundled::write_files("sawit-test", FILES, &root).unwrap();
        for at in ["references", "references/deep"] {
            let meta = std::fs::symlink_metadata(folder.join(at)).unwrap();
            assert!(meta.is_dir(), "{linked}: {at} is a folder");
        }
        assert_eq!(files_under(&folder), shipped, "{linked}");
        assert_eq!(files_under(&elsewhere), before, "{linked}");
    }
}
