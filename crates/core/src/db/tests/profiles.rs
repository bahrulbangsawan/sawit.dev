//! Saved profiles, their passwords (Keychain, `.pgpass`) and `.env` files.

use super::*;

#[test]
fn certificate_paths_survive_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("profiles.toml");
    let (p, _) =
        parse_database_url("postgres://u@h/d?sslmode=verify-full&sslrootcert=/ca.pem").unwrap();
    let profiles = Profiles {
        items: vec![p.clone()],
    };
    profiles.save(&file).unwrap();
    assert_eq!(Profiles::load(&file).unwrap().items, vec![p]);
}

#[test]
fn profiles_saved_before_sslmode_still_load() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("profiles.toml");
    std::fs::write(
        &file,
        "[[profile]]\nname = \"x\"\nhost = \"h\"\nport = 5432\ndatabase = \"d\"\nuser = \"u\"\n",
    )
    .unwrap();
    assert_eq!(Profiles::load(&file).unwrap().items[0].ssl, SslMode::Prefer);
}

#[test]
fn dotenv_finds_database_url_variants() {
    let env = "# comment\nNODE_ENV=dev\nexport DATABASE_URL=\"postgres://a:b@localhost:5440/app\" # trailing\nOTHER='x'\n";
    assert_eq!(
        dotenv_value(env, "DATABASE_URL").as_deref(),
        Some("postgres://a:b@localhost:5440/app")
    );
    assert_eq!(
        dotenv_value("DATABASE_URL='postgres://q@h/d'\n", "DATABASE_URL").as_deref(),
        Some("postgres://q@h/d")
    );
    assert_eq!(
        dotenv_value("DATABASE_URL=postgres://q@h/d\n", "DATABASE_URL").as_deref(),
        Some("postgres://q@h/d")
    );
    assert_eq!(
        dotenv_value("#DATABASE_URL=postgres://q@h/d\n", "DATABASE_URL"),
        None
    );
}

/// A `.env` that can't be read is left as it is: read as nothing, writing DATABASE_URL replaced
/// the whole file with that one line, and every other key in it was lost. One that isn't there is
/// made.
#[test]
fn a_dotenv_that_cant_be_read_keeps_its_keys() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(".env");
    let url = "postgres://postgres@localhost:5440/app";
    let theirs = b"API_KEY=abc\nNAME=caf\xe9\n";
    std::fs::write(&env, theirs).unwrap();
    let refused = write_database_url(dir.path(), url).unwrap_err();
    assert!(refused.contains("can't be read"), "{refused}");
    assert_eq!(std::fs::read(&env).unwrap(), theirs);
    assert!(read_dotenv(dir.path()).is_err());

    std::fs::remove_file(&env).unwrap();
    assert_eq!(read_dotenv(dir.path()), Ok(String::new()));
    write_database_url(dir.path(), url).unwrap();
    assert_eq!(
        std::fs::read_to_string(&env).unwrap(),
        format!("DATABASE_URL={url}\n")
    );
}

#[test]
fn dotenv_upsert_replaces_or_appends() {
    let url = "postgres://postgres@localhost:5440/app";
    assert_eq!(
        dotenv_upsert("A=1\nDATABASE_URL=old\nB=2\n", "DATABASE_URL", url),
        format!("A=1\nDATABASE_URL={url}\nB=2\n")
    );
    assert_eq!(
        dotenv_upsert("A=1", "DATABASE_URL", url),
        format!("A=1\nDATABASE_URL={url}\n")
    );
    assert_eq!(
        dotenv_upsert("", "DATABASE_URL", url),
        format!("DATABASE_URL={url}\n")
    );
}

#[test]
fn profiles_roundtrip_without_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("profiles.toml");
    let mut profiles = Profiles::default();
    let (p, _) = parse_database_url("postgres://u:topsecret@localhost:5440/app").unwrap();
    profiles.upsert(p.clone());
    profiles.upsert(p.clone());
    assert_eq!(profiles.items.len(), 1);
    profiles.save(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("topsecret"));
    assert_eq!(Profiles::load(&file).unwrap(), profiles);
}

#[test]
fn keychain_account_is_stable() {
    let (p, _) = parse_database_url("postgres://u@localhost:5440/app").unwrap();
    assert_eq!(p.keychain_account(), "u@localhost:5440/app");
}

#[test]
#[ignore = "touches the login keychain"]
fn keychain_roundtrip() {
    let (p, _) = parse_database_url("postgres://sawit_test@localhost:1/keychain_test").unwrap();
    store_password(&p, "pw-123").unwrap();
    assert_eq!(load_password(&p).unwrap().as_deref(), Some("pw-123"));
    delete_password(&p).unwrap();
    assert_eq!(load_password(&p).unwrap(), None);
}

#[test]
fn pgpass_matches_like_libpq() {
    let p = ConnProfile::new("db.example.com", 5432, "app", "svc");
    let text = "# comment\nother:5432:app:svc:nope\n*:5432:app:svc:pa\\:ss\\\\w\n";
    assert_eq!(
        super::profile::pgpass_lookup(text, &p).as_deref(),
        Some("pa:ss\\w")
    );
    let socket = ConnProfile::new("/tmp", 5432, "app", "svc");
    assert_eq!(
        super::profile::pgpass_lookup("localhost:*:*:svc:s\n", &socket).as_deref(),
        Some("s")
    );
    assert_eq!(super::profile::pgpass_lookup("h:1:a:b\n", &p), None);
}

#[test]
fn display_name_prefers_a_non_blank_label() {
    let mut p = ConnProfile::new("h", 5432, "db", "u");
    assert_eq!(p.display_name(), "u@h:5432/db");
    p.label = Some("  ".into());
    assert_eq!(p.display_name(), "u@h:5432/db");
    p.label = Some("Staging".into());
    assert_eq!(p.display_name(), "Staging");
}
