use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Transaction};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScanInfo {
    pub id: i64,
    pub root_path: PathBuf,
}

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

            CREATE TABLE IF NOT EXISTS classifications (
              id INTEGER PRIMARY KEY,
              scan_id INTEGER NOT NULL,
              path TEXT NOT NULL,
              label TEXT NOT NULL,
              confidence REAL NOT NULL,
              source TEXT NOT NULL,
              reason TEXT NOT NULL,
              FOREIGN KEY(scan_id) REFERENCES scans(id)
            );

            CREATE INDEX IF NOT EXISTS idx_classifications_scan_path
              ON classifications(scan_id, path);

            CREATE INDEX IF NOT EXISTS idx_classifications_scan_label
              ON classifications(scan_id, label);
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

    pub fn resolve_scan_id(&self, scan_id: Option<i64>) -> Result<i64> {
        match scan_id {
            Some(id) => Ok(id),
            None => Ok(self.latest_completed_scan()?.id),
        }
    }

    pub fn latest_completed_scan(&self) -> Result<ScanInfo> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, root_path
            FROM scans
            WHERE status = 'completed'
            ORDER BY finished_at DESC, id DESC
            LIMIT 1
            "#,
        )?;

        let mut rows = stmt.query([])?;
        let Some(row) = rows.next()? else {
            bail!("no completed scans found; run `tidyfs scan <path>` first");
        };

        Ok(ScanInfo {
            id: row.get(0)?,
            root_path: PathBuf::from(row.get::<_, String>(1)?),
        })
    }

    pub fn get_scan(&self, scan_id: i64) -> Result<ScanInfo> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, root_path
            FROM scans
            WHERE id = ?1
            "#,
        )?;

        let scan = stmt
            .query_row(params![scan_id], |row| {
                Ok(ScanInfo {
                    id: row.get(0)?,
                    root_path: PathBuf::from(row.get::<_, String>(1)?),
                })
            })
            .with_context(|| format!("scan id {scan_id} not found"))?;

        Ok(scan)
    }
}
