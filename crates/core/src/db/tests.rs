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
    assert_eq!(p.name, "app_user@localhost:5440/kyzn_crm");
    assert_eq!(p.ssl, SslMode::Disable);
}

#[test]
fn sslmode_from_url_and_defaults() {
    let mode = |url: &str| parse_database_url(url).unwrap().0.ssl;
    assert_eq!(mode("postgres://u@db.example.com/app"), SslMode::Prefer);
    assert_eq!(
        mode("postgres://u@db.example.com/app?sslmode=require"),
        SslMode::Require
    );
    assert_eq!(
        mode("postgres://u@h/app?connect_timeout=5&sslmode=verify-full"),
        SslMode::VerifyFull
    );
    assert_eq!(
        mode("postgres://u@h/app?sslmode=verify-ca"),
        SslMode::VerifyCa
    );
    assert!(parse_database_url("postgres://u@h/app?sslmode=bogus").is_err());
}

#[test]
fn parses_neon_strings_as_pasted() {
    let url = "postgresql://neondb_owner:npg_AbC123@ep-cool-name-a1b2c3-pooler.us-east-2.aws.neon.tech/neondb?sslmode=require&channel_binding=require";
    for pasted in [
        url.to_string(),
        format!("psql '{url}'"),
        format!("DATABASE_URL=\"{url}\""),
        format!("  export DATABASE_URL='{url}'\n"),
    ] {
        let (p, pw) = parse_database_url(&pasted).unwrap();
        assert_eq!(p.host, "ep-cool-name-a1b2c3-pooler.us-east-2.aws.neon.tech");
        assert_eq!((p.port, p.database.as_str()), (5432, "neondb"));
        assert_eq!(p.user, "neondb_owner");
        assert_eq!(pw.as_deref(), Some("npg_AbC123"));
        assert_eq!(p.ssl, SslMode::Require);
    }
}

#[test]
fn parses_cloud_sql_socket_and_certificates() {
    // Cloud Run / App Engine style: the host is a socket directory.
    let (p, pw) =
        parse_database_url("postgres://app:pw@/appdb?host=/cloudsql/my-proj:asia-southeast2:main")
            .unwrap();
    assert_eq!(p.host, "/cloudsql/my-proj:asia-southeast2:main");
    assert!(p.is_socket());
    assert_eq!((p.database.as_str(), pw.as_deref()), ("appdb", Some("pw")));
    assert!(p.url().contains("?host=%2Fcloudsql%2F"), "{}", p.url());
    // Public IP with the server CA and a client certificate.
    let (p, _) = parse_database_url(
        "postgresql://app@34.101.1.2:5432/appdb?sslmode=verify-ca&sslrootcert=/certs/server-ca.pem&sslcert=/certs/client-cert.pem&sslkey=/certs/client-key.pem",
    )
    .unwrap();
    assert_eq!(p.ssl, SslMode::VerifyCa);
    assert_eq!(p.ssl_root_cert.as_deref(), Some("/certs/server-ca.pem"));
    assert_eq!(p.ssl_cert.as_deref(), Some("/certs/client-cert.pem"));
    assert_eq!(p.ssl_key.as_deref(), Some("/certs/client-key.pem"));
    // Query parameters override the URL's parts, as in libpq.
    let (p, pw) = parse_database_url(
        "postgres://x@h/d?host=10.0.0.5&port=6543&user=svc&password=p%26w&dbname=prod",
    )
    .unwrap();
    assert_eq!(
        (
            p.host.as_str(),
            p.port,
            p.user.as_str(),
            p.database.as_str()
        ),
        ("10.0.0.5", 6543, "svc", "prod")
    );
    assert_eq!(pw.as_deref(), Some("p&w"));
}

