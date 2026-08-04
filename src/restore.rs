use crate::db::Database;
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::fs;
use std::path::{Path, PathBuf};

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

    if let Err(err) = atomic_rename_noreplace(&action.quarantine_path, &action.original_path)
        .with_context(|| {
            format!(
                "moving {} back to {} without replacement",
                action.quarantine_path.display(),
                action.original_path.display()
            )
        })
    {
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

#[cfg(target_os = "linux")]
fn atomic_rename_noreplace(source: &Path, destination: &Path) -> Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt;

    const AT_FDCWD: c_int = -100;
    const RENAME_NOREPLACE: c_uint = 1;

    unsafe extern "C" {
        fn renameat2(
            olddirfd: c_int,
            oldpath: *const c_char,
            newdirfd: c_int,
            newpath: *const c_char,
            flags: c_uint,
        ) -> c_int;
    }

    let source = CString::new(source.as_os_str().as_bytes())
        .context("source path contains an interior NUL byte")?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .context("destination path contains an interior NUL byte")?;

    // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
    let result = unsafe {
        renameat2(
            AT_FDCWD,
            source.as_ptr(),
            AT_FDCWD,
            destination.as_ptr(),
            RENAME_NOREPLACE,
        )
    };

    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(not(target_os = "linux"))]
fn atomic_rename_noreplace(_source: &Path, _destination: &Path) -> Result<()> {
    bail!("atomic no-replace restore is not supported on this platform")
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::atomic_rename_noreplace;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sandbox(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tidyfs-atomic-restore-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create sandbox");
        root
    }

    #[test]
    fn atomic_restore_moves_when_destination_is_absent() {
        let root = sandbox("success");
        let source = root.join("payload");
        let destination = root.join("restored");
        fs::write(&source, b"payload").expect("write source");

        atomic_rename_noreplace(&source, &destination).expect("atomic restore");

        assert!(!source.exists());
        assert_eq!(fs::read(&destination).expect("read destination"), b"payload");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_restore_never_replaces_racing_destination() {
        let root = sandbox("collision");
        let source = root.join("payload");
        let destination = root.join("restored");
        fs::write(&source, b"quarantined").expect("write source");
        fs::write(&destination, b"external-writer").expect("write destination");

        atomic_rename_noreplace(&source, &destination)
            .expect_err("existing destination must reject atomic restore");

        assert_eq!(fs::read(&source).expect("read source"), b"quarantined");
        assert_eq!(
            fs::read(&destination).expect("read destination"),
            b"external-writer"
        );
        let _ = fs::remove_dir_all(root);
    }
}
