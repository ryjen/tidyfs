use crate::db::Database;
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::path::PathBuf;

#[derive(Debug)]
pub struct ExplainQuery {
    pub scan_id: Option<i64>,
    pub path: PathBuf,
    pub children: bool,
}

#[derive(Debug)]
struct EntrySummary {
    path: PathBuf,
    entry_type: String,
    size_bytes: u64,
    allocated_size_bytes: u64,
}

#[derive(Debug)]
struct DirSummary {
    allocated_size_bytes: u64,
    total_size_bytes: u64,
    file_count: u64,
    dir_count: u64,
    symlink_count: u64,
}

#[derive(Debug)]
struct ClassificationRow {
    label: String,
    confidence: f64,
    source: String,
    reason: String,
}

pub fn print_explanation(database: &Database, query: ExplainQuery) -> Result<()> {
    let scan = match query.scan_id {
        Some(id) => database.get_scan(id)?,
        None => database.latest_completed_scan()?,
    };

    let requested = util::normalize_path_best_effort(&query.path);
    let path = resolve_indexed_path(database, scan.id, &requested)
        .with_context(|| format!("path not found in scan {}: {}", scan.id, requested.display()))?;

    println!("scan_id: {}", scan.id);
    println!("scan_root: {}", scan.root_path.display());
    println!("path: {}", path.display());
    println!();

    if let Some(entry) = load_entry(database, scan.id, &path)? {
        println!("entry_type: {}", entry.entry_type);
        println!("logical_size: {}", util::format_bytes(entry.size_bytes));
        println!(
            "allocated_size: {}",
            util::format_bytes(entry.allocated_size_bytes)
        );
    }

    if let Some(dir) = load_directory_summary(database, scan.id, &path)? {
        println!("directory_total: {}", util::format_bytes(dir.allocated_size_bytes));
        println!("directory_logical_total: {}", util::format_bytes(dir.total_size_bytes));
        println!("files: {}", dir.file_count);
        println!("dirs: {}", dir.dir_count);
        println!("symlinks: {}", dir.symlink_count);
    }

    let classifications = load_classifications(database, scan.id, &path)?;
    println!();

    if classifications.is_empty() {
        println!("classifications: none");
        println!("interpretation: unknown / observe-only");
    } else {
        println!("classifications:");
        for c in classifications {
            println!(
                "  - {} ({:.0}%) [{}]",
                c.label,
                c.confidence * 100.0,
                c.source
            );
            println!("    reason: {}", c.reason);
        }
    }

    if query.children {
        print_child_summary(database, scan.id, &path)?;
    }

    Ok(())
}

fn resolve_indexed_path(database: &Database, scan_id: i64, requested: &PathBuf) -> Result<PathBuf> {
    let requested_str = requested.to_string_lossy().to_string();

    let mut stmt = database.connection().prepare(
        r#"
        SELECT path
        FROM entries
        WHERE scan_id = ?1 AND path = ?2
        LIMIT 1
        "#,
    )?;

    let exact = stmt.query_row(params![scan_id, requested_str], |row| {
        Ok(PathBuf::from(row.get::<_, String>(0)?))
    });

    match exact {
        Ok(path) => Ok(path),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let mut stmt = database.connection().prepare(
                r#"
                SELECT path
                FROM directory_totals
                WHERE scan_id = ?1 AND path = ?2
                LIMIT 1
                "#,
            )?;

            stmt.query_row(params![scan_id, requested.to_string_lossy().to_string()], |row| {
                Ok(PathBuf::from(row.get::<_, String>(0)?))
            })
            .map_err(anyhow::Error::from)
        }
        Err(err) => Err(err.into()),
    }
}

fn load_entry(database: &Database, scan_id: i64, path: &PathBuf) -> Result<Option<EntrySummary>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT path, entry_type, size_bytes, allocated_size_bytes
        FROM entries
        WHERE scan_id = ?1 AND path = ?2
        LIMIT 1
        "#,
    )?;

    let result = stmt.query_row(params![scan_id, path.to_string_lossy().to_string()], |row| {
        Ok(EntrySummary {
            path: PathBuf::from(row.get::<_, String>(0)?),
            entry_type: row.get(1)?,
            size_bytes: row.get::<_, i64>(2)? as u64,
            allocated_size_bytes: row.get::<_, i64>(3)? as u64,
        })
    });

    match result {
        Ok(entry) => Ok(Some(entry)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn load_directory_summary(
    database: &Database,
    scan_id: i64,
    path: &PathBuf,
) -> Result<Option<DirSummary>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT allocated_size_bytes, total_size_bytes, file_count, dir_count, symlink_count
        FROM directory_totals
        WHERE scan_id = ?1 AND path = ?2
        LIMIT 1
        "#,
    )?;

    let result = stmt.query_row(params![scan_id, path.to_string_lossy().to_string()], |row| {
        Ok(DirSummary {
            allocated_size_bytes: row.get::<_, i64>(0)? as u64,
            total_size_bytes: row.get::<_, i64>(1)? as u64,
            file_count: row.get::<_, i64>(2)? as u64,
            dir_count: row.get::<_, i64>(3)? as u64,
            symlink_count: row.get::<_, i64>(4)? as u64,
        })
    });

    match result {
        Ok(summary) => Ok(Some(summary)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn load_classifications(
    database: &Database,
    scan_id: i64,
    path: &PathBuf,
) -> Result<Vec<ClassificationRow>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT label, confidence, source, reason
        FROM classifications
        WHERE scan_id = ?1 AND path = ?2
        ORDER BY confidence DESC, label ASC
        "#,
    )?;

    let rows = stmt
        .query_map(params![scan_id, path.to_string_lossy().to_string()], |row| {
            Ok(ClassificationRow {
                label: row.get(0)?,
                confidence: row.get(1)?,
                source: row.get(2)?,
                reason: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}

fn print_child_summary(database: &Database, scan_id: i64, path: &PathBuf) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();

    let mut stmt = database.connection().prepare(
        r#"
        SELECT c.label, COUNT(*) AS count
        FROM classifications c
        JOIN entries e
          ON e.scan_id = c.scan_id
         AND e.path = c.path
        WHERE c.scan_id = ?1
          AND e.parent_path = ?2
        GROUP BY c.label
        ORDER BY count DESC, c.label ASC
        "#,
    )?;

    let rows = stmt
        .query_map(params![scan_id, path_str], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    println!();

    if rows.is_empty() {
        println!("child_classifications: none");
    } else {
        println!("child_classifications:");
        for (label, count) in rows {
            println!("  - {:>5} {}", count, label);
        }
    }

    Ok(())
}
