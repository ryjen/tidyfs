use crate::adapters;
use crate::db::Database;
use crate::rules::{self, Rule, Risk};
use crate::util;
use anyhow::Result;
use rusqlite::params;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Debug)]
pub struct PlanQuery {
    pub scan_id: Option<i64>,
    pub max_risk: Risk,
    pub root: Option<PathBuf>,
    pub include_blocked: bool,
    pub include_adapters: bool,
    pub limit: usize,
}

#[derive(Debug, Clone)]
struct ClassifiedPath {
    path: PathBuf,
    labels: Vec<String>,
    size_bytes: u64,
    max_mtime: Option<i64>,
}

#[derive(Debug, Clone)]
struct PlannedCandidate {
    path: PathBuf,
    size_bytes: u64,
    rule_id: String,
    rule_label: String,
    category: String,
    risk: Risk,
    action_type: String,
    reversible: bool,
    reason: String,
    blocked: bool,
    blocked_reason: Option<String>,
}

pub fn run_plan(database: &mut Database, query: PlanQuery) -> Result<()> {
    let scan = match query.scan_id {
        Some(id) => database.get_scan(id)?,
        None => database.latest_completed_scan()?,
    };

    let root_filter = query
        .root
        .as_ref()
        .map(|p| util::normalize_path_best_effort(p));

    let rules = rules::load_builtin_rules()?;
    let paths = load_classified_paths(database, scan.id)?;

    let mut candidates = Vec::new();

    for path in paths {
        if let Some(root) = &root_filter {
            if !path.path.starts_with(root) {
                continue;
            }
        }

        for rule in &rules {
            if rule_matches(rule, &path) {
                let blocked_reason = validate_policy(rule, &path, query.max_risk);
                let blocked = blocked_reason.is_some();

                candidates.push(PlannedCandidate {
                    path: path.path.clone(),
                    size_bytes: path.size_bytes,
                    rule_id: rule.id.clone(),
                    rule_label: rule.label.clone(),
                    category: rule.category.clone(),
                    risk: rule.risk,
                    action_type: rule.action_type.to_string(),
                    reversible: rule.reversible,
                    reason: rule.reason.clone(),
                    blocked,
                    blocked_reason,
                });
            }
        }
    }

    if query.include_adapters {
        for adapter_candidate in adapters::build_adapter_candidates(query.max_risk) {
            let blocked = adapter_candidate.blocked_reason.is_some();
            candidates.push(PlannedCandidate {
                path: adapter_candidate.path,
                size_bytes: adapter_candidate.size_bytes,
                rule_id: adapter_candidate.rule_id,
                rule_label: adapter_candidate.rule_label,
                category: adapter_candidate.category,
                risk: adapter_candidate.risk,
                action_type: adapter_candidate.action_type.to_string(),
                reversible: adapter_candidate.reversible,
                reason: adapter_candidate.reason,
                blocked,
                blocked_reason: adapter_candidate.blocked_reason,
            });
        }
    }

    candidates.sort_by(|a, b| {
        a.blocked
            .cmp(&b.blocked)
            .then_with(|| b.size_bytes.cmp(&a.size_bytes))
            .then_with(|| a.path.cmp(&b.path))
    });

    persist_candidates(database, scan.id, &candidates)?;

    print_plan(scan.id, &scan.root_path, query.max_risk, &candidates, query.include_blocked, query.limit);

    Ok(())
}

fn load_classified_paths(database: &Database, scan_id: i64) -> Result<Vec<ClassifiedPath>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT
          c.path,
          c.label,
          COALESCE(dt.allocated_size_bytes, e.allocated_size_bytes, 0) AS size_bytes,
          dt.max_mtime
        FROM classifications c
        LEFT JOIN directory_totals dt
          ON dt.scan_id = c.scan_id
         AND dt.path = c.path
        LEFT JOIN entries e
          ON e.scan_id = c.scan_id
         AND e.path = c.path
        WHERE c.scan_id = ?1
        ORDER BY c.path ASC
        "#,
    )?;

    let mut grouped: BTreeMap<PathBuf, ClassifiedPath> = BTreeMap::new();

    let rows = stmt.query_map(params![scan_id], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? as u64,
            row.get::<_, Option<i64>>(3)?,
        ))
    })?;

    for row in rows {
        let (path, label, size_bytes, max_mtime) = row?;
        grouped
            .entry(path.clone())
            .and_modify(|existing| {
                existing.labels.push(label.clone());
                existing.size_bytes = existing.size_bytes.max(size_bytes);
                existing.max_mtime = match (existing.max_mtime, max_mtime) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (None, Some(b)) => Some(b),
                    (a, None) => a,
                };
            })
            .or_insert_with(|| ClassifiedPath {
                path,
                labels: vec![label],
                size_bytes,
                max_mtime,
            });
    }

    Ok(grouped.into_values().collect())
}

