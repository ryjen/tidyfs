use crate::db::Database;
use crate::util;
use anyhow::{Context, Result};
use rusqlite::{params, Transaction};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    pub one_file_system: bool,
    pub include_pseudo: bool,
}

#[derive(Debug)]
pub struct ScanResult {
    pub scan_id: i64,
    pub entries: u64,
    pub errors: u64,
    pub total_allocated_size: u64,
}

#[derive(Debug, Default, Clone)]
struct DirAgg {
    total_size: u64,
    allocated_size: u64,
    file_count: u64,
    dir_count: u64,
    symlink_count: u64,
    max_mtime: Option<i64>,
}

#[derive(Debug)]
struct EntryRecord {
    path: PathBuf,
    parent_path: Option<PathBuf>,
    name: String,
    entry_type: EntryType,
    size_bytes: u64,
    allocated_size_bytes: u64,
    mtime: Option<i64>,
    atime: Option<i64>,
    ctime: Option<i64>,
    uid: Option<u32>,
    gid: Option<u32>,
    mode: Option<u32>,
    dev: Option<u64>,
    inode: Option<u64>,
    extension: Option<String>,
    symlink_target: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
enum EntryType {
    File,
    Directory,
    Symlink,
    Other,
}

impl EntryType {
    fn as_str(self) -> &'static str {
        match self {
            EntryType::File => "file",
            EntryType::Directory => "dir",
            EntryType::Symlink => "symlink",
            EntryType::Other => "other",
        }
    }
}

pub fn scan_path(database: &mut Database, root: &Path, options: ScanOptions) -> Result<ScanResult> {
    let started_at = util::unix_now();
    let root_dev = fs::symlink_metadata(root).ok().and_then(|m| metadata_dev(&m));

    let tx = database.transaction()?;
    let scan_id = begin_scan(&tx, root, options, started_at)?;

    let mut aggregations: HashMap<PathBuf, DirAgg> = HashMap::new();
    aggregations.entry(root.to_path_buf()).or_default();

    let mut entries = 0_u64;
    let mut errors = 0_u64;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .same_file_system(options.one_file_system)
        .into_iter()
        .filter_entry(|entry| should_descend(entry, root, options, root_dev));

    for item in walker {
        match item {
            Ok(entry) => match record_from_dir_entry(&entry) {
                Ok(record) => {
                    aggregate_record(root, &record, &mut aggregations);
                    insert_entry(&tx, scan_id, &record)?;
                    entries += 1;
                }
                Err(err) => {
                    insert_scan_error(&tx, scan_id, Some(entry.path()), &format!("{err:#}"))?;
                    errors += 1;
                }
            },
            Err(err) => {
                let path = err.path().map(Path::to_path_buf);
                insert_scan_error(&tx, scan_id, path.as_deref(), &err.to_string())?;
                errors += 1;
            }
        }
    }

    let total_allocated_size = aggregations
        .get(root)
        .map(|agg| agg.allocated_size)
        .unwrap_or_default();

    insert_directory_totals(&tx, scan_id, &aggregations)?;
    finish_scan(&tx, scan_id, util::unix_now())?;
    tx.commit()?;

    Ok(ScanResult {
        scan_id,
        entries,
        errors,
        total_allocated_size,
    })
}

fn begin_scan(tx: &Transaction<'_>, root: &Path, options: ScanOptions, started_at: i64) -> Result<i64> {
    tx.execute(
        r#"
        INSERT INTO scans(root_path, started_at, status, one_file_system, include_pseudo)
        VALUES (?1, ?2, 'running', ?3, ?4)
        "#,
        params![
            root.to_string_lossy(),
            started_at,
            options.one_file_system as i64,
            options.include_pseudo as i64,
        ],
    )?;

    Ok(tx.last_insert_rowid())
}

