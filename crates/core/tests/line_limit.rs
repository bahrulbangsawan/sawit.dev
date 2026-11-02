//! Every hand-written source file in the repository stays at 500 lines or fewer. The check
//! itself is `scripts/check-line-limit.sh`, which CI runs too (`.github/workflows/line-limit.yml`).

use std::path::Path;
use std::process::Command;

#[test]
fn source_files_stay_within_the_line_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("sh")
        .arg("scripts/check-line-limit.sh")
        .current_dir(&root)
        .output()
        .expect("sh runs scripts/check-line-limit.sh");
    assert!(
        out.status.success(),
        "split these files to 500 lines or fewer:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