#[test]
fn parses_keyword_value_strings() {
    let (p, pw) = parse_database_url(
        "host=34.101.1.2 port=5433 dbname=app user=svc password='p w' sslmode=require",
    )
    .unwrap();
    assert_eq!((p.host.as_str(), p.port), ("34.101.1.2", 5433));
    assert_eq!((p.database.as_str(), p.user.as_str()), ("app", "svc"));
    assert_eq!((p.ssl, pw.as_deref()), (SslMode::Require, Some("p w")));
    let (p, pw) = parse_database_url("host=db user=u password='a b\\'c'").unwrap();
    assert_eq!((p.host.as_str(), p.database.as_str()), ("db", "u"));
    assert_eq!(pw.as_deref(), Some("a b'c"));
    assert!(parse_database_url("host=db user").is_err());
    assert!(parse_database_url("host='db").is_err());
}

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
fn same_database_different_users_are_different_profiles() {
    let (a, _) = parse_database_url("postgres://alice@h:5432/app").unwrap();
    let (b, _) = parse_database_url("postgres://bob@h:5432/app").unwrap();
    assert_ne!(a.name, b.name);
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
    let (p, _) = parse_database_url("postgres://sawit_test@localhost:1/keychain_test").unwrap();
    store_password(&p, "pw-123").unwrap();
    assert_eq!(load_password(&p).unwrap().as_deref(), Some("pw-123"));
    delete_password(&p).unwrap();
    assert_eq!(load_password(&p).unwrap(), None);
}

#[test]
fn result_set_renders_as_aligned_text_table() {
    let set = ResultSet {
        columns: vec!["id".into(), "email".into()],
        rows: vec![
            vec![Some("1".into()), Some("a@x.dev".into())],
            vec![Some("22".into()), None],
        ],
        truncated: true,
    };
    assert_eq!(
        format_table(&set),
        "id | email\n---+--------\n1  | a@x.dev\n22 | NULL\n(2 rows, truncated)"
    );
}

#[test]
fn percent_decode_with_multibyte_does_not_panic() {
    let (_, pw) = parse_database_url("postgres://u:p%a\u{e9}x@localhost/db").unwrap();
    assert_eq!(pw.as_deref(), Some("p%a\u{e9}x"));
    let (_, pw) = parse_database_url("postgres://u:%C3%A9@localhost/db").unwrap();
    assert_eq!(pw.as_deref(), Some("\u{e9}"));
}

#[test]
fn read_only_sql_screening() {
    assert!(screen_read_only_sql("select 1").is_ok());
    assert!(screen_read_only_sql("  -- c\n COPY (SELECT 1) TO PROGRAM 'touch x'").is_err());
    assert!(screen_read_only_sql("(copy t to '/tmp/x')").is_err());
    assert!(screen_read_only_sql("set default_transaction_read_only = off").is_err());
}

#[test]
fn recognises_poolers_rejecting_startup_options() {
    use super::query::rejects_startup_options;
    assert!(rejects_startup_options(
        "db error: FATAL: unsupported startup parameter: options"
    ));
    assert!(rejects_startup_options(
        "FATAL: unsupported startup parameter in options: synchronize_seqscans"
    ));
    assert!(!rejects_startup_options(
        "FATAL: password authentication failed for user \"app\""
    ));
}

/// (input, host, port, database, user, password, sslmode)
type Expect<'a> = (
    &'a str,
    &'a str,
    u16,
    &'a str,
    &'a str,
    Option<&'a str>,
    SslMode,
);

