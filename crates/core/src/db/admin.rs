//! Whole-object operations from the database sidebar: `pg_dump` backups,
//! loading a CSV file into a table, and dropping or emptying relations.

use std::path::Path;

use bytes::Bytes;
use futures::{SinkExt, pin_mut};
use tokio::io::AsyncReadExt;
use tokio_postgres::Client;

use super::profile::ConnProfile;
use super::query::{RelKind, Relation, SchemaInfo, sql_error};
use super::quote_ident;
use crate::binaries::PgBinaries;
use crate::{CoreError, Result};

/// What a backup covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupScope {
    Database,
    Schema(String),
    Table { schema: String, table: String },
}

/// `pg_dump`'s output format, chosen by the file name: `.dump` or
/// `.backup` is the custom format (for `pg_restore`), anything else plain SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupFormat {
    Sql,
    Custom,
}

impl BackupFormat {
    pub fn for_path(path: &Path) -> BackupFormat {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("dump") || e.eq_ignore_ascii_case("backup") => {
                BackupFormat::Custom
            }
            _ => BackupFormat::Sql,
        }
    }
}

/// A `pg_dump` pattern matching exactly this name (case and dots included).
fn dump_pattern(name: &str) -> String {
    quote_ident(name)
}

/// `pg_dump` arguments after the connection.
pub fn backup_args(scope: &BackupScope, format: BackupFormat, path: &Path) -> Vec<String> {
    let mut args = vec![
        "--no-password".to_string(),
        format!(
            "--format={}",
            match format {
                BackupFormat::Sql => "plain",
                BackupFormat::Custom => "custom",
            }
        ),
        format!("--file={}", path.display()),
    ];
    match scope {
        BackupScope::Database => {}
        BackupScope::Schema(schema) => args.push(format!("--schema={}", dump_pattern(schema))),
        BackupScope::Table { schema, table } => args.push(format!(
            "--table={}.{}",
            dump_pattern(schema),
            dump_pattern(table)
        )),
    }
    args
}

/// The server's major version (`17` for 17.2).
pub async fn server_major(client: &Client) -> Result<u32> {
    let row = client
        .query_one("SELECT current_setting('server_version_num')::int", &[])
        .await
        .map_err(sql_error)?;
    Ok((row.get::<_, i32>(0) / 10_000) as u32)
}

/// The newest `pg_dump` among `binaries` that can dump a server of `major`
/// (an older `pg_dump` refuses a newer server).
pub fn pick_pg_dump(binaries: &[PgBinaries], major: u32) -> Result<std::path::PathBuf> {
    binaries
        .iter()
        .filter(|b| b.tool("pg_dump").is_file())
        .max_by_key(|b| b.version)
        .filter(|b| b.version.major >= major)
        .map(|b| b.tool("pg_dump"))
        .ok_or_else(|| {
            CoreError::Config(format!(
                "Backing up this server needs pg_dump {major} or newer. \
                 Install PostgreSQL {major} (Instances → Download), then try again."
            ))
        })
}

/// Runs `pg_dump` for `scope` into `path` (removed again on failure).
pub async fn backup(
    client: &Client,
    profile: &ConnProfile,
    password: Option<&str>,
    binaries: &[PgBinaries],
    scope: &BackupScope,
    path: &Path,
) -> Result<()> {
    let pg_dump = pick_pg_dump(binaries, server_major(client).await?)?;
    let mut cmd = tokio::process::Command::new(pg_dump);
    cmd.arg(format!("--dbname={}", profile.connection_string()))
        .args(backup_args(scope, BackupFormat::for_path(path), path))
        .env_remove("PGPASSFILE")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(pw) = password {
        cmd.env("PGPASSWORD", pw);
    }
    let out = cmd.output().await?;
    if out.status.success() {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let message = stderr
        .lines()
        .map(|l| l.trim_start_matches("pg_dump: ").trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Err(CoreError::Sql(if message.is_empty() {
        format!("pg_dump failed ({}).", out.status)
    } else {
        message
    }))
}

/// The first record of a CSV file: its column names.
pub fn csv_header(text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            '\r' | '\n' if !quoted => break,
            c => field.push(c),
        }
    }
    fields.push(field);
    fields
}

/// Loads a CSV file (header row naming the table's columns; empty unquoted
/// fields are NULL, as the CSV export writes them) into `schema.table` with
/// COPY. One statement: every row goes in, or none. Returns the rows added.
pub async fn import_csv(client: &Client, schema: &str, table: &str, path: &Path) -> Result<u64> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut head = vec![0; 64 * 1024];
    let n = file.read(&mut head).await?;
    head.truncate(n);
    let columns = csv_header(&String::from_utf8_lossy(&head));
    if columns.iter().all(|c| c.trim().is_empty()) {
        return Err(CoreError::Parse(
            "The file is empty. Its first line must name the columns.".into(),
        ));
    }
    let list = columns
        .iter()
        .map(|c| quote_ident(c.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "COPY {}.{} ({list}) FROM STDIN WITH (FORMAT csv, HEADER true)",
        quote_ident(schema),
        quote_ident(table)
    );
    let sink = client.copy_in::<_, Bytes>(&sql).await.map_err(sql_error)?;
    pin_mut!(sink);
    sink.send(Bytes::from(head)).await.map_err(sql_error)?;
    let mut buf = vec![0; 256 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        sink.send(Bytes::copy_from_slice(&buf[..n]))
            .await
            .map_err(sql_error)?;
    }
    sink.as_mut().finish().await.map_err(sql_error)
}

