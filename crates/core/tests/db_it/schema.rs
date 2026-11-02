//! The schema tree from a real server: columns, keys, estimates, constraints, indexes and
//! materialized views, what the role can read, and the diagram drawn from it.

use sawit_core::db::{self, ConnProfile, RelKind};

use super::cluster;

#[tokio::test]
async fn schema_paging_and_sql() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            "CREATE SCHEMA app;
             CREATE TABLE app.users (id bigint PRIMARY KEY, email text NOT NULL, note text);
             INSERT INTO app.users SELECT g, 'user' || g || '@x.dev', CASE WHEN g % 2 = 0 THEN NULL ELSE 'n' END
               FROM generate_series(1, 250000) g;
             CREATE VIEW app.active AS SELECT id FROM app.users WHERE id < 10;
             ANALYZE app.users;",
        )
        .await
        .unwrap();

    // Schema tree.
    let schemas = db::load_schema(&client).await.unwrap();
    let app = schemas
        .iter()
        .find(|s| s.name == "app")
        .expect("app schema");
    let users = app.relations.iter().find(|r| r.name == "users").unwrap();
    assert_eq!(users.kind, RelKind::Table);
    let cols: Vec<_> = users
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c.data_type.as_str(), c.nullable))
        .collect();
    assert_eq!(
        cols,
        vec![
            ("id", "bigint", false),
            ("email", "text", false),
            ("note", "text", true)
        ]
    );
    assert!(
        app.relations
            .iter()
            .any(|r| r.name == "active" && r.kind == RelKind::View)
    );
    assert!(
        !schemas
            .iter()
            .any(|s| s.name == "pg_catalog" || s.name == "information_schema")
    );

    // Count and pages.
    assert_eq!(
        db::row_count(&client, "app", "users").await.unwrap(),
        db::RowCount {
            rows: 250_000,
            exact: true
        }
    );
    let page = db::fetch_page(&client, "app", "users", &[], 200_000, 200)
        .await
        .unwrap();
    assert_eq!(page.columns, vec!["id", "email", "note"]);
    assert_eq!(page.rows.len(), 200);
    assert_eq!(page.rows[0][0].as_deref(), Some("200001"));
    assert_eq!(page.rows[0][2].as_deref(), Some("n")); // odd id
    assert_eq!(page.rows[1][2], None); // even id → NULL

    // Ad-hoc SQL: results, affected rows, errors.
    let out = db::run_sql(&client, "UPDATE app.users SET note = 'z' WHERE id <= 3; SELECT id, note FROM app.users WHERE id <= 3 ORDER BY id", 1000)
        .await
        .unwrap();
    assert_eq!(out.affected, vec![3]);
    let last = out.results.last().unwrap();
    assert_eq!(last.rows.len(), 3);
    assert_eq!(last.rows[2][1].as_deref(), Some("z"));
    let truncated = db::run_sql(&client, "SELECT * FROM app.users", 50)
        .await
        .unwrap();
    assert_eq!(truncated.results[0].rows.len(), 50);
    assert!(truncated.results[0].truncated);
    let err = db::run_sql(&client, "SELECT * FROM missing_table", 10)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing_table"), "{err}");

    // Read-only connections refuse writes.
    let ro = db::connect_read_only(&profile, None).await.unwrap();
    assert!(
        db::run_read_only(&ro, "SELECT count(*) FROM app.users", 10)
            .await
            .is_ok()
    );
    assert!(
        db::run_read_only(&ro, "DELETE FROM app.users", 10)
            .await
            .is_err()
    );
    assert!(
        db::run_read_only(&ro, "COMMIT; DELETE FROM app.users", 10)
            .await
            .is_err()
    );
    assert!(
        db::run_read_only(
            &ro,
            "SET default_transaction_read_only = off; DELETE FROM app.users",
            10
        )
        .await
        .is_err()
    );
    assert_eq!(
        db::row_count(&client, "app", "users").await.unwrap().rows,
        250_000
    );

    let dbs = db::list_databases(&client).await.unwrap();
    assert!(dbs.contains(&"postgres".to_string()));

    // Views get a bounded exact count (no unbounded count(*) on huge views).
    assert_eq!(
        db::row_count(&client, "app", "active").await.unwrap().rows,
        9
    );
}

#[tokio::test]
async fn schema_marks_relations_the_role_cannot_read() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute(
            "CREATE TABLE open_t (x int); CREATE TABLE secret_t (x int);
             CREATE ROLE limited LOGIN; GRANT SELECT ON open_t TO limited;",
        )
        .await
        .unwrap();
    let limited = ConnProfile {
        user: "limited".into(),
        ..profile.clone()
    };
    let client = db::connect(&limited, None).await.unwrap();
    let schemas = db::load_schema(&client).await.unwrap();
    let public = schemas.iter().find(|s| s.name == "public").unwrap();
    let readable = |name: &str| {
        public
            .relations
            .iter()
            .find(|r| r.name == name)
            .unwrap()
            .readable
    };
    assert!(readable("open_t"));
    assert!(!readable("secret_t"));
}

