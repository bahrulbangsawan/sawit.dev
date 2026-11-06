//! The tools that need `confirm` are the ones the Permissions page and `list_permissions` name.

use crate::Sawit;

#[test]
fn the_confirm_list_is_every_tool_with_confirm() {
    let mut with: Vec<String> = Sawit::router()
        .list_all()
        .iter()
        .filter(|t| {
            t.input_schema
                .get("properties")
                .and_then(|p| p.get("confirm"))
                .is_some()
        })
        .map(|t| t.name.to_string())
        .collect();
    with.sort();
    let mut listed: Vec<String> = sawit_core::permissions::CONFIRM_TOOLS
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    listed.sort();
    assert_eq!(
        listed, with,
        "update sawit_core::permissions::CONFIRM_TOOLS"
    );
}
