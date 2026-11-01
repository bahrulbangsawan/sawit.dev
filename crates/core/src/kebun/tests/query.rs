//! What `sawit <db>` plans and shows: its SQL, the statement it asks about, clipped text.

use crate::kebun::ink::clip;
use crate::kebun::query::statement_preview;
use crate::kebun::sql::LIST_TABLES;

use super::*;

/// Before it asks, `sawit <db>` shows the statement: up to 20 lines, each cut at 200
/// characters, then how many more lines there are.
#[test]
fn a_statement_is_shown_before_it_runs() {
    assert_eq!(
        statement_preview("  delete from leads\n where id = 3\n"),
        ["delete from leads", " where id = 3"]
    );
    let long = (1..=25)
        .map(|n| format!("insert into t values ({n});"))
        .collect::<Vec<_>>()
        .join("\n");
    let shown = statement_preview(&long);
    assert_eq!(shown.len(), 21);
    assert_eq!(shown[19], "insert into t values (20);");
    assert_eq!(shown[20], "… (5 baris lagi)");
    let wide = statement_preview(&"x".repeat(300));
    assert_eq!(wide[0].chars().count(), 200);
    assert!(wide[0].ends_with('…'));
}

#[test]
fn clipping() {
    assert_eq!(clip("bun run dev", 60), "bun run dev");
    assert_eq!(clip("abcdef", 4), "abc…");
    assert_eq!(clip("abcd", 4), "abcd");
}

#[test]
fn sql_plans() {
    assert_eq!(plan_sql(None, None), LIST_TABLES);
    assert_eq!(
        plan_sql(Some("leads"), None),
        "SELECT * FROM \"leads\" LIMIT 100"
    );
    assert_eq!(
        plan_sql(
            Some("leads"),
            Some("select * from leads where phone is null")
        ),
        "select * from leads where phone is null"
    );
    assert_eq!(
        plan_sql(Some("leads"), Some("phone is null")),
        "SELECT * FROM \"leads\" WHERE phone is null"
    );
    assert_eq!(
        plan_sql(Some("crm.leads"), Some(" order by id desc limit 5")),
        "SELECT * FROM \"crm\".\"leads\" order by id desc limit 5"
    );
    assert_eq!(
        plan_sql(Some("leads"), Some("(select 1) union (select 2)")),
        "(select 1) union (select 2)"
    );
    assert_eq!(
        plan_sql(
            Some("\"Leads\""),
            Some("WITH x AS (select 1) select * from x")
        ),
        "WITH x AS (select 1) select * from x"
    );
    assert_eq!(plan_sql(None, Some("select 1")), "select 1");
}
