//! Talking to a database: connect, schema tree, paged reads, ad-hoc SQL.

mod connect;
mod failure;
mod read;
mod read_only;
mod run;
mod schema;

pub use connect::{cancel, connect, connect_all, connect_read_only};
pub use failure::{FailureKind, failure_kind, sql_error};
pub use read::{
    RowCount, fetch_page, fetch_rows, row_count, row_count_estimated, row_count_matching,
    row_count_within,
};
pub use read_only::{
    READ_ONLY_ROLE, ReadOnlyTry, ensure_read_only_role, run_if_read_only, run_read_only,
    screen_read_only_sql,
};
pub use run::{QueryOutcome, ResultSet, run_bulk, run_sql};
pub use schema::{
    ColumnInfo, Constraint, ConstraintKind, ForeignKey, IndexInfo, RelKind, Relation, SchemaInfo,
    list_databases, load_schema, primary_key,
};

pub(super) use read::select_sql;

#[cfg(test)]
pub(super) use connect::rejects_startup_options;
#[cfg(test)]
pub(super) use failure::classify_message;
#[cfg(test)]
pub(super) use read_only::several_statements;
