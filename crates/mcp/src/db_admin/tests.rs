//! Export and backup files: absolute, new unless replaced, in a folder that is there, and never a
//! canvas file; and what an export says it did.

use super::*;

#[test]
fn output_file_must_be_absolute_new_and_in_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.csv");
    assert_eq!(
        output_file("export_table", file.to_str().unwrap(), false).unwrap(),
        file
    );
    assert!(
        output_file("export_table", "a.csv", false)
            .unwrap_err()
            .contains("absolute")
    );
    let missing = dir.path().join("nope/a.csv");
    assert!(
        output_file("export_table", missing.to_str().unwrap(), false)
            .unwrap_err()
            .contains("does not exist")
    );
    std::fs::write(&file, "x").unwrap();
    assert!(
        output_file("export_table", file.to_str().unwrap(), false)
            .unwrap_err()
            .contains("overwrite=true")
    );
    assert!(output_file("export_table", file.to_str().unwrap(), true).is_ok());
    assert!(
        output_file("export_table", dir.path().to_str().unwrap(), true)
            .unwrap_err()
            .contains("folder")
    );
}

/// A canvas file is never a table's, a query's or a backup's file, overwrite or not, and the
/// tool says so before it connects to anything: the board is as it was. Another file in the
/// same folder is still one to write (writing it takes a database).
#[tokio::test]
async fn no_export_or_backup_writes_over_a_canvas_file() {
    let dir = tempfile::tempdir().unwrap();
    let board = dir.path().join(".nyawit").join("canvas.sawit");
    std::fs::create_dir_all(board.parent().unwrap()).unwrap();
    std::fs::write(&board, b"the board").unwrap();
    let target = board.display().to_string();
    let refused = format!("would overwrite a canvas file: {target}");
    for overwrite in [false, true] {
        let said = output_file("backup_database", &target, overwrite).unwrap_err();
        assert_eq!(said, format!("backup_database {refused}"));
    }
    let server = Sawit::new(sawit_core::instances::AppPaths {
        root: dir.path().join("home"),
    });
    let results = [
        server
            .export_table_tool(ExportTableArgs {
                target: "nowhere".into(),
                database: None,
                schema: None,
                table: "users".into(),
                path: target.clone(),
                overwrite: Some(true),
            })
            .await,
        server
            .export_query_tool(ExportQueryArgs {
                target: "nowhere".into(),
                database: None,
                sql: "select 1".into(),
                path: target.clone(),
                overwrite: Some(true),
            })
            .await,
        server
            .backup_tool(BackupArgs {
                target: "nowhere".into(),
                database: None,
                schema: None,
                table: None,
                path: target.clone(),
                overwrite: Some(true),
            })
            .await,
    ];
    for (result, tool) in results
        .iter()
        .zip(["export_table", "export_query", "backup_database"])
    {
        assert_eq!(result.is_error, Some(true));
        let text = &result.content[0].as_text().unwrap().text;
        assert_eq!(text, &format!("{tool} {refused}"));
    }
    assert_eq!(std::fs::read(&board).unwrap(), b"the board");
    let rows = dir.path().join(".nyawit").join("canvas.csv");
    assert_eq!(
        output_file("export_query", rows.to_str().unwrap(), false),
        Ok(rows)
    );
}

#[test]
fn export_summary_mentions_truncation() {
    let path = std::path::Path::new("/x/a.xlsx");
    assert_eq!(
        exported(
            ExportSummary {
                rows: 1,
                truncated: false
            },
            path
        ),
        "Exported 1 row to /x/a.xlsx."
    );
    assert!(
        exported(
            ExportSummary {
                rows: 9,
                truncated: true
            },
            path
        )
        .contains("Truncated")
    );
}
