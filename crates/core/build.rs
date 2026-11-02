//! Builds the skills Sawit ships into it: every file of each folder in the repository's
//! `skills/` (one folder a skill: its SKILL.md and whatever else it holds, a `references/`
//! folder included), for `skills::bundled` to write out and `canvas_design_guide` to read. A
//! file added, changed or taken out there is picked up by the next build, with no list to keep.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    // crates/core, two folders down from the repository's root.
    let repository = manifest.ancestors().nth(2).expect("the repository's root");
    let root = repository.join("skills");
    println!("cargo:rerun-if-changed=build.rs");
    // A folder: Cargo looks at everything in it, so a file added anywhere under it counts too.
    println!("cargo:rerun-if-changed={}", root.display());
    // Its types are skills::bundled's.
    let mut code = String::from(
        "// Made by crates/core/build.rs from the repository's skills/ folder: each skill Sawit ships,\n\
         // with every file in its folder, by its path there.\n\
         const SKILLS: &[Skill] = &[\n",
    );
    for skill in entries(&root) {
        if !skill.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect(&skill, "", &mut files);
        writeln!(code, "    ({:?}, &[", name(&skill)).unwrap();
        for (path, file) in files {
            println!("cargo:rerun-if-changed={}", file.display());
            let file = file.to_str().expect("a skill file's path is UTF-8");
            writeln!(code, "        ({path:?}, include_bytes!({file:?})),").unwrap();
        }
        code.push_str("    ]),\n");
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("bundled_skills.rs"), code).expect("write bundled_skills.rs");
}

/// What `dir` holds, by name, without hidden files (`.DS_Store`). Its absence fails the build:
/// Sawit would ship without its skills.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let read =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{} can't be read: {e}", dir.display()));
    let mut paths: Vec<PathBuf> = read
        .map(|entry| entry.expect("a folder entry").path())
        .filter(|path| !name(path).starts_with('.'))
        .collect();
    paths.sort();
    paths
}

/// Every file under `dir` (the skill folder's `rel`), with its path in the skill's folder.
fn collect(dir: &Path, rel: &str, files: &mut Vec<(String, PathBuf)>) {
    for path in entries(dir) {
        let at = if rel.is_empty() {
            name(&path).to_string()
        } else {
            format!("{rel}/{}", name(&path))
        };
        if path.is_dir() {
            collect(&path, &at, files);
        } else if path.is_file() {
            files.push((at, path));
        }
    }
}

fn name(path: &Path) -> &str {
    let name = path.file_name().and_then(|name| name.to_str());
    name.unwrap_or_else(|| panic!("{} isn't a UTF-8 name", path.display()))
}
