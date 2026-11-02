//! What Your Repositories shows: the repositories the search and the owner leave, which the
//! filter tabs count, those under the tab picked in the order the sort picks, and the owners
//! the owner menu offers.

use std::cmp::Reverse;

use sawit_core::github::Repo;

/// Which repositories the filter tabs show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::git::github) enum Filter {
    All,
    Public,
    Private,
    Forks,
    Archived,
}

impl Filter {
    pub(super) const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Public,
        Filter::Private,
        Filter::Forks,
        Filter::Archived,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Public => "Public",
            Filter::Private => "Private",
            Filter::Forks => "Forks",
            Filter::Archived => "Archived",
        }
    }

    /// Whether `repo` is under this tab.
    pub(super) fn passes(self, repo: &Repo) -> bool {
        match self {
            Filter::All => true,
            Filter::Public => !repo.private,
            Filter::Private => repo.private,
            Filter::Forks => repo.fork,
            Filter::Archived => repo.archived,
        }
    }
}

/// The order of the list. Repositories equal in it keep the order GitHub sent them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::git::github) enum Sort {
    /// Newest push first, as GitHub sends them.
    Pushed,
    /// A to Z by owner and name, whatever their case.
    Name,
    /// Most stars first; among equals, newest push first.
    Stars,
}

impl Sort {
    pub(super) const ALL: [Sort; 3] = [Sort::Pushed, Sort::Name, Sort::Stars];

    pub(super) fn label(self) -> &'static str {
        match self {
            Sort::Pushed => "Last Pushed",
            Sort::Name => "Name",
            Sort::Stars => "Stars",
        }
    }
}

/// Whether `query` (lowercase) is in `repo`'s name or description. An empty one is in every
/// repository.
fn matches(repo: &Repo, query: &str) -> bool {
    let has = |s: &str| s.to_lowercase().contains(query);
    query.is_empty() || has(&repo.full_name) || repo.description.as_deref().is_some_and(has)
}

/// The repositories whose name or description has `query` in it, whatever its case, and that
/// belong to `owner` (any owner when `None`): what the filter tabs count.
pub(super) fn in_scope<'a>(repos: &'a [Repo], query: &str, owner: Option<&str>) -> Vec<&'a Repo> {
    let query = query.trim().to_lowercase();
    repos
        .iter()
        .filter(|r| matches(r, &query) && owner.is_none_or(|o| r.owner == o))
        .collect()
}

/// Each tab with how many repositories of `scope` it holds: "All 42", "Public 9".
pub(super) fn counts(scope: &[&Repo]) -> Vec<(&'static str, usize)> {
    Filter::ALL
        .iter()
        .map(|f| (f.label(), scope.iter().filter(|r| f.passes(r)).count()))
        .collect()
}

/// What the list shows: the repositories of `scope` under `filter`, in `sort` order.
pub(super) fn shown(scope: Vec<&Repo>, filter: Filter, sort: Sort) -> Vec<&Repo> {
    let mut rows: Vec<&Repo> = scope.into_iter().filter(|r| filter.passes(r)).collect();
    match sort {
        Sort::Pushed => rows.sort_by_key(|r| Reverse(r.pushed_at)),
        Sort::Name => rows.sort_by_cached_key(|r| r.full_name.to_lowercase()),
        Sort::Stars => rows.sort_by_key(|r| (Reverse(r.stars), Reverse(r.pushed_at))),
    }
    rows
}

/// The owners the owner menu offers, each once: `me`, the account signed in, first when it owns
/// any of `repos`, then the others A to Z, whatever their case.
pub(super) fn owners(repos: &[Repo], me: Option<&str>) -> Vec<String> {
    let mine = |owner: &str| me.is_some_and(|me| owner.eq_ignore_ascii_case(me));
    let mut owners: Vec<&str> = repos
        .iter()
        .map(|r| r.owner.as_str())
        .filter(|o| !o.is_empty())
        .collect();
    owners.sort_by_cached_key(|o| (!mine(o), o.to_lowercase(), *o));
    owners.dedup();
    owners.into_iter().map(String::from).collect()
}
