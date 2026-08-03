use crate::db::Database;
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct RecoverQuery {
    pub action_id: Option<i64>,
    pub all: bool,
}

#[derive(Debug)]
struct RecoveryAction {
    id: i64,
    original_path: PathBuf,
    quarantine_path: Option<PathBuf>,
    status: String,
}

pub fn run_recover(database: &Database, query: RecoverQuery) -> Result<()> {
    let actions = match (query.action_id, query.all) {
        (Some(id), false) => vec![load_action(database, id)?],
        (None, true) => load_interrupted_actions(database)?,
        (Some(_), true) => bail!("use either --action or --all, not both"),
        (None, false) => bail!("recover requires --action <id> or --all"),
    };

    if actions.is_empty() {
        println!("No interrupted actions found.");
        return Ok(());
    }

    let mut recovered = 0_u64;
    let mut failed = 0_u64;

    for action in actions {
        let status = reconcile_action(database, &action)?;
        if status == "failed" {
            failed += 1;
        } else {
            recovered += 1;
        }
        println!("action_id={} status={status}", action.id);
    }

    println!();
    println!("recovery completed:");
    println!("  reconciled: {recovered}");
    println!("  terminal_failures: {failed}");
    println!("  filesystem_writes: 0");

    Ok(())
}

fn reconcile_action(database: &Database, action: &RecoveryAction) -> Result<&'static str> {
    if !matches!(action.status.as_str(), "planned" | "moving" | "restoring") {
        bail!(
            "action {} is not interrupted; status={}",
            action.id,
            action.status
        );
    }

    let Some(quarantine_path) = action.quarantine_path.as_ref() else {
        mark_failed(database, action.id, "interrupted action has no quarantine path")?;
        return Ok("failed");
    };

    let original_exists = path_entry_exists(&action.original_path).with_context(|| {
        format!(
            "inspecting original path for action {}: {}",
            action.id,
            action.original_path.display()
        )
    })?;
    let quarantine_exists = path_entry_exists(quarantine_path).with_context(|| {
        format!(
            "inspecting quarantine path for action {}: {}",
            action.id,
            quarantine_path.display()
        )
    })?;

    match action.status.as_str() {
        "planned" | "moving" => reconcile_quarantine(
            database,
            action,
            original_exists,
            quarantine_exists,
        ),
        "restoring" => reconcile_restore(
            database,
            action,
            original_exists,
            quarantine_exists,
        ),
        _ => unreachable!(),
    }
}

fn reconcile_quarantine(
    database: &Database,
    action: &RecoveryAction,
    original_exists: bool,
    quarantine_exists: bool,
) -> Result<&'static str> {
    match (original_exists, quarantine_exists) {
        (false, true) => {
            database.connection().execute(
                r#"
                UPDATE actions
                SET status = 'quarantined',
                    error = NULL
                WHERE id = ?1
                "#,
                params![action.id],
            )?;
            Ok("quarantined")
        }
        (true, false) => {
            mark_failed(
                database,
                action.id,
                "quarantine move did not complete; original path still exists",
            )?;
            Ok("failed")
        }
        (true, true) => {
            mark_failed(
                database,
                action.id,
                "both original and quarantine paths exist; refusing to guess",
            )?;
            Ok("failed")
        }
        (false, false) => {
            mark_failed(
                database,
                action.id,
                "neither original nor quarantine path exists",
            )?;
            Ok("failed")
        }
    }
}

fn reconcile_restore(
    database: &Database,
    action: &RecoveryAction,
    original_exists: bool,
    quarantine_exists: bool,
) -> Result<&'static str> {
    match (original_exists, quarantine_exists) {
        (true, false) => {
            database.connection().execute(
                r#"
                UPDATE actions
                SET status = 'restored',
                    restored_at = COALESCE(restored_at, ?1),
                    error = NULL,
                    restore_error = NULL
                WHERE id = ?2
                "#,
                params![util::unix_now(), action.id],
            )?;
            Ok("restored")
        }
        (false, true) => {
            database.connection().execute(
                r#"
                UPDATE actions
                SET status = 'quarantined',
                    restore_error = 'interrupted restore did not move payload'
                WHERE id = ?1
                "#,
                params![action.id],
            )?;
            Ok("quarantined")
        }
        (true, true) => {
            mark_failed(
                database,
                action.id,
                "both restore destination and quarantine payload exist; refusing to guess",
            )?;
            Ok("failed")
        }
        (false, false) => {
            mark_failed(
                database,
                action.id,
                "neither restore destination nor quarantine payload exists",
            )?;
            Ok("failed")
        }
    }
}

fn mark_failed(database: &Database, action_id: i64, error: &str) -> Result<()> {
    database.connection().execute(
        r#"
        UPDATE actions
        SET status = 'failed',
            error = ?1
        WHERE id = ?2
        "#,
        params![error, action_id],
    )?;
    Ok(())
}

fn path_entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn load_interrupted_actions(database: &Database) -> Result<Vec<RecoveryAction>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT id, original_path, quarantine_path, status
        FROM actions
        WHERE status IN ('planned', 'moving', 'restoring')
        ORDER BY timestamp ASC, id ASC
        "#,
    )?;

    let rows = stmt
        .query_map([], row_to_action)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn load_action(database: &Database, action_id: i64) -> Result<RecoveryAction> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT id, original_path, quarantine_path, status
        FROM actions
        WHERE id = ?1
        "#,
    )?;

    stmt.query_row(params![action_id], row_to_action)
        .with_context(|| format!("action {action_id} not found"))
}

fn row_to_action(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecoveryAction> {
    Ok(RecoveryAction {
        id: row.get(0)?,
        original_path: PathBuf::from(row.get::<_, String>(1)?),
        quarantine_path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
        status: row.get(3)?,
    })
}