fn finish_scan(tx: &Transaction<'_>, scan_id: i64, finished_at: i64) -> Result<()> {
    tx.execute(
        "UPDATE scans SET finished_at = ?1, status = 'completed' WHERE id = ?2",
        params![finished_at, scan_id],
    )?;
    Ok(())
}

fn should_descend(entry: &DirEntry, root: &Path, options: ScanOptions, root_dev: Option<u64>) -> bool {
    if entry.path() == root {
        return true;
    }

    if !options.include_pseudo && is_linux_pseudo_path(entry.path()) {
        return false;
    }

    if options.one_file_system {
        if let (Some(root_dev), Ok(meta)) = (root_dev, entry.metadata()) {
            if metadata_dev(&meta) != Some(root_dev) {
                return false;
            }
        }
    }

    true
}

fn is_linux_pseudo_path(path: &Path) -> bool {
    let pseudo = ["/proc", "/sys", "/dev", "/run"];
    pseudo.iter().any(|prefix| path.starts_with(prefix))
}

fn record_from_dir_entry(entry: &DirEntry) -> Result<EntryRecord> {
    let path = entry.path().to_path_buf();
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;

    let file_type = metadata.file_type();
    let entry_type = if file_type.is_symlink() {
        EntryType::Symlink
    } else if file_type.is_dir() {
        EntryType::Directory
    } else if file_type.is_file() {
        EntryType::File
    } else {
        EntryType::Other
    };

    let symlink_target = if matches!(entry_type, EntryType::Symlink) {
        fs::read_link(&path).ok()
    } else {
        None
    };

    Ok(EntryRecord {
        parent_path: path.parent().map(Path::to_path_buf),
        name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string()),
        extension: path.extension().map(|e| e.to_string_lossy().to_string()),
        path,
        entry_type,
        size_bytes: metadata.len(),
        allocated_size_bytes: allocated_size(&metadata),
        mtime: metadata_mtime(&metadata),
        atime: metadata_atime(&metadata),
        ctime: metadata_ctime(&metadata),
        uid: metadata_uid(&metadata),
        gid: metadata_gid(&metadata),
        mode: metadata_mode(&metadata),
        dev: metadata_dev(&metadata),
        inode: metadata_inode(&metadata),
        symlink_target,
    })
}

fn aggregate_record(root: &Path, record: &EntryRecord, aggregations: &mut HashMap<PathBuf, DirAgg>) {
    let mut current = Some(record.path.as_path());

    while let Some(path) = current {
        if path.starts_with(root) {
            let agg = aggregations.entry(path.to_path_buf()).or_default();
            agg.total_size = agg.total_size.saturating_add(record.size_bytes);
            agg.allocated_size = agg.allocated_size.saturating_add(record.allocated_size_bytes);

            match record.entry_type {
                EntryType::File => agg.file_count += 1,
                EntryType::Directory => {
                    if record.path != path {
                        agg.dir_count += 1;
                    }
                }
                EntryType::Symlink => agg.symlink_count += 1,
                EntryType::Other => {}
            }

            agg.max_mtime = match (agg.max_mtime, record.mtime) {
                (Some(existing), Some(newer)) => Some(existing.max(newer)),
                (None, Some(newer)) => Some(newer),
                (existing, None) => existing,
            };
        }

        if path == root {
            break;
        }

        current = path.parent();
    }
}

fn insert_entry(tx: &Transaction<'_>, scan_id: i64, record: &EntryRecord) -> Result<()> {
    tx.execute(
        r#"
        INSERT INTO entries(
          scan_id, path, parent_path, name, entry_type,
          size_bytes, allocated_size_bytes,
          mtime, atime, ctime,
          uid, gid, mode, dev, inode,
          extension, symlink_target
        )
        VALUES (
          ?1, ?2, ?3, ?4, ?5,
          ?6, ?7,
          ?8, ?9, ?10,
          ?11, ?12, ?13, ?14, ?15,
          ?16, ?17
        )
        "#,
        params![
            scan_id,
            record.path.to_string_lossy(),
            record.parent_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            record.name,
            record.entry_type.as_str(),
            record.size_bytes as i64,
            record.allocated_size_bytes as i64,
            record.mtime,
            record.atime,
            record.ctime,
            record.uid.map(i64::from),
            record.gid.map(i64::from),
            record.mode.map(i64::from),
            record.dev.map(|v| v as i64),
            record.inode.map(|v| v as i64),
            record.extension,
            record.symlink_target.as_ref().map(|p| p.to_string_lossy().to_string()),
        ],
    )?;

    Ok(())
}

