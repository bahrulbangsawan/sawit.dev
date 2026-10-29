//! What the table browser shows of a relation: an optional sort column, AND-ed
//! column filters and a whole-row text search, turned into SQL clauses.
//! Identifiers are quoted and values become literals, never raw SQL.

use super::quote_ident;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    NotContains,
    StartsWith,
    IsNull,
    IsNotNull,
}

impl FilterOp {
    pub const ALL: [FilterOp; 11] = [
        FilterOp::Eq,
        FilterOp::Ne,
        FilterOp::Contains,
        FilterOp::NotContains,
        FilterOp::StartsWith,
        FilterOp::Gt,
        FilterOp::Ge,
        FilterOp::Lt,
        FilterOp::Le,
        FilterOp::IsNull,
        FilterOp::IsNotNull,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterOp::Eq => "=",
            FilterOp::Ne => "≠",
            FilterOp::Gt => ">",
            FilterOp::Ge => "≥",
            FilterOp::Lt => "<",
            FilterOp::Le => "≤",
            FilterOp::Contains => "contains",
            FilterOp::NotContains => "doesn't contain",
            FilterOp::StartsWith => "starts with",
            FilterOp::IsNull => "is null",
            FilterOp::IsNotNull => "is not null",
        }
    }

    pub fn needs_value(self) -> bool {
        !matches!(self, FilterOp::IsNull | FilterOp::IsNotNull)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFilter {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowQuery {
    pub sort: Option<(String, SortDir)>,
    pub filters: Vec<RowFilter>,
    /// Matched, ignoring case, against the whole row as text.
    pub search: String,
}

/// A string literal that reads the same whatever `standard_conforming_strings` is.
pub fn quote_literal(value: &str) -> String {
    let quoted = value.replace('\'', "''");
    if value.contains('\\') {
        format!("E'{}'", quoted.replace('\\', "\\\\"))
    } else {
        format!("'{quoted}'")
    }
}

/// A LIKE pattern matching `value` literally, for `ESCAPE '!'`.
fn like_literal(prefix: &str, value: &str, suffix: &str) -> String {
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '!' | '%' | '_' => vec!['!', c],
            c => vec![c],
        })
        .collect();
    quote_literal(&format!("{prefix}{escaped}{suffix}"))
}

impl RowQuery {
    /// Rows are narrowed, so the planner's estimate no longer counts them.
    pub fn is_filtered(&self) -> bool {
        !self.filters.is_empty() || !self.search.trim().is_empty()
    }

    /// The `WHERE` condition over `alias` (the relation's alias in FROM), if any.
    pub fn where_sql(&self, alias: &str) -> Option<String> {
        let mut parts: Vec<String> = self
            .filters
            .iter()
            .map(|f| {
                let col = format!("{alias}.{}", quote_ident(&f.column));
                let v = || quote_literal(&f.value);
                match f.op {
                    // Untyped literals take the column's type, so `id = '7'`
                    // compares numbers, not text.
                    FilterOp::Eq => format!("{col} = {}", v()),
                    FilterOp::Ne => format!("{col} IS DISTINCT FROM {}", v()),
                    FilterOp::Gt => format!("{col} > {}", v()),
                    FilterOp::Ge => format!("{col} >= {}", v()),
                    FilterOp::Lt => format!("{col} < {}", v()),
                    FilterOp::Le => format!("{col} <= {}", v()),
                    FilterOp::Contains => format!(
                        "{col}::text ILIKE {} ESCAPE '!'",
                        like_literal("%", &f.value, "%")
                    ),
                    FilterOp::NotContains => format!(
                        "({col} IS NULL OR {col}::text NOT ILIKE {} ESCAPE '!')",
                        like_literal("%", &f.value, "%")
                    ),
                    FilterOp::StartsWith => format!(
                        "{col}::text ILIKE {} ESCAPE '!'",
                        like_literal("", &f.value, "%")
                    ),
                    FilterOp::IsNull => format!("{col} IS NULL"),
                    FilterOp::IsNotNull => format!("{col} IS NOT NULL"),
                }
            })
            .collect();
        let search = self.search.trim();
        if !search.is_empty() {
            parts.push(format!(
                "{alias}::text ILIKE {} ESCAPE '!'",
                like_literal("%", search, "%")
            ));
        }
        (!parts.is_empty()).then(|| parts.join(" AND "))
    }

    /// The ORDER BY list: the sort column first (NULLs last either way),
    /// then `key` so equal values keep a stable order across pages.
    pub fn order_sql(&self, alias: &str, key: &[String]) -> Option<String> {
        let mut cols: Vec<String> = Vec::new();
        if let Some((column, dir)) = &self.sort {
            cols.push(format!(
                "{alias}.{} {} NULLS LAST",
                quote_ident(column),
                match dir {
                    SortDir::Asc => "ASC",
                    SortDir::Desc => "DESC",
                }
            ));
        }
        cols.extend(
            key.iter()
                .filter(|k| self.sort.as_ref().is_none_or(|(c, _)| c != *k))
                .map(|k| format!("{alias}.{}", quote_ident(k))),
        );
        (!cols.is_empty()).then(|| cols.join(", "))
    }
}
