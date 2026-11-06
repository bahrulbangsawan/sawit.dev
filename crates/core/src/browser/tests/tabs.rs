//! The browser's tabs kept between runs, and how an agent names one.

use crate::browser::{
    SavedTab, SavedTabs, ViewportMode, find_tab, load_tabs, save_tabs, tabs_file,
};

#[test]
fn tabs_are_kept_in_the_projects_nyawit() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    std::fs::create_dir(&project).unwrap();
    let file = tabs_file(Some(&project), dir.path());
    assert_eq!(file, project.join(".nyawit").join("browser-tabs.json"));
    assert_eq!(load_tabs(&file), SavedTabs::default(), "no file: no tabs");
    let saved = SavedTabs {
        active: 1,
        tabs: vec![
            SavedTab {
                url: "http://localhost:5173/".into(),
                title: "Shop".into(),
                viewport: ViewportMode::Responsive,
                history: Some("AAEC".into()),
            },
            SavedTab {
                url: "http://localhost:5173/cart".into(),
                title: "Cart".into(),
                viewport: ViewportMode::Fixed {
                    name: Some("iPhone 15".into()),
                    width: 393,
                    height: 852,
                },
                history: None,
            },
        ],
    };
    save_tabs(&file, Some(&project), &saved).unwrap();
    assert!(project.join(".nyawit/.gitignore").is_file());
    assert_eq!(load_tabs(&file), saved);
    // A broken file starts afresh; an active index past the end comes back in.
    std::fs::write(&file, b"{not json").unwrap();
    assert_eq!(load_tabs(&file), SavedTabs::default());
    std::fs::write(
        &file,
        br#"{"active": 9, "tabs": [{"url": "a"}, {"url": ""}]}"#,
    )
    .unwrap();
    let loaded = load_tabs(&file);
    assert_eq!((loaded.active, loaded.tabs.len()), (0, 1));
    assert_eq!(
        tabs_file(None, dir.path()),
        dir.path().join("browser-tabs.json")
    );
}

#[test]
fn agents_name_a_tab_by_id_title_or_address() {
    let tabs = [
        (1, "Shop", "http://localhost:5173/"),
        (4, "Cart · Shop", "http://localhost:5173/cart"),
        (7, "Docs", "https://docs.example.com/"),
    ];
    assert_eq!(find_tab(&tabs, "4"), Some(1));
    assert_eq!(find_tab(&tabs, "#7"), Some(2));
    assert_eq!(find_tab(&tabs, "2"), None, "an id no tab has");
    assert_eq!(find_tab(&tabs, "shop"), Some(0), "a whole title first");
    assert_eq!(find_tab(&tabs, "cart"), Some(1));
    assert_eq!(find_tab(&tabs, "docs.example"), Some(2));
    assert_eq!(
        find_tab(&tabs, "5173"),
        Some(0),
        "a number no id has: a port"
    );
    assert_eq!(find_tab(&tabs, " "), None);
    assert_eq!(find_tab(&tabs, "nothing"), None);
}

/// An agent's op names its tab beside the op; without one it goes to the tab on show.
#[test]
fn a_browser_op_may_name_its_tab() {
    use crate::app_control::{Command, Request, decode};
    let op = |json: &str| match decode(format!("sawit-control/1\n{json}").as_bytes()) {
        Some(Ok(Request {
            command: Command::Browser { tab, .. },
            ..
        })) => tab,
        other => panic!("not a browser op: {other:?}"),
    };
    assert_eq!(
        op(r#"{"cmd":"browser","op":{"op":"snapshot"},"tab":"2"}"#).as_deref(),
        Some("2")
    );
    assert_eq!(op(r#"{"cmd":"browser","op":{"op":"snapshot"}}"#), None);
}
