#[path = "identity.rs"]
mod identity;

pub use identity::fingerprint;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn resolve_db_path(path: Option<PathBuf>) -> Result<PathBuf> {
    let resolved = if let Some(path) = path {
        expand_tilde(path)
    } else if let Ok(path) = std::env::var("TIDYFS_DB") {
        expand_tilde(PathBuf::from(path))
    } else {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set and --db was not provided")?;
        home.join(".local/share/tidyfs/tidyfs.db")
    };

    warn_if_interrupted_actions(&resolved);
    Ok(resolved)
}

fn warn_if_interrupted_actions(database_path: &Path) {
    match interrupted_action_count(database_path) {
        Ok(0) => {}
        Ok(count) => {
            eprintln!("warning: {count} interrupted tidyfs action(s) require reconciliation");
            eprintln!(
                "run `tidyfs --db {} recover --all`",
                database_path.display()
            );
        }
        Err(err) => {
            eprintln!(
                "warning: could not inspect interrupted actions in {}: {err:#}",
                database_path.display()
            );
        }
    }
}

fn interrupted_action_count(database_path: &Path) -> Result<u64> {
    if !database_path.exists() {
        return Ok(0);
    }

    let connection = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| {
        format!(
            "opening SQLite database read-only for startup recovery check: {}",
            database_path.display()
        )
    })?;

    let actions_table_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'actions')",
        [],
        |row| row.get(0),
    )?;
    if !actions_table_exists {
        return Ok(0);
    }

    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM actions WHERE status IN ('planned', 'moving', 'restoring', 'running')",
        [],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u64)
}

pub fn normalize_existing_path(path: &Path) -> Result<PathBuf> {
    let expanded = expand_tilde(path.to_path_buf());
    expanded
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", expanded.display()))
}

pub fn normalize_path_best_effort(path: &Path) -> PathBuf {
    let expanded = expand_tilde(path.to_path_buf());
    expanded.canonicalize().unwrap_or(expanded)
}

pub fn expand_tilde(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();

    if s == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }

    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    path
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn data_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;

    Ok(home.join(".local/share/tidyfs"))
}

pub fn quarantine_root() -> Result<PathBuf> {
    Ok(data_dir()?.join("quarantine"))
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, interrupted_action_count};
    use rusqlite::Connection;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_database_path() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tidyfs-startup-recovery-{}-{nonce}.db",
            std::process::id()
        ))
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }

    #[test]
    fn counts_only_interrupted_action_states() {
        let path = temporary_database_path();
        let connection = Connection::open(&path).expect("open test database");
        connection
            .execute_batch(
                r#"
                CREATE TABLE actions(status TEXT NOT NULL);
                INSERT INTO actions(status) VALUES
                  ('planned'), ('moving'), ('restoring'), ('running'),
                  ('quarantined'), ('restored'), ('failed');
                "#,
            )
            .expect("seed action states");
        drop(connection);

        assert_eq!(
            interrupted_action_count(&path).expect("count interrupted actions"),
            4
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_database_has_no_interrupted_actions() {
        let path = temporary_database_path();
        assert_eq!(
            interrupted_action_count(&path).expect("inspect missing database"),
            0
        );
    }
}
