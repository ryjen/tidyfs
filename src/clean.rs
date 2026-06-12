use crate::db::Database;
use crate::rules::{self, Risk};
use crate::util;
use anyhow::Result;
use rusqlite::params;
use std::path::PathBuf;

#[derive(Debug)]
pub struct CleanQuery {
    pub scan_id: Option<i64>,
    pub dry_run: bool,
    pub max_risk: Risk,
    pub root: Option<PathBuf>,
    pub limit: usize,
}

#[derive(Debug)]
struct Candidate {
    id: i64,
    path: PathBuf,
    size_bytes: u64,
    rule_id: String,
    rule_label: String,
    category: String,
    risk: Risk,
    action_type: String,
    reversible: bool,
    reason: String,
}

pub fn run_clean(database: &Database, query: CleanQuery) -> Result<()> {
    if !query.dry_run {
        anyhow::bail!("real cleanup is not implemented yet; use --dry-run");
    }

    let scan = match query.scan_id {
        Some(id) => database.get_scan(id)?,
        None => database.latest_completed_scan()?,
    };

    let root_filter = query
        .root
        .as_ref()
        .map(|p| util::normalize_path_best_effort(p));

    let mut candidates = load_allowed_candidates(database, scan.id)?;

    candidates.retain(|candidate| {
        rules::risk_allows(candidate.risk, query.max_risk)
            && root_filter
                .as_ref()
                .map(|root| candidate.path.starts_with(root))
                .unwrap_or(true)
    });

    candidates.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| a.path.cmp(&b.path))
    });

    let total_bytes: u64 = candidates.iter().map(|c| c.size_bytes).sum();

    println!("Dry-run cleanup preview");
    println!();
    println!("scan_id: {}", scan.id);
    println!("scan_root: {}", scan.root_path.display());
    println!("risk_threshold: {}", query.max_risk);
    if let Some(root) = &root_filter {
        println!("filter_root: {}", root.display());
    }
    println!("candidate_count: {}", candidates.len());
    println!("potential_reclaimable: {}", util::format_bytes(total_bytes));
    println!();

    if candidates.is_empty() {
        println!("No allowed cleanup candidates found.");
        println!();
        println!("Run a plan first, for example:");
        println!("  tidyfs plan --safe");
        return Ok(());
    }

    println!("Would process:");

    for candidate in candidates.iter().take(query.limit) {
        print_candidate(candidate);
    }

    if candidates.len() > query.limit {
        println!(
            "  ... {} more candidates omitted",
            candidates.len() - query.limit
        );
    }

    println!();
    println!("No filesystem changes were made.");
    println!("Real cleanup is intentionally not implemented in Milestone 4.");

    Ok(())
}

fn load_allowed_candidates(database: &Database, scan_id: i64) -> Result<Vec<Candidate>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT
          id,
          path,
          size_bytes,
          rule_id,
          rule_label,
          category,
          risk,
          action_type,
          reversible,
          reason
        FROM cleanup_candidates
        WHERE scan_id = ?1
          AND blocked = 0
        "#,
    )?;

    let rows = stmt
        .query_map(params![scan_id], |row| {
            let risk_text: String = row.get(6)?;
            Ok(Candidate {
                id: row.get(0)?,
                path: PathBuf::from(row.get::<_, String>(1)?),
                size_bytes: row.get::<_, i64>(2)? as u64,
                rule_id: row.get(3)?,
                rule_label: row.get(4)?,
                category: row.get(5)?,
                risk: parse_risk(&risk_text),
                action_type: row.get(7)?,
                reversible: row.get::<_, i64>(8)? != 0,
                reason: row.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}

fn parse_risk(value: &str) -> Risk {
    match value {
        "low" => Risk::Low,
        "medium" => Risk::Medium,
        "high" => Risk::High,
        "forbidden" => Risk::Forbidden,
        _ => Risk::Forbidden,
    }
}

fn print_candidate(candidate: &Candidate) {
    println!(
        "  {:>10}  {}",
        util::format_bytes(candidate.size_bytes),
        candidate.path.display()
    );
    println!("           Candidate: {}", candidate.id);
    println!("           Rule: {}", candidate.rule_id);
    println!("           Label: {}", candidate.rule_label);
    println!("           Category: {}", candidate.category);
    println!("           Risk: {}", candidate.risk);
    println!("           Action: {}", candidate.action_type);
    println!(
        "           Reversible: {}",
        if candidate.reversible { "yes" } else { "no" }
    );
    println!("           Reason: {}", candidate.reason);
}
