//! The SQL Sawit writes and screens: quoting, result tables, read-only screening, failure kinds,
//! row filters and bulk changes.

use super::*;

#[test]
fn quote_ident_escapes_quotes() {
    assert_eq!(quote_ident("users"), "\"users\"");
    assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
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
fn read_only_sql_screening() {
    assert!(screen_read_only_sql("select 1").is_ok());
    assert!(screen_read_only_sql("  -- c\n COPY (SELECT 1) TO PROGRAM 'touch x'").is_err());
    assert!(screen_read_only_sql("(copy t to '/tmp/x')").is_err());
    assert!(screen_read_only_sql("set default_transaction_read_only = off").is_err());
}

/// A statement the server couldn't parse may be several, only with a `;` before the end of its
/// text: `sawit <db>` then asks before running them, as it can't try them read-only first.
#[test]
fn several_statements_have_a_semicolon_before_the_end() {
    use super::query::several_statements;
    assert!(several_statements("select 1; select 2"));
    assert!(several_statements("commit; delete from t;"));
    assert!(!several_statements("select 1"));
    assert!(!several_statements("select 1;"));
    assert!(!several_statements("delete from t ;  \n"));
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
        q.order_sql("r", &["id".into(), "Created At".into()])
            .unwrap(),
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

#[test]
fn bulk_changes_target_keys_or_the_query() {
    let keys = RowTarget::Keys {
        key: vec!["id".into()],
        rows: vec![vec!["1".into()], vec!["o'k".into()]],
    };
    assert_eq!(
        bulk_sql("public", "t", &keys, &BulkChange::Delete),
        "DELETE FROM \"public\".\"t\" AS \"sawit row\" WHERE \"sawit row\".\"id\" IN ('1', 'o''k')"
    );
    let pair = RowTarget::Keys {
        key: vec!["a".into(), "b".into()],
        rows: vec![vec!["1".into(), "x".into()]],
    };
    assert_eq!(
        bulk_sql(
            "s",
            "t",
            &pair,
            &BulkChange::Update {
                column: "Note".into(),
                value: None
            }
        ),
        "UPDATE \"s\".\"t\" AS \"sawit row\" SET \"Note\" = NULL \
         WHERE (\"sawit row\".\"a\", \"sawit row\".\"b\") IN (('1', 'x'))"
    );
    let none = RowTarget::Keys {
        key: vec!["id".into()],
        rows: vec![],
    };
    assert!(bulk_sql("s", "t", &none, &BulkChange::Delete).ends_with(" WHERE false"));
    let all = RowTarget::Matching(RowQuery::default());
    assert_eq!(
        bulk_sql(
            "s",
            "t",
            &all,
            &BulkChange::Update {
                column: "n".into(),
                value: Some("5".into())
            }
        ),
        "UPDATE \"s\".\"t\" AS \"sawit row\" SET \"n\" = '5'"
    );
}