#[test]
fn parses_every_common_connection_string_form() {
    use SslMode::*;
    let cases: &[Expect] = &[
        // libpq URLs, both schemes, defaults.
        ("postgres://u:p@h/d", "h", 5432, "d", "u", Some("p"), Prefer),
        (
            "postgresql://u@h:6543/d?sslmode=require",
            "h",
            6543,
            "d",
            "u",
            None,
            Require,
        ),
        ("postgres://u@h", "h", 5432, "u", "u", None, Prefer),
        (
            "postgres://u@:5433/d",
            "localhost",
            5433,
            "d",
            "u",
            None,
            Prefer,
        ),
        ("POSTGRES://u@h/d", "h", 5432, "d", "u", None, Prefer),
        // Unencoded special characters in passwords.
        (
            "postgres://u:p@ss#w/rd?x@h:5432/d?sslmode=require",
            "h",
            5432,
            "d",
            "u",
            Some("p@ss#w/rd?x"),
            Require,
        ),
        (
            "postgres://u:a%40b%3Ac@h/d",
            "h",
            5432,
            "d",
            "u",
            Some("a@b:c"),
            Prefer,
        ),
        (
            "postgres://u:pa+ss@h/d?password=x+y",
            "h",
            5432,
            "d",
            "u",
            Some("x+y"),
            Prefer,
        ),
        // Query parameters with `@` don't count as user info.
        (
            "postgres://h/d?user=me@corp&password=pw",
            "h",
            5432,
            "d",
            "me@corp",
            Some("pw"),
            Prefer,
        ),
        // IPv6.
        (
            "postgres://u@[2001:db8::1]:5433/d",
            "2001:db8::1",
            5433,
            "d",
            "u",
            None,
            Prefer,
        ),
        // Socket directories: query, percent-encoded authority, Cloud SQL.
        (
            "postgresql:///d?host=/var/run/postgresql",
            "/var/run/postgresql",
            5432,
            "d",
            whoami(),
            None,
            Prefer,
        ),
        (
            "postgresql://%2Fcloudsql%2Fp%3Ar%3Ai/d",
            "/cloudsql/p:r:i",
            5432,
            "d",
            whoami(),
            None,
            Prefer,
        ),
        (
            "postgresql://%2Fcloudsql%2Fp:r:i/d",
            "/cloudsql/p:r:i",
            5432,
            "d",
            whoami(),
            None,
            Prefer,
        ),
        // Other drivers' schemes and TLS flags.
        (
            "jdbc:postgresql://h:5432/d?user=u&password=p&ssl=true",
            "h",
            5432,
            "d",
            "u",
            Some("p"),
            Require,
        ),
        (
            "jdbc:postgresql://h/d?sslfactory=org.postgresql.ssl.NonValidatingFactory",
            "h",
            5432,
            "d",
            whoami(),
            None,
            Require,
        ),
        (
            "postgresql+psycopg2://u:p@h/d",
            "h",
            5432,
            "d",
            "u",
            Some("p"),
            Prefer,
        ),
        (
            "postgresql+asyncpg://u:p@h/d?ssl=require",
            "h",
            5432,
            "d",
            "u",
            Some("p"),
            Require,
        ),
        (
            "postgis://u:p@h/geo",
            "h",
            5432,
            "geo",
            "u",
            Some("p"),
            Prefer,
        ),
        ("pgsql://u@h/d", "h", 5432, "d", "u", None, Prefer),
        (
            "postgres://u@h/d?sslmode=no-verify",
            "h",
            5432,
            "d",
            "u",
            None,
            Require,
        ),
        (
            "postgres://u@h/d?sslaccept=strict",
            "h",
            5432,
            "d",
            "u",
            None,
            VerifyFull,
        ),
        (
            "postgres://u@h/d?ssl=true&sslmode=disable",
            "h",
            5432,
            "d",
            "u",
            None,
            Disable,
        ),
        (
            "postgres://u@h/d?sslmode=allow",
            "h",
            5432,
            "d",
            "u",
            None,
            Prefer,
        ),
        (
            "postgres://u@h/d?sslrootcert=system",
            "h",
            5432,
            "d",
            "u",
            None,
            VerifyFull,
        ),
        // Providers, as copied from their consoles.
        (
            "postgresql://postgres.abcd:pw@aws-0-ap-southeast-1.pooler.supabase.com:6543/postgres?pgbouncer=true",
            "aws-0-ap-southeast-1.pooler.supabase.com",
            6543,
            "postgres",
            "postgres.abcd",
            Some("pw"),
            Prefer,
        ),
        (
            "postgres://u:p@ec2-1-2-3-4.compute-1.amazonaws.com:5432/dabc",
            "ec2-1-2-3-4.compute-1.amazonaws.com",
            5432,
            "dabc",
            "u",
            Some("p"),
            Prefer,
        ),
        (
            "postgresql://u:p@db.x.us-east-1.rds.amazonaws.com:5432/app?sslmode=verify-full&sslrootcert=/certs/rds.pem",
            "db.x.us-east-1.rds.amazonaws.com",
            5432,
            "app",
            "u",
            Some("p"),
            VerifyFull,
        ),
        (
            "postgresql://u:p@x.cockroachlabs.cloud:26257/defaultdb?sslmode=verify-full",
            "x.cockroachlabs.cloud",
            26257,
            "defaultdb",
            "u",
            Some("p"),
            VerifyFull,
        ),
        // Keyword/value.
        (
            "host=h port=5433 dbname=d user=u password=p sslmode=require",
            "h",
            5433,
            "d",
            "u",
            Some("p"),
            Require,
        ),
        (
            "host = h  dbname = 'my db'",
            "h",
            5432,
            "my db",
            whoami(),
            None,
            Prefer,
        ),
        (
            "hostaddr=10.0.0.5 dbname=d",
            "10.0.0.5",
            5432,
            "d",
            whoami(),
            None,
            Prefer,
        ),
        // Wrapped when copied.
        ("`postgres://u@h/d`", "h", 5432, "d", "u", None, Prefer),
        (
            "DATABASE_URL_2='postgres://u@h/d'",
            "h",
            5432,
            "d",
            "u",
            None,
            Prefer,
        ),
    ];
    for (input, host, port, db, user, pw, ssl) in cases {
        let (p, password) = parse_database_url(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            (
                p.host.as_str(),
                p.port,
                p.database.as_str(),
                p.user.as_str(),
                password.as_deref(),
                p.ssl
            ),
            (*host, *port, *db, *user, *pw, *ssl),
            "{input}"
        );
    }
}

