//! Tests of the `sawit` command's terminal tools.

use std::ffi::OsString;
use std::path::Path;

use super::*;

mod app;
mod query;
mod tools;
mod words;

fn q(args: &[&str]) -> Option<QueryArgs> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    query_args(&args, Path::new("/nonexistent-sawit-test"))
}

fn qa(db: &str, table: Option<&str>, sql: Option<&str>) -> Option<QueryArgs> {
    Some(QueryArgs {
        db: db.into(),
        table: table.map(Into::into),
        sql: sql.map(Into::into),
        yes: false,
    })
}
