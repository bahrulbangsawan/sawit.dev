//! The find bar's state and matching.

use super::*;

fn ranges(text: &str, query: &str, options: FindOptions) -> Vec<Range<usize>> {
    let re = matcher(query, options).unwrap().unwrap();
    find_all(text, &re)
}

#[test]
fn options_change_what_matches() {
    let text = "Find the finder; find(x) and FIND.";
    let plain = FindOptions::default();
    assert_eq!(ranges(text, "find", plain).len(), 4);
    let case = FindOptions {
        match_case: true,
        ..plain
    };
    assert_eq!(ranges(text, "find", case), vec![9..13, 17..21]);
    let word = FindOptions {
        whole_word: true,
        ..plain
    };
    assert_eq!(ranges(text, "find", word).len(), 3, "not inside “finder”");
    // A query is text unless Regex is on: its brackets are literal.
    assert_eq!(ranges(text, "find(x)", plain), vec![17..24]);
    let regex = FindOptions {
        regex: true,
        ..plain
    };
    assert_eq!(ranges(text, r"fi\w+er", regex), vec![9..15]);
    assert!(matcher("(", regex).is_err());
    assert!(matcher("", plain).unwrap().is_none());
    // An empty match is no match.
    assert!(ranges("abc", "x*", regex).is_empty());
}

#[test]
fn state_counts_and_wraps() {
    let mut state = FindState::found(3, None);
    assert_eq!(state.label("q"), "1 of 3");
    assert_eq!(state.next(), Some(1));
    assert_eq!(state.next(), Some(2));
    assert_eq!(state.next(), Some(0), "after the last, the first");
    assert_eq!(state.previous(), Some(2), "before the first, the last");
    assert_eq!(state.label("q"), "3 of 3");
    // New matches keep the current one where it can.
    assert_eq!(FindState::found(5, Some(2)).current, Some(2));
    assert_eq!(FindState::found(2, Some(4)).current, Some(0));
    let none = FindState::found(0, Some(1));
    assert_eq!(none.label("q"), "No results");
    assert_eq!(none.label(""), "");
    assert_eq!(first_from([3, 10, 20], 11), Some(2));
    assert_eq!(
        first_from([3, 10, 20], 21),
        Some(0),
        "past the last, the first"
    );
    assert_eq!(first_from(Vec::<usize>::new(), 0), None);
}
