use crate::db::{Database, ScanInfo};
use crate::rules::{self, Risk};
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CleanQuery {
    pub scan_id: Option<i64>,
    pub dry_run: bool,
    pub safe: bool,
    pub interactive: bool,
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

    if query.limit > 0 {
        candidates.truncate(query.limit);
    }

    if query.dry_run {
        print_dry_run(&scan, query.max_risk, root_filter.as_ref(), &candidates);
        return Ok(());
    }

    if !query.safe {
        bail!("real cleanup requires --safe");
    }

    if !query.interactive {
        bail!("real cleanup requires --interactive");
    }

    execute_interactive(database, &scan, query.max_risk, root_filter.as_ref(), &candidates)
}

fn print_dry_run(
    scan: &ScanInfo,
    max_risk: Risk,
    root_filter: Option<&PathBuf>,
    candidates: &[Candidate],
) {
    let total_bytes: u64 = candidates.iter().map(|c| c.size_bytes).sum();

    println!("Dry-run cleanup preview");
    println!();
    println!("scan_id: {}", scan.id);
    println!("scan_root: {}", scan.root_path.display());
    println!("risk_threshold: {}", max_risk);
    if let Some(root) = root_filter {
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
        return;
    }

    println!("Would process:");

    for candidate in candidates {
        print_candidate(candidate);
    }

    println!();
    println!("No filesystem changes were made.");
}

fn execute_interactive(
    database: &Database,
    scan: &ScanInfo,
    max_risk: Risk,
    root_filter: Option<&PathBuf>,
    candidates: &[Candidate],
) -> Result<()> {
    let executable: Vec<_> = candidates
        .iter()
        .filter(|c| c.reversible)
        .filter(|c| matches!(c.action_type.as_str(), "quarantine" | "trash"))
        .collect();

    let skipped = candidates.len().saturating_sub(executable.len());
    let total_bytes: u64 = executable.iter().map(|c| c.size_bytes).sum();

    println!("Interactive reversible cleanup");
    println!();
    println!("scan_id: {}", scan.id);
    println!("scan_root: {}", scan.root_path.display());
    println!("risk_threshold: {}", max_risk);
    if let Some(root) = root_filter {
        println!("filter_root: {}", root.display());
    }
    println!("candidate_count: {}", executable.len());
    println!("skipped_non_executable: {}", skipped);
    println!("potential_reclaimable: {}", util::format_bytes(total_bytes));
    println!();

    if executable.is_empty() {
        println!("No reversible executable candidates found.");
        println!("Run `tidyfs plan --safe` and inspect whether allowed rules use quarantine/trash actions.");
        return Ok(());
    }

    println!("Candidates to quarantine:");
    for candidate in &executable {
        print_candidate(candidate);
    }

    println!();
    println!("This will move each selected path into tidyfs quarantine.");
    println!("No permanent deletion will be performed.");
    println!("Restore with:");
    println!("  tidyfs restore --action <id>");
    println!();

    if !confirm("Proceed with quarantine? Type 'yes' to continue: ")? {
        println!("Aborted. No filesystem changes were made.");
        return Ok(());
    }

    let quarantine_root = util::quarantine_root()?;
    fs::create_dir_all(&quarantine_root)
        .with_context(|| format!("creating quarantine root {}", quarantine_root.display()))?;

    let mut success = 0_u64;
    let mut failed = 0_u64;

    for candidate in executable {
        match quarantine_candidate(database, scan, candidate, &quarantine_root) {
            Ok(action_id) => {
                success += 1;
                println!("quarantined action_id={action_id}: {}", candidate.path.display());
            }
            Err(err) => {
                failed += 1;
                eprintln!("failed: {}: {err:#}", candidate.path.display());
            }
        }
    }

    println!();
    println!("completed:");
    println!("  successful: {success}");
    println!("  failed: {failed}");
    println!("  permanent_deletes: 0");

    Ok(())
}

fn quarantine_candidate(
    database: &Database,
    scan: &ScanInfo,
    candidate: &Candidate,
    quarantine_root: &Path,
) -> Result<i64> {
    preflight_candidate(scan, candidate)?;

    let action_id = insert_action(database, scan.id, candidate, "running", None, None)?;
    let action_dir = quarantine_root.join(format!("action-{action_id}"));
    let payload_path = action_dir.join("payload");
    let manifest_path = action_dir.join("manifest.txt");

    fs::create_dir_all(&action_dir)
        .with_context(|| format!("creating action quarantine dir {}", action_dir.display()))?;

    let manifest = format!(
        "action_id={}\nscan_id={}\ncandidate_id={}\noriginal_path={}\nquarantine_path={}\nrule_id={}\nrisk={}\nsize_bytes={}\n",
        action_id,
        scan.id,
        candidate.id,
        candidate.path.display(),
        payload_path.display(),
        candidate.rule_id,
        candidate.risk,
        candidate.size_bytes,
    );
    fs::write(&manifest_path, manifest)
        .with_context(|| format!("writing manifest {}", manifest_path.display()))?;

    fs::rename(&candidate.path, &payload_path).with_context(|| {
        format!(
            "moving {} to {}",
            candidate.path.display(),
            payload_path.display()
        )
    })?;

    update_action_success(database, action_id, &payload_path)?;
    Ok(action_id)
}

fn preflight_candidate(scan: &ScanInfo, candidate: &Candidate) -> Result<()> {
    if candidate.risk != Risk::Low {
        bail!("only low-risk execution is supported in milestone 5");
    }

    if !candidate.reversible {
        bail!("candidate is not reversible");
    }

    if !matches!(candidate.action_type.as_str(), "quarantine" | "trash") {
        bail!("candidate action is not executable by quarantine");
    }

    if !candidate.path.starts_with(&scan.root_path) {
        bail!("candidate path is outside scanned root");
    }

    let meta = fs::symlink_metadata(&candidate.path)
        .with_context(|| format!("reading metadata for {}", candidate.path.display()))?;

    if meta.file_type().is_symlink() {
        bail!("refusing to quarantine symlink path");
    }

    Ok(())
}

fn insert_action(
    database: &Database,
    scan_id: i64,
    candidate: &Candidate,
    status: &str,
    quarantine_path: Option<&Path>,
    error: Option<&str>,
) -> Result<i64> {
    database.connection().execute(
        r#"
        INSERT INTO actions(
          timestamp, scan_id, candidate_id, original_path, quarantine_path,
          action_type, size_bytes, rule_id, risk, status, error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
        params![
            util::unix_now(),
            scan_id,
            candidate.id,
            candidate.path.to_string_lossy(),
            quarantine_path.map(|p| p.to_string_lossy().to_string()),
            candidate.action_type,
            candidate.size_bytes as i64,
            candidate.rule_id,
            candidate.risk.to_string(),
            status,
            error,
        ],
    )?;

    Ok(database.connection().last_insert_rowid())
}

fn update_action_success(database: &Database, action_id: i64, quarantine_path: &Path) -> Result<()> {
    database.connection().execute(
        r#"
        UPDATE actions
        SET status = 'quarantined',
            quarantine_path = ?1
        WHERE id = ?2
        "#,
        params![quarantine_path.to_string_lossy(), action_id],
    )?;
    Ok(())
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;

    Ok(line.trim() == "yes")
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