fn rule_matches(rule: &Rule, item: &ClassifiedPath) -> bool {
    let m = &rule.r#match;

    if !m.labels_any.is_empty()
        && !m
            .labels_any
            .iter()
            .any(|wanted| item.labels.iter().any(|label| label == wanted))
    {
        return false;
    }

    if !m.path_contains_any.is_empty() {
        let path = item.path.to_string_lossy();
        if !m.path_contains_any.iter().any(|needle| path.contains(needle)) {
            return false;
        }
    }

    if let Some(expected) = &m.path_basename {
        if rules::basename(&item.path).as_deref() != Some(expected.as_str()) {
            return false;
        }
    }

    if let Some(min) = m.min_size_bytes {
        if item.size_bytes < min {
            return false;
        }
    }

    if let Some(days) = m.older_than_days {
        let Some(max_mtime) = item.max_mtime else {
            return false;
        };
        let age_seconds = util::unix_now().saturating_sub(max_mtime);
        if age_seconds < (days as i64).saturating_mul(24 * 60 * 60) {
            return false;
        }
    }

    true
}

fn validate_policy(rule: &Rule, item: &ClassifiedPath, max_risk: Risk) -> Option<String> {
    if item.labels.iter().any(|label| {
        matches!(
            label.as_str(),
            "secret_material"
                | "git_repo"
                | "database"
                | "vm_image"
                | "browser_profile"
        )
    }) {
        return Some("policy forbids cleanup of protected/sensitive path category".to_string());
    }

    if item.labels.iter().any(|label| {
        matches!(
            label.as_str(),
            "docker_data" | "podman_data" | "nix_store" | "systemd_journal"
        )
    }) {
        return Some("policy requires a future tool-native adapter; raw file cleanup is blocked".to_string());
    }

    if !rules::risk_allows(rule.risk, max_risk) {
        return Some(format!(
            "risk {} exceeds selected threshold {}",
            rule.risk, max_risk
        ));
    }

    if rule.risk == Risk::Forbidden {
        return Some("rule is forbidden by design".to_string());
    }

    None
}

fn persist_candidates(database: &mut Database, scan_id: i64, candidates: &[PlannedCandidate]) -> Result<()> {
    let tx = database.transaction()?;
    tx.execute(
        "DELETE FROM cleanup_candidates WHERE scan_id = ?1",
        params![scan_id],
    )?;

    let now = util::unix_now();
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO cleanup_candidates(
          scan_id, path, size_bytes, rule_id, rule_label, category, risk,
          action_type, reversible, reason, blocked, blocked_reason, created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
    )?;

    for c in candidates {
        stmt.execute(params![
            scan_id,
            c.path.to_string_lossy(),
            c.size_bytes as i64,
            c.rule_id,
            c.rule_label,
            c.category,
            c.risk.to_string(),
            c.action_type,
            c.reversible as i64,
            c.reason,
            c.blocked as i64,
            c.blocked_reason,
            now,
        ])?;
    }

    drop(stmt);
    tx.commit()?;
    Ok(())
}

fn print_plan(
    scan_id: i64,
    scan_root: &PathBuf,
    max_risk: Risk,
    candidates: &[PlannedCandidate],
    include_blocked: bool,
    limit: usize,
) {
    let allowed: Vec<_> = candidates.iter().filter(|c| !c.blocked).collect();
    let blocked: Vec<_> = candidates.iter().filter(|c| c.blocked).collect();

    let allowed_bytes: u64 = allowed.iter().map(|c| c.size_bytes).sum();

    println!("scan_id: {scan_id}");
    println!("scan_root: {}", scan_root.display());
    println!("risk_threshold: {max_risk}");
    println!("include_adapters: {}", candidates.iter().any(|c| c.path.to_string_lossy().starts_with("adapter://")));
    println!("allowed_candidates: {}", allowed.len());
    println!("allowed_reclaimable: {}", util::format_bytes(allowed_bytes));
    println!("blocked_or_report_only: {}", blocked.len());
    println!();

    println!("Allowed cleanup candidates:");
    if allowed.is_empty() {
        println!("  none");
    } else {
        for c in allowed.iter().take(limit) {
            print_candidate(c);
        }
        if allowed.len() > limit {
            println!("  ... {} more allowed candidates omitted", allowed.len() - limit);
        }
    }

    if include_blocked {
        println!();
        println!("Blocked / report-only:");
        if blocked.is_empty() {
            println!("  none");
        } else {
            for c in blocked.iter().take(limit) {
                print_candidate(c);
                if let Some(reason) = &c.blocked_reason {
                    println!("           Blocked: {reason}");
                }
            }
            if blocked.len() > limit {
                println!("  ... {} more blocked candidates omitted", blocked.len() - limit);
            }
        }
    }
}

fn print_candidate(c: &PlannedCandidate) {
    println!(
        "  {:>10}  {}",
        util::format_bytes(c.size_bytes),
        c.path.display()
    );
    println!("           Rule: {}", c.rule_id);
    println!("           Label: {}", c.rule_label);
    println!("           Risk: {}", c.risk);
    println!("           Action: {}", c.action_type);
    println!("           Reason: {}", c.reason);
}
