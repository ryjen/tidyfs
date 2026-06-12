use crate::db::Database;
use crate::util;
use anyhow::Result;
use rusqlite::params;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct TopQuery {
    pub scan_id: Option<i64>,
    pub limit: usize,
    pub depth: Option<usize>,
    pub root: Option<PathBuf>,
}

#[derive(Debug)]
struct TopRow {
    path: PathBuf,
    allocated_size: u64,
    total_size: u64,
    file_count: u64,
    dir_count: u64,
    symlink_count: u64,
}

pub fn print_top(database: &Database, query: TopQuery) -> Result<()> {
    let scan = match query.scan_id {
        Some(id) => database.get_scan(id)?,
        None => database.latest_completed_scan()?,
    };

    let rows = load_rows(database, scan.id)?;
    let root_filter = query
        .root
        .as_ref()
        .map(|p| util::normalize_path_best_effort(p));

    let mut filtered: Vec<_> = rows
        .into_iter()
        .filter(|row| match &root_filter {
            Some(root) => row.path.starts_with(root),
            None => true,
        })
        .filter(|row| match query.depth {
            Some(max_depth) => relative_depth(&scan.root_path, &row.path)
                .map(|depth| depth <= max_depth)
                .unwrap_or(false),
            None => true,
        })
        .collect();

    filtered.sort_by(|a, b| b.allocated_size.cmp(&a.allocated_size));
    filtered.truncate(query.limit);

    println!("scan_id: {}", scan.id);
    println!("scan_root: {}", scan.root_path.display());
    if let Some(root) = &root_filter {
        println!("filter_root: {}", root.display());
    }
    println!();

    println!(
        "{:>12} {:>12} {:>8} {:>8} {:>8}  {}",
        "ALLOCATED", "LOGICAL", "FILES", "DIRS", "LINKS", "PATH"
    );

    for row in filtered {
        println!(
            "{:>12} {:>12} {:>8} {:>8} {:>8}  {}",
            util::format_bytes(row.allocated_size),
            util::format_bytes(row.total_size),
            row.file_count,
            row.dir_count,
            row.symlink_count,
            row.path.display()
        );
    }

    Ok(())
}

fn load_rows(database: &Database, scan_id: i64) -> Result<Vec<TopRow>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT path, allocated_size_bytes, total_size_bytes, file_count, dir_count, symlink_count
        FROM directory_totals
        WHERE scan_id = ?1
        "#,
    )?;

    let rows = stmt
        .query_map(params![scan_id], |row| {
            Ok(TopRow {
                path: PathBuf::from(row.get::<_, String>(0)?),
                allocated_size: row.get::<_, i64>(1)? as u64,
                total_size: row.get::<_, i64>(2)? as u64,
                file_count: row.get::<_, i64>(3)? as u64,
                dir_count: row.get::<_, i64>(4)? as u64,
                symlink_count: row.get::<_, i64>(5)? as u64,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}

fn relative_depth(root: &Path, path: &Path) -> Option<usize> {
    let rel = path.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        Some(0)
    } else {
        Some(rel.components().count())
    }
}
