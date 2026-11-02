//! The `sawit` command's terminal tools, which work without the app:
//! `sawit <db> [table] ["sql"]` like psql, `sawit list`, `sawit bakar`
//! ("burn": stop dev servers or instances, like the `kill-dev` script),
//! `sawit panen|backup|tanam` (export, `pg_dump`, CSV import),
//! `sawit pasang|pupuk|cabut` (install, upgrade or uninstall developer
//! tools) and `sawit nyalakan` (start instances). Two need the app:
//! `sawit browse` (a page in the app's browser), which starts it first, and
//! `sawit list keys`, which asks the running app and never starts it.
//! The wording riffs on Indonesia's forest fires (karhutla) and the palm-oil
//! plantations (kebun sawit) behind them.

mod args;
mod bakar;
mod bibit;
mod browse;
mod db_job;
mod find;
mod ink;
mod keys;
mod list;
mod nyalakan;
mod query;
mod sql;
mod tools;

pub use args::{
    BakarArgs, BakarWhat, CabutArgs, DbJob, DbJobKind, ListWhat, NyalakanArgs, PasangArgs,
    PupukArgs, QueryArgs, query_args,
};
pub use bakar::{PROTECTED, bakar, burnable, protected};
pub use bibit::{BibitAction, BibitArgs, bibit};
pub use browse::browse;
pub use db_job::db_job;
pub use find::{Kebun, dotenv_kebuns, find, local_kebuns, saved_profiles};
pub use list::list;
pub use nyalakan::nyalakan;
pub use query::query;
pub use sql::plan_sql;
pub use tools::{cabut, pasang, pupuk};

#[cfg(test)]
mod tests;

/// Words `sawit` takes as commands rather than database names.
pub(crate) const SUBCOMMANDS: &[&str] = &[
    "list",
    "ls",
    "bakar",
    "export",
    "ekspor",
    "panen",
    "backup",
    "dump",
    "cadangkan",
    "import",
    "impor",
    "tanam",
    "install",
    "pasang",
    "upgrade",
    "pupuk",
    "uninstall",
    "cabut",
    "start",
    "nyalakan",
    "browse",
    "intip",
    "daftar",
    "stop",
    "new",
    "semai",
    // `sawit github token`: never a database named github.
    "github",
    // `sawit skills install`: never a database named skills.
    "skills",
    "skill",
    "bibit",
];

/// Runs `fut` on a fresh Tokio runtime (the CLI runs before the app's).
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(fut)
}