#[tokio::test]
async fn schema_carries_keys_estimates_and_draws_as_a_diagram() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute(
            "CREATE TABLE users (id bigint PRIMARY KEY, email varchar(255) NOT NULL);
             CREATE TABLE line (order_id int, n int, user_id bigint REFERENCES users(id),
                                PRIMARY KEY (order_id, n));
             INSERT INTO users SELECT g, 'u' || g FROM generate_series(1, 500) g;
             ANALYZE users;",
        )
        .await
        .unwrap();
    // Keyword/value connection strings connect like URLs.
    let (kv, _) = db::parse_database_url(&format!(
        "host=localhost port={} dbname=postgres user=postgres sslmode=disable",
        profile.port
    ))
    .unwrap();
    let (_rw, _ro, schemas) = db::connect_all(&kv, None).await.unwrap();
    let public = schemas.iter().find(|s| s.name == "public").unwrap();
    let rel = |n: &str| public.relations.iter().find(|r| r.name == n).unwrap();
    assert_eq!(rel("users").primary_key, vec!["id"]);
    assert_eq!(rel("users").estimated_rows, Some(500));
    assert_eq!(rel("line").primary_key, vec!["order_id", "n"]);
    assert_eq!(rel("line").estimated_rows, None); // never analyzed
    assert_eq!(
        rel("line").foreign_keys,
        vec![db::ForeignKey {
            columns: vec!["user_id".into()],
            ref_schema: "public".into(),
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
        }]
    );
    let dbml = db::diagram::to_dbml(&schemas, Some("public"));
    assert!(
        dbml.contains("Ref: public.line.user_id > public.users.id"),
        "{dbml}"
    );
    let er = db::diagram::to_mermaid(&schemas, Some("public"));
    let drawn = sawit_core::mermaid::render(&er, &Default::default()).unwrap();
    assert_eq!(drawn.kind, "er");
    assert!(drawn.svg.contains("public.users"));
}

#[tokio::test]
async fn schema_lists_indexes_constraints_and_matviews() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            "CREATE TABLE teams (id int PRIMARY KEY);
             CREATE TABLE players (
               id int PRIMARY KEY,
               team int REFERENCES teams(id),
               email text CONSTRAINT players_email_key UNIQUE,
               age int CONSTRAINT adult CHECK (age >= 18));
             CREATE INDEX players_team_idx ON players USING hash (team);
             CREATE MATERIALIZED VIEW roster AS SELECT id, team FROM players;
             CREATE INDEX roster_team ON roster (team);",
        )
        .await
        .unwrap();
    let schemas = db::load_schema(&client).await.unwrap();
    let public = schemas.iter().find(|s| s.name == "public").unwrap();
    let rel = |name: &str| public.relations.iter().find(|r| r.name == name).unwrap();

    let players = rel("players");
    let kinds: Vec<_> = players
        .constraints
        .iter()
        .map(|c| (c.name.as_str(), c.kind))
        .filter(|(_, k)| *k != db::ConstraintKind::Other) // NOT NULL rows on PG 18
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("adult", db::ConstraintKind::Check),
            ("players_email_key", db::ConstraintKind::Unique),
            ("players_pkey", db::ConstraintKind::PrimaryKey),
            ("players_team_fkey", db::ConstraintKind::ForeignKey),
        ]
    );
    let check = players
        .constraints
        .iter()
        .find(|c| c.name == "adult")
        .unwrap();
    assert_eq!(check.definition, "CHECK ((age >= 18))");
    // The key and foreign key the diagram uses still load.
    assert_eq!(players.primary_key, vec!["id"]);
    assert_eq!(players.foreign_keys[0].ref_table, "teams");

    let indexes: Vec<_> = players
        .indexes
        .iter()
        .map(|i| (i.name.as_str(), i.method.as_str(), i.unique, i.primary))
        .collect();
    assert_eq!(
        indexes,
        vec![
            ("players_email_key", "btree", true, false),
            ("players_pkey", "btree", true, true),
            ("players_team_idx", "hash", false, false),
        ]
    );
    assert!(players.indexes[2].definition.contains("USING hash (team)"));

    let roster = rel("roster");
    assert_eq!(roster.kind, RelKind::MaterializedView);
    assert_eq!(roster.indexes.len(), 1);
    assert!(roster.constraints.is_empty());
}
