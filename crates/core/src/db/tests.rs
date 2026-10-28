use super::*;

#[test]
fn parses_full_database_url() {
    let (p, pw) =
        parse_database_url("postgres://app_user:s3cr%40t@localhost:5440/kyzn_crm?sslmode=disable")
            .unwrap();
    assert_eq!(p.host, "localhost");
    assert_eq!(p.port, 5440);
    assert_eq!(p.user, "app_user");
    assert_eq!(p.database, "kyzn_crm");
    assert_eq!(pw.as_deref(), Some("s3cr@t"));
    assert_eq!(p.name, "kyzn_crm@localhost:5440");
}

#[test]
fn database_url_defaults() {
    let (p, pw) = parse_database_url("postgresql://postgres@127.0.0.1").unwrap();
    assert_eq!((p.port, p.database.as_str(), pw), (5432, "postgres", None));
    let (p, _) = parse_database_url("postgres://bob@[::1]:5433/x").unwrap();
    assert_eq!((p.host.as_str(), p.port), ("::1", 5433));
}

#[test]
fn rejects_non_postgres_url() {
    assert!(parse_database_url("mysql://root@localhost/db").is_err());
    assert!(parse_database_url("not a url").is_err());
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
fn quote_ident_escapes_quotes() {
    assert_eq!(quote_ident("users"), "\"users\"");
    assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
}

#[test]
fn keychain_account_is_stable() {
    let (p, _) = parse_database_url("postgres://u@localhost:5440/app").unwrap();
    assert_eq!(p.keychain_account(), "u@localhost:5440/app");
}

#[test]
#[ignore = "touches the login keychain"]
fn keychain_roundtrip() {
    let (p, _) = parse_database_url("postgres://pgdesk_test@localhost:1/keychain_test").unwrap();
    store_password(&p, "pw-123").unwrap();
    assert_eq!(load_password(&p).unwrap().as_deref(), Some("pw-123"));
    delete_password(&p).unwrap();
    assert_eq!(load_password(&p).unwrap(), None);
}