fn whoami() -> &'static str {
    Box::leak(
        std::env::var("USER")
            .unwrap_or_else(|_| "postgres".into())
            .into_boxed_str(),
    )
}

#[test]
fn parses_multiple_hosts_and_session_settings() {
    let (p, _) = parse_database_url(
        "postgres://u@h1:5432,h2:5433,[::1]/d?target_session_attrs=read-write&connect_timeout=7&channel_binding=require&options=-c%20search_path%3Dapp",
    )
    .unwrap();
    assert_eq!((p.host.as_str(), p.port), ("h1", 5432));
    assert_eq!(
        p.fallback_hosts,
        vec![
            HostPort {
                host: "h2".into(),
                port: 5433
            },
            HostPort {
                host: "::1".into(),
                port: 5432
            },
        ]
    );
    assert_eq!(p.target_session_attrs, Some(SessionAttrs::ReadWrite));
    assert_eq!(p.connect_timeout, Some(7));
    assert_eq!(p.channel_binding, Some(ChannelBinding::Require));
    assert_eq!(p.options.as_deref(), Some("-c search_path=app"));
    assert!(
        p.url().contains("h1:5432,h2:5433,[::1]:5432"),
        "{}",
        p.url()
    );
    // Keyword form: one port for all hosts, or one each.
    let (p, _) = parse_database_url("host=a,b port=6000 dbname=d").unwrap();
    assert_eq!((p.port, p.fallback_hosts[0].port), (6000, 6000));
    let (p, _) = parse_database_url("host=a,b port=1,2 dbname=d").unwrap();
    assert_eq!((p.port, p.fallback_hosts[0].port), (1, 2));
    assert!(parse_database_url("host=a,b port=1,2,3").is_err());
    // Neon's endpoint option survives for clients without SNI.
    let (p, _) =
        parse_database_url("postgres://u@ep-x.neon.tech/d?options=endpoint%3Dep-x&sslmode=require")
            .unwrap();
    assert_eq!(p.options.as_deref(), Some("endpoint=ep-x"));
}

#[test]
fn explains_strings_it_cannot_use() {
    let msg = |s: &str| parse_database_url(s).unwrap_err().to_string();
    assert!(msg("mysql://root@h/db").contains("MySQL"));
    assert!(msg("mongodb+srv://u@c.mongodb.net/db").contains("MongoDB"));
    assert!(msg("prisma+postgres://accelerate.prisma-data.net/?api_key=x").contains("DIRECT_URL"));
    assert!(msg("postgres://u@h:99999/d").contains("port"));
    assert!(msg("postgres://u@h:0/d").contains("port"));
    assert!(msg("postgres://u@h/d?sslmode=maybe").contains("sslmode"));
    assert!(msg("postgres://u@h/d?ssl=perhaps").contains("ssl"));
    assert!(msg("postgres://u@h/d?connect_timeout=soon").contains("seconds"));
    assert!(msg("postgres://u@h1,,h3/d").contains("empty"));
    assert!(msg("postgres://u@[::1/d").contains("]"));
    assert!(msg("").contains("Paste"));
    assert!(msg("just some text").contains("Not a PostgreSQL"));
    assert!(msg("service=definitely-not-defined-anywhere").contains("Service"));
}

