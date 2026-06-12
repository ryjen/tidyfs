use crate::db::Database;
use crate::util;
use anyhow::Result;
use rusqlite::params;
use std::path::PathBuf;

#[derive(Debug)]
pub struct ActionsQuery {
    pub limit: usize,
}

#[derive(Debug)]
struct ActionRow {
    id: i64,
    timestamp: i64,
    original_path: PathBuf,
    quarantine_path: Option<PathBuf>,
    action_type: String,
    size_bytes: Option<u64>,
    rule_id: Option<String>,
    risk: Option<String>,
    status: String,
    restored_at: Option<i64>,
}

pub fn print_actions(database: &Database, query: ActionsQuery) -> Result<()> {
    let rows = load_actions(database, query.limit)?;

    println!("Actions:");
    if rows.is_empty() {
        println!("  none");
        return Ok(());
    }

    for row in rows {
        println!(
            "  #{:<5} {:>10}  {}",
            row.id,
            row.size_bytes
                .map(util::format_bytes)
                .unwrap_or_else(|| "-".to_string()),
            row.original_path.display()
        );
        println!("         status: {}", row.status);
        println!("         action: {}", row.action_type);
        println!("         timestamp: {}", row.timestamp);
        if let Some(rule_id) = row.rule_id {
            println!("         rule: {}", rule_id);
        }
        if let Some(risk) = row.risk {
            println!("         risk: {}", risk);
        }
        if let Some(path) = row.quarantine_path {
            println!("         quarantine: {}", path.display());
        }
        if let Some(restored_at) = row.restored_at {
            println!("         restored_at: {}", restored_at);
        }
    }

    Ok(())
}

fn load_actions(database: &Database, limit: usize) -> Result<Vec<ActionRow>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT
          id,
          timestamp,
          original_path,
          quarantine_path,
          action_type,
          size_bytes,
          rule_id,
          risk,
          status,
          restored_at
        FROM actions
        ORDER BY timestamp DESC, id DESC
        LIMIT ?1
        "#,
    )?;

    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok(ActionRow {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                original_path: PathBuf::from(row.get::<_, String>(2)?),
                quarantine_path: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
                action_type: row.get(4)?,
                size_bytes: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
                rule_id: row.get(6)?,
                risk: row.get(7)?,
                status: row.get(8)?,
                restored_at: row.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}