fn kind_keyword(kind: RelKind) -> &'static str {
    match kind {
        RelKind::Table => "TABLE",
        RelKind::View => "VIEW",
        RelKind::MaterializedView => "MATERIALIZED VIEW",
        RelKind::Foreign => "FOREIGN TABLE",
    }
}

/// `DROP …` for a relation; `cascade` also drops what depends on it.
pub fn drop_sql(schema: &str, name: &str, kind: RelKind, cascade: bool) -> String {
    format!(
        "DROP {} {}.{}{}",
        kind_keyword(kind),
        quote_ident(schema),
        quote_ident(name),
        if cascade { " CASCADE" } else { "" }
    )
}

/// Removes every row of a table.
pub fn truncate_sql(schema: &str, table: &str) -> String {
    format!(
        "TRUNCATE TABLE {}.{}",
        quote_ident(schema),
        quote_ident(table)
    )
}

pub fn refresh_matview_sql(schema: &str, name: &str) -> String {
    format!(
        "REFRESH MATERIALIZED VIEW {}.{}",
        quote_ident(schema),
        quote_ident(name)
    )
}

/// `name` in `schema`, or, without one, in `public` or the only schema that
/// has it. Answers the schema's name with the relation.
pub fn find_relation<'a>(
    schemas: &'a [SchemaInfo],
    schema: Option<&str>,
    name: &str,
) -> Result<(&'a str, &'a Relation)> {
    let found: Vec<(&str, &Relation)> = schemas
        .iter()
        .filter(|s| schema.is_none_or(|want| want == s.name))
        .flat_map(|s| {
            s.relations
                .iter()
                .filter(|r| r.name == name)
                .map(|r| (s.name.as_str(), r))
        })
        .collect();
    match found.as_slice() {
        [] => Err(CoreError::Parse(match schema {
            Some(s) => format!("No table or view named \"{s}.{name}\"."),
            None => format!("No table or view named \"{name}\"."),
        })),
        [one] => Ok(*one),
        many => match many.iter().find(|(s, _)| *s == "public") {
            Some(public) => Ok(*public),
            None => Err(CoreError::Parse(format!(
                "\"{name}\" exists in several schemas ({}); name the schema.",
                many.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(", ")
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_header_reads_one_quoted_record() {
        assert_eq!(
            csv_header("\u{feff}id,\"full, name\",\"say \"\"hi\"\"\"\r\n1,a,b"),
            ["id", "full, name", "say \"hi\""]
        );
        assert_eq!(csv_header("\"multi\nline\",x\ny"), ["multi\nline", "x"]);
    }

    #[test]
    fn backup_format_and_scope_args() {
        let p = Path::new("/b/app.dump");
        assert_eq!(BackupFormat::for_path(p), BackupFormat::Custom);
        assert_eq!(
            BackupFormat::for_path(Path::new("a.sql")),
            BackupFormat::Sql
        );
        let args = backup_args(
            &BackupScope::Table {
                schema: "Sales".into(),
                table: "a.b".into(),
            },
            BackupFormat::Custom,
            p,
        );
        assert_eq!(
            args,
            [
                "--no-password",
                "--format=custom",
                "--file=/b/app.dump",
                "--table=\"Sales\".\"a.b\""
            ]
        );
    }

    #[test]
    fn drop_and_truncate_quote_names() {
        assert_eq!(
            drop_sql("public", "My View", RelKind::MaterializedView, true),
            "DROP MATERIALIZED VIEW \"public\".\"My View\" CASCADE"
        );
        assert_eq!(truncate_sql("s", "t"), "TRUNCATE TABLE \"s\".\"t\"");
    }

    #[test]
    fn find_relation_prefers_the_named_then_public_schema() {
        let rel = |name: &str| Relation {
            name: name.into(),
            kind: RelKind::Table,
            columns: vec![],
            readable: true,
            primary_key: vec![],
            foreign_keys: vec![],
            constraints: vec![],
            indexes: vec![],
            estimated_rows: None,
        };
        let schemas = vec![
            SchemaInfo {
                name: "public".into(),
                relations: vec![rel("users"), rel("orders")],
            },
            SchemaInfo {
                name: "sales".into(),
                relations: vec![rel("orders"), rel("leads")],
            },
            SchemaInfo {
                name: "crm".into(),
                relations: vec![rel("leads")],
            },
        ];
        assert_eq!(find_relation(&schemas, None, "users").unwrap().0, "public");
        assert_eq!(find_relation(&schemas, None, "orders").unwrap().0, "public");
        assert_eq!(
            find_relation(&schemas, Some("sales"), "orders").unwrap().0,
            "sales"
        );
        let e = find_relation(&schemas, None, "leads")
            .unwrap_err()
            .to_string();
        assert!(e.contains("sales, crm"), "{e}");
        assert!(find_relation(&schemas, Some("crm"), "users").is_err());
    }
}
