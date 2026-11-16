//! The connection store never writes over a profiles.toml it could not read.

use sawit_core::db::{ConnProfile, Profiles};

use super::DbStore;

#[test]
fn an_unreadable_profiles_file_is_never_saved_over() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("profiles.toml");
    std::fs::write(&file, "not = [toml").unwrap();
    let mut store = DbStore::with_profiles(file.clone(), Profiles::default());
    store.load_error = Some("profiles.toml: invalid".into());
    store
        .profiles
        .upsert(ConnProfile::new("h", 5432, "app", "u"));
    store.save();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "not = [toml");
    assert!(store.notice.is_some_and(|n| n.contains("profiles.toml")));
}
