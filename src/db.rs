use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction};
use std::path::Path;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating db directory {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite database {}", path.display()))?;

        Ok(Self { conn })
    }

    pub fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS scans (
              id INTEGER PRIMARY KEY,
              root_path TEXT NOT NULL,
              started_at INTEGER NOT NULL,
              finished_at INTEGER,
              status TEXT NOT NULL,
              one_file_system INTEGER NOT NULL DEFAULT 0,
              include_pseudo INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS entries (
              id INTEGER PRIMARY KEY,
              scan_id INTEGER NOT NULL,
              path TEXT NOT NULL,
              parent_path TEXT,
              name TEXT NOT NULL,
              entry_type TEXT NOT NULL,
              size_bytes INTEGER NOT NULL,
              allocated_size_bytes INTEGER NOT NULL,
              mtime INTEGER,
              atime INTEGER,
              ctime INTEGER,
              uid INTEGER,
              gid INTEGER,
              mode INTEGER,
              dev INTEGER,
              inode INTEGER,
              extension TEXT,
              symlink_target TEXT,
              FOREIGN KEY(scan_id) REFERENCES scans(id)
            );

            CREATE INDEX IF NOT EXISTS idx_entries_scan_path
              ON entries(scan_id, path);

            CREATE INDEX IF NOT EXISTS idx_entries_scan_parent
              ON entries(scan_id, parent_path);

            CREATE INDEX IF NOT EXISTS idx_entries_scan_size
              ON entries(scan_id, allocated_size_bytes DESC);

            CREATE TABLE IF NOT EXISTS directory_totals (
              scan_id INTEGER NOT NULL,
              path TEXT NOT NULL,
              total_size_bytes INTEGER NOT NULL,
              allocated_size_bytes INTEGER NOT NULL,
              file_count INTEGER NOT NULL,
              dir_count INTEGER NOT NULL,
              symlink_count INTEGER NOT NULL,
              max_mtime INTEGER,
              PRIMARY KEY(scan_id, path),
              FOREIGN KEY(scan_id) REFERENCES scans(id)
            );

            CREATE INDEX IF NOT EXISTS idx_directory_totals_size
              ON directory_totals(scan_id, allocated_size_bytes DESC);

            CREATE TABLE IF NOT EXISTS scan_errors (
              id INTEGER PRIMARY KEY,
              scan_id INTEGER NOT NULL,
              path TEXT,
              error TEXT NOT NULL,
              FOREIGN KEY(scan_id) REFERENCES scans(id)
            );
            "#,
        )?;

        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }
}