fn insert_directory_totals(tx: &Transaction<'_>, scan_id: i64, aggregations: &HashMap<PathBuf, DirAgg>) -> Result<()> {
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO directory_totals(
          scan_id, path, total_size_bytes, allocated_size_bytes,
          file_count, dir_count, symlink_count, max_mtime
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
    )?;

    for (path, agg) in aggregations {
        stmt.execute(params![
            scan_id,
            path.to_string_lossy(),
            agg.total_size as i64,
            agg.allocated_size as i64,
            agg.file_count as i64,
            agg.dir_count as i64,
            agg.symlink_count as i64,
            agg.max_mtime,
        ])?;
    }

    Ok(())
}

fn insert_scan_error(tx: &Transaction<'_>, scan_id: i64, path: Option<&Path>, error: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO scan_errors(scan_id, path, error) VALUES (?1, ?2, ?3)",
        params![scan_id, path.map(|p| p.to_string_lossy().to_string()), error],
    )?;

    Ok(())
}

#[cfg(unix)]
fn allocated_size(metadata: &fs::Metadata) -> u64 {
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_size(metadata: &fs::Metadata) -> u64 {
    metadata.len()
}

#[cfg(unix)]
fn metadata_mtime(metadata: &fs::Metadata) -> Option<i64> { Some(metadata.mtime()) }
#[cfg(not(unix))]
fn metadata_mtime(metadata: &fs::Metadata) -> Option<i64> { metadata.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64) }

#[cfg(unix)]
fn metadata_atime(metadata: &fs::Metadata) -> Option<i64> { Some(metadata.atime()) }
#[cfg(not(unix))]
fn metadata_atime(_metadata: &fs::Metadata) -> Option<i64> { None }

#[cfg(unix)]
fn metadata_ctime(metadata: &fs::Metadata) -> Option<i64> { Some(metadata.ctime()) }
#[cfg(not(unix))]
fn metadata_ctime(_metadata: &fs::Metadata) -> Option<i64> { None }

#[cfg(unix)]
fn metadata_uid(metadata: &fs::Metadata) -> Option<u32> { Some(metadata.uid()) }
#[cfg(not(unix))]
fn metadata_uid(_metadata: &fs::Metadata) -> Option<u32> { None }

#[cfg(unix)]
fn metadata_gid(metadata: &fs::Metadata) -> Option<u32> { Some(metadata.gid()) }
#[cfg(not(unix))]
fn metadata_gid(_metadata: &fs::Metadata) -> Option<u32> { None }

#[cfg(unix)]
fn metadata_mode(metadata: &fs::Metadata) -> Option<u32> { Some(metadata.mode()) }
#[cfg(not(unix))]
fn metadata_mode(_metadata: &fs::Metadata) -> Option<u32> { None }

#[cfg(unix)]
fn metadata_dev(metadata: &fs::Metadata) -> Option<u64> { Some(metadata.dev()) }
#[cfg(not(unix))]
fn metadata_dev(_metadata: &fs::Metadata) -> Option<u64> { None }

#[cfg(unix)]
fn metadata_inode(metadata: &fs::Metadata) -> Option<u64> { Some(metadata.ino()) }
#[cfg(not(unix))]
fn metadata_inode(_metadata: &fs::Metadata) -> Option<u64> { None }
