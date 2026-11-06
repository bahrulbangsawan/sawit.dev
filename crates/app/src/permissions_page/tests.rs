//! The page plans a change, shows it, and saves only when Save is pressed.

use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::permissions::{Bucket, Change, read_rules};

use super::PermissionsPage;

/// Adding a rule to a project's file that isn't there yet: the sheet holds the plan (nothing
/// written); saving its plan creates the file with the rule.
#[gpui_kit::test]
fn a_change_waits_in_the_sheet_until_save(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("app");
    std::fs::create_dir_all(&project).unwrap();
    let file = project.join(".claude/settings.json");
    let page = cx.new(|cx| PermissionsPage::new(vec![project.clone()], cx));
    page.update(cx, |page, cx| {
        assert!(
            page.files
                .iter()
                .any(|f| f.path == file && !f.exists && f.editable)
        );
        page.propose(
            file.clone(),
            Change::Add {
                rule: "Bash(npm test)".into(),
                to: Bucket::Allow,
            },
            cx,
        );
        let pending = page.pending.as_ref().unwrap();
        assert!(
            pending.plan.diff.contains("+      \"Bash(npm test)\""),
            "{}",
            pending.plan.diff
        );
        assert!(pending.title.starts_with("Add Bash(npm test) to Allow"));
    });
    assert!(!file.exists(), "nothing is written before Save");
    page.update(cx, |page, cx| {
        let pending = page.pending.take().unwrap();
        sawit_core::permissions::save(&pending.plan).unwrap();
        page.reload();
        cx.notify();
    });
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(read_rules(&text).unwrap().allow, ["Bash(npm test)"]);
    // A refused rule says why and opens nothing.
    page.update(cx, |page, cx| {
        page.propose(
            file.clone(),
            Change::Add {
                rule: "Bash(export KEY=sk-live-abc)".into(),
                to: Bucket::Deny,
            },
            cx,
        );
        assert!(page.pending.is_none());
        assert!(
            page.message
                .as_ref()
                .is_some_and(|(ok, m)| !ok && m.contains("secret"))
        );
    });
}
