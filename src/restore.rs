use crate::db::Database;
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
pub struct RestoreQuery {
    pub action_id: Option<i64>,
    pub latest: bool,
}

#[derive(Debug)]
struct RestoreAction {
    id: i64,
    original_path: PathBuf,
    quarantine_path: PathBuf,
    status: String,
    restored_at: Option<i64>,
}

pub fn run_restore(database: &Database, query: RestoreQuery) -> Result<()> {
    let action = match (query.action_id, query.latest) {
        (Some(id), false) => load_action(database, id)?,
        (None, true) => load_latest_action(database)?,
        (Some(_), true) => bail!("use either --action or --latest, not both"),
        (None, false) => bail!("restore requires --action <id> or --latest"),
    };

    if action.status != "quarantined" {
        bail!(
            "action {} is not restorable; status={}",
            action.id,
            action.status
        );
    }

    if action.restored_at.is_some() {
        bail!("action {} was already restored", action.id);
    }

    if !action.quarantine_path.exists() {
        bail!(
            "quarantine payload does not exist: {}",
            action.quarantine_path.display()
        );
    }

    if action.original_path.exists() {
        bail!(
            "refusing to overwrite existing destination: {}",
            action.original_path.display()
        );
    }

    set_restore_status(database, action.id, "restoring", None)?;

    if let Some(parent) = action.original_path.parent() {
        if let Err(err) = fs::create_dir_all(parent)
            .with_context(|| format!("creating restore parent {}", parent.display()))
        {
            record_restore_failure(database, action.id, &err);
            return Err(err);
        }
    }

    if let Err(err) = fs::rename(&action.quarantine_path, &action.original_path).with_context(|| {
        format!(
            "moving {} back to {}",
            action.quarantine_path.display(),
            action.original_path.display()
        )
    }) {
        record_restore_failure(database, action.id, &err);
        return Err(err);
    }

    database.connection().execute(
        r#"
        UPDATE actions
        SET status = 'restored',
            restored_at = ?1,
            error = NULL,
            restore_error = NULL
        WHERE id = ?2
        "#,
        params![util::unix_now(), action.id],
    )?;

    println!("restored action_id={}", action.id);
    println!("path: {}", action.original_path.display());

    Ok(())
}

fn set_restore_status(
    database: &Database,
    action_id: i64,
    status: &str,
    restore_error: Option<&str>,
) -> Result<()> {
    database.connection().execute(
        r#"
        UPDATE actions
        SET status = ?1,
            restore_error = ?2
        WHERE id = ?3
        "#,
        params![status, restore_error, action_id],
    )?;
    Ok(())
}

fn record_restore_failure(database: &Database, action_id: i64, error: &anyhow::Error) {
    let message = format!("{error:#}");
    let _ = set_restore_status(database, action_id, "quarantined", Some(&message));
}

fn load_latest_action(database: &Database) -> Result<RestoreAction> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT id, original_path, quarantine_path, status, restored_at
        FROM actions
        WHERE status = 'quarantined'
          AND quarantine_path IS NOT NULL
        ORDER BY timestamp DESC, id DESC
        LIMIT 1
        "#,
    )?;

    stmt.query_row([], row_to_action)
        .context("no restorable quarantined action found")
}

fn load_action(database: &Database, action_id: i64) -> Result<RestoreAction> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT id, original_path, quarantine_path, status, restored_at
        FROM actions
        WHERE id = ?1
        "#,
    )?;

    stmt.query_row(params![action_id], row_to_action)
        .with_context(|| format!("action {} not found", action_id))
}

fn row_to_action(row: &rusqlite::Row<'_>) -> rusqlite::Result<RestoreAction> {
    Ok(RestoreAction {
        id: row.get(0)?,
        original_path: PathBuf::from(row.get::<_, String>(1)?),
        quarantine_path: PathBuf::from(row.get::<_, String>(2)?),
        status: row.get(3)?,
        restored_at: row.get(4)?,
    })
}
