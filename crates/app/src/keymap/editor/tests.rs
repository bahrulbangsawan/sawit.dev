//! The editor's helpers: a context read as tokens, and the keys keystroke search matches.

use super::render::tokenize_context;
use super::starts_with_keys;

#[test]
fn tokenizes_contexts() {
    assert_eq!(
        tokenize_context("AcpThread > Editor && !use_modifier_to_send"),
        [
            "AcpThread",
            ">",
            "Editor",
            "&&",
            "!",
            "use_modifier_to_send"
        ]
    );
    assert_eq!(
        tokenize_context("Editor && mode == full"),
        ["Editor", "&&", "mode", "==", "full"]
    );
}

#[test]
fn keystroke_search_matches_a_chords_start() {
    assert!(starts_with_keys("cmd-k e", "cmd-k"));
    assert!(starts_with_keys("cmd-k e", "cmd-k e"));
    assert!(!starts_with_keys("cmd-k", "cmd-k e"));
    assert!(!starts_with_keys("cmd-e", "cmd-k"));
}
