//! Connection strings read as libpq reads them: URLs, keyword/value pairs, what wraps them when
//! pasted, several hosts, service files, and errors that say what is wrong.

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
fn same_database_different_users_are_different_profiles() {
    let (a, _) = parse_database_url("postgres://alice@h:5432/app").unwrap();
    let (b, _) = parse_database_url("postgres://bob@h:5432/app").unwrap();
    assert_ne!(a.name, b.name);
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
fn percent_decode_with_multibyte_does_not_panic() {
    let (_, pw) = parse_database_url("postgres://u:p%a\u{e9}x@localhost/db").unwrap();
    assert_eq!(pw.as_deref(), Some("p%a\u{e9}x"));
    let (_, pw) = parse_database_url("postgres://u:%C3%A9@localhost/db").unwrap();
    assert_eq!(pw.as_deref(), Some("\u{e9}"));
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
    assert_eq!(
        plain.connection_string(),
        "postgres://me@localhost:5432/app"
    );
}