#[test]
fn service_file_sections_supply_defaults() {
    let text = "# comment\n[other]\nhost=x\n\n[prod]\nhost=db.prod\nport=6432\ndbname=app\nuser=svc\nsslmode=require\n";
    let params = super::profile::service_section(text, "prod").unwrap();
    assert_eq!(params[0], ("host".to_string(), "db.prod".to_string()));
    assert_eq!(params.len(), 5);
    assert!(super::profile::service_section(text, "missing").is_none());
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
fn failures_are_classified_for_retrying() {
    use super::query::FailureKind::*;
    // Proxies (Neon's) answer in plain text; SQLSTATE and I/O kinds are
    // covered by the integration tests.
    let msg = super::query::classify_message;
    assert_eq!(
        msg("FATAL: password authentication failed for user \"x\""),
        Auth
    );
    assert_eq!(
        msg("FATAL: database \"nope\" does not exist"),
        DatabaseMissing
    );
    assert_eq!(
        msg("FATAL: sorry, too many clients already: too many connections"),
        ServerBusy
    );
    assert_eq!(msg("ERROR: Couldn't connect to compute node"), ServerBusy);
    assert_eq!(
        msg("error connecting to server: failed to lookup address information"),
        Network
    );
    assert_eq!(msg("timeout waiting for server"), Network);
    assert_eq!(
        msg("error performing TLS handshake: invalid peer certificate"),
        Tls
    );
    assert_eq!(msg("ERROR: Endpoint ID is not specified"), Config);
    assert!(Network.retryable() && ServerBusy.retryable());
    assert!(!Auth.retryable() && !Tls.retryable() && !DatabaseMissing.retryable());
    assert_eq!(failure_kind(&crate::CoreError::Config("x".into())), Config);
}

#[test]
fn connection_string_round_trips_settings() {
    let (p, _) = parse_database_url(
        "postgres://app:secret@db.example.com:6543/shop?sslmode=verify-full&sslrootcert=/tmp/ca%20file.pem&options=-c%20search_path%3Dapp&connect_timeout=7&target_session_attrs=read-write",
    )
    .unwrap();
    let url = p.connection_string();
    assert!(!url.contains("secret"), "{url}");
    let (back, password) = parse_database_url(&url).unwrap();
    assert_eq!(password, None);
    assert_eq!(back, p);
    // Defaults add nothing.
    let plain = ConnProfile::new("localhost", 5432, "app", "me");
    assert_eq!(plain.connection_string(), "postgres://me@localhost:5432/app");
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

#[test]
fn row_query_builds_quoted_clauses() {
    let f = |column: &str, op, value: &str| RowFilter {
        column: column.into(),
        op,
        value: value.into(),
    };
    let q = RowQuery {
        sort: Some(("Created At".into(), SortDir::Desc)),
        filters: vec![
            f("id", FilterOp::Ge, "10"),
            f("name", FilterOp::Contains, "50%_o'k!"),
            f("note", FilterOp::IsNull, "ignored"),
        ],
        search: "  x  ".into(),
    };
    assert_eq!(
        q.where_sql("r").unwrap(),
        "r.\"id\" >= '10' AND r.\"name\"::text ILIKE '%50!%!_o''k!!%' ESCAPE '!' \
         AND r.\"note\" IS NULL AND r::text ILIKE '%x%' ESCAPE '!'"
    );
    assert_eq!(
        q.order_sql("r", &["id".into(), "Created At".into()]).unwrap(),
        "r.\"Created At\" DESC NULLS LAST, r.\"id\""
    );
    assert!(q.is_filtered());
    let none = RowQuery::default();
    assert_eq!(none.where_sql("r"), None);
    assert_eq!(none.order_sql("r", &[]), None);
    assert!(!none.is_filtered());
    // Backslashes survive whatever standard_conforming_strings is.
    assert_eq!(quote_literal(r"a\b'c"), r"E'a\\b''c'");
}
