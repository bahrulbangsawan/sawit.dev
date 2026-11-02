//! One instruction file written whole and read back: made when it isn't there and deleted again
//! once it holds nothing else, copied into Sawit's data folder before Sawit first changes it,
//! nothing written when nothing changes, a link's file changed and the link kept, a newer
//! Sawit's block left alone, and damaged markers leaving the file as it was.

use std::path::Path;

use super::*;
use crate::instances::AppPaths;
use crate::sawit_skills::files::{self, Done, Holds};
use crate::sawit_skills::ledger::Ledger;

fn ledger(dir: &Path) -> (AppPaths, Ledger) {
    let paths = AppPaths {
        root: dir.join("data"),
    };
    let ledger = Ledger::load(&paths).unwrap();
    (paths, ledger)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// A file Sawit makes holds the block alone, reads as current, and goes away again on removal;
/// Sawit's list says it made it until then.
#[test]
fn a_file_sawit_made_is_deleted_once_empty() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, mut ledger) = ledger(dir.path());
    let file = dir.path().join("home/.codex/AGENTS.md");
    assert_eq!(files::holds(&file), Holds::Nothing);
    let made = files::put(&file, &mut ledger).unwrap();
    assert_eq!((made.done, made.backup), (Done::Created, None));
    assert_eq!(read(&file), ours());
    assert_eq!(files::holds(&file), Holds::Current);
    // Kept on disk: another run (or the app after a restart) knows Sawit made it.
    let mut again = Ledger::load(&paths).unwrap();
    assert!(again.created(&file));
    assert_eq!(files::put(&file, &mut again).unwrap().done, Done::Unchanged);
    assert_eq!(files::take(&file, &mut again).unwrap().done, Done::Deleted);
    assert!(!file.exists());
    assert!(!Ledger::load(&paths).unwrap().created(&file));
    assert_eq!(files::take(&file, &mut again).unwrap().done, Done::Absent);
}

/// The user's file: copied into Sawit's data folder before the first change (once: a later run
/// keeps the first copy), the block put at its top, an older block updated, nothing written when
/// it is current, and a removal gives back the file byte for byte, which stays.
#[test]
fn the_users_file_is_kept_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut ledger) = ledger(dir.path());
    let file = dir.path().join("CLAUDE.md");
    let mine = "# My rules\r\n\r\nUse tabs.\r\n";
    std::fs::write(&file, mine).unwrap();
    let first = files::put(&file, &mut ledger).unwrap();
    assert_eq!(first.done, Done::Added);
    let copy = first.backup.expect("a copy before the first change");
    assert!(copy.starts_with(dir.path().join("data/instruction-backups")));
    assert_eq!(read(&copy), mine);
    assert_eq!(
        read(&file),
        format!("{}\r\n{mine}", ours().replace('\n', "\r\n"))
    );

    let unchanged = files::put(&file, &mut ledger).unwrap();
    assert_eq!((unchanged.done, unchanged.backup), (Done::Unchanged, None));

    // Someone edits inside the block: it reads as older, and the next put puts it back.
    let edited = read(&file).replace("Prefer Sawit", "Maybe use Sawit");
    std::fs::write(&file, &edited).unwrap();
    assert_eq!(files::holds(&file), Holds::Older(Some(VERSION)));
    let updated = files::put(&file, &mut ledger).unwrap();
    assert_eq!((updated.done, updated.backup), (Done::Updated, None));

    let removed = files::take(&file, &mut ledger).unwrap();
    assert_eq!(removed.done, Done::Removed);
    assert_eq!(read(&file), mine);
    assert_eq!(read(&copy), mine);
}

/// A link (dotfiles kept in a repository) stays a link: the file it points at gets the block.
#[test]
fn a_link_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut ledger) = ledger(dir.path());
    let real = dir.path().join("dotfiles/CLAUDE.md");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "mine\n").unwrap();
    let link = dir.path().join("CLAUDE.md");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(files::put(&link, &mut ledger).unwrap().done, Done::Added);
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(read(&real), format!("{}\nmine\n", ours()));
    assert_eq!(files::take(&link, &mut ledger).unwrap().done, Done::Removed);
    assert_eq!(read(&real), "mine\n");
}

/// A newer Sawit's block stays as it is; damaged markers leave the file untouched and say why.
#[test]
fn a_newer_block_and_damaged_markers_stay() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut ledger) = ledger(dir.path());
    let file = dir.path().join("GEMINI.md");
    let newer = format!(
        "{}\n<!-- Sawit Skills v{}: newer -->\n{}\n",
        block::START,
        VERSION + 1,
        block::END
    );
    std::fs::write(&file, &newer).unwrap();
    assert_eq!(files::holds(&file), Holds::Newer(VERSION + 1));
    assert_eq!(
        files::put(&file, &mut ledger).unwrap().done,
        Done::Newer(VERSION + 1)
    );
    assert_eq!(read(&file), newer);

    let damaged = format!("mine\n{}\nhalf a block\n", block::START);
    std::fs::write(&file, &damaged).unwrap();
    assert!(matches!(files::holds(&file), Holds::Damaged(_)));
    let said = files::put(&file, &mut ledger).unwrap_err();
    assert!(
        said.contains("line 2") && said.contains("changes nothing"),
        "{said}"
    );
    assert!(files::take(&file, &mut ledger).is_err());
    assert_eq!(read(&file), damaged);
    // Nothing was changed, so nothing was copied.
    assert!(!dir.path().join("data/instruction-backups").exists());
}
