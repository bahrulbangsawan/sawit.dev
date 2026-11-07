//! Builds the skills Sawit ships into it: every file of each folder in the repository's
//! `skills/` (one folder a skill: its SKILL.md and whatever else it holds, a `references/`
//! folder included), for `skills::bundled` to write out and `canvas_design_guide` to read. A
//! file added, changed or taken out there is picked up by the next build, with no list to keep.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The design standard's digest, made here from its SKILL.md (`skills::digest`).
#[path = "src/skills/digest.rs"]
mod digest;

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
    // The design standard's digest: a rule it names that SKILL.md lost fails the build.
    let skill = root.join(digest::SKILL).join("SKILL.md");
    let text = std::fs::read_to_string(&skill)
        .unwrap_or_else(|e| panic!("{} can't be read: {e}", skill.display()));
    let made = digest::digest(&text).unwrap_or_else(|e| panic!("the design digest: {e}"));
    std::fs::write(out.join("design_digest.md"), made).expect("write design_digest.md");
    // The big JS bundles, deflated: the binary carries about a third of their size, and
    // `embedded::inflate` gives them back when they are run (they are never held).
    for (source, name) in [
        ("crates/core/assets/pele/pele.global.js", "pele.js.deflate"),
        ("vendor/mesurer/bundle/mesurer.js", "mesurer.js.deflate"),
    ] {
        let source = repository.join(source);
        println!("cargo:rerun-if-changed={}", source.display());
        deflate(&source, &out.join(name));
    }
}

/// Writes `source` deflated (raw DEFLATE, the best level) to `to`.
fn deflate(source: &Path, to: &Path) {
    use std::io::Write as _;
    let bytes =
        std::fs::read(source).unwrap_or_else(|e| panic!("{} can't be read: {e}", source.display()));
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&bytes).expect("deflate");
    std::fs::write(to, encoder.finish().expect("deflate")).expect("write a deflated bundle");
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
