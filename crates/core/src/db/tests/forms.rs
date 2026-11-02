//! Every common connection-string form people paste: libpq's, JDBC's, SQLAlchemy's, Django's and
//! the cloud consoles' own.

use super::*;

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
