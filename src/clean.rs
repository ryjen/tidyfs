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

#[derive(Debug, Clone)]
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
    blocked: bool,
    blocked_reason: Option<String>,
    scanned_dev: Option<u64>,
    scanned_inode: Option<u64>,
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

    execute_interactive(
        database,
        &scan,
        query.max_risk,
        root_filter.as_ref(),
        &candidates,
    )
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
                println!(
                    "quarantined action_id={action_id}: {}",
                    candidate.path.display()
                );
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
    tidyfs::filesystem_boundary::ensure_same_filesystem(&candidate.path, quarantine_root)?;

    let payload_sha256 = util::fingerprint(&candidate.path)
        .with_context(|| format!("fingerprinting {}", candidate.path.display()))?;

    verify_candidate_at_path(candidate, &candidate.path)
        .context("source changed while fingerprinting; rescan before cleanup")?;

    let action_id = insert_action(database, scan.id, candidate, &payload_sha256)?;
    let action_dir = quarantine_root.join(format!("action-{action_id}"));
    let payload_path = action_dir.join("payload");
    let manifest_path = action_dir.join("manifest.txt");

    set_action_intent(database, action_id, &payload_path)?;

    let prepare_result = (|| -> Result<()> {
        fs::create_dir_all(&action_dir)
            .with_context(|| format!("creating action quarantine dir {}", action_dir.display()))?;

        let manifest = format!(
            "action_id={}\nscan_id={}\ncandidate_id={}\noriginal_path={}\nquarantine_path={}\nrule_id={}\nrisk={}\nsize_bytes={}\nidentity_version=sha256-tree-v1\npayload_sha256={}\n",
            action_id,
            scan.id,
            candidate.id,
            candidate.path.display(),
            payload_path.display(),
            candidate.rule_id,
            candidate.risk,
            candidate.size_bytes,
            payload_sha256,
        );
        fs::write(&manifest_path, manifest)
            .with_context(|| format!("writing manifest {}", manifest_path.display()))?;
        Ok(())
    })();

    if let Err(err) = prepare_result {
        record_action_failure(database, action_id, &err);
        return Err(err);
    }

    set_action_status(database, action_id, "moving", None)?;

    if let Err(err) = verify_candidate_at_path(candidate, &candidate.path)
        .context("source changed immediately before quarantine rename; rescan before cleanup")
    {
        record_action_failure(database, action_id, &err);
        return Err(err);
    }

    if let Err(err) = fs::rename(&candidate.path, &payload_path).with_context(|| {
        format!(
            "moving {} to {}",
            candidate.path.display(),
            payload_path.display()
        )
    }) {
        record_action_failure(database, action_id, &err);
        return Err(err);
    }

    if let Err(err) = verify_candidate_at_path(candidate, &payload_path)
        .context("moved payload is not the filesystem object recorded by the scan")
    {
        record_action_failure(database, action_id, &err);
        return Err(err);
    }

    let quarantined_sha256 = util::fingerprint(&payload_path)
        .with_context(|| format!("verifying quarantined payload {}", payload_path.display()))?;
    if quarantined_sha256 != payload_sha256 {
        let err = anyhow::anyhow!(
            "payload identity changed during quarantine: expected={payload_sha256} actual={quarantined_sha256}"
        );
        record_action_failure(database, action_id, &err);
        return Err(err);
    }

    set_action_status(database, action_id, "quarantined", None)?;
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

    verify_candidate_at_path(candidate, &candidate.path)?;
    Ok(())
}

fn verify_candidate_at_path(candidate: &Candidate, path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;

    if metadata.file_type().is_symlink() {
        bail!("refusing to quarantine symlink path: {}", path.display());
    }

    verify_scanned_identity(candidate, &metadata)
}

#[cfg(unix)]
fn verify_scanned_identity(candidate: &Candidate, metadata: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let scanned_dev = candidate
        .scanned_dev
        .context("scan record has no device identity; rescan before cleanup")?;
    let scanned_inode = candidate
        .scanned_inode
        .context("scan record has no inode identity; rescan before cleanup")?;
    let current_dev = metadata.dev();
    let current_inode = metadata.ino();

    if current_dev != scanned_dev || current_inode != scanned_inode {
        bail!(
            "candidate changed since scan: expected device/inode={scanned_dev}/{scanned_inode}, current={current_dev}/{current_inode}; rescan before cleanup"
        );
    }

    Ok(())
}

#[cfg(not(unix))]
fn verify_scanned_identity(_candidate: &Candidate, _metadata: &fs::Metadata) -> Result<()> {
    bail!("source identity verification is not supported on this platform")
}

fn insert_action(
    database: &Database,
    scan_id: i64,
    candidate: &Candidate,
    payload_sha256: &str,
) -> Result<i64> {
    database.connection().execute(
        r#"
        INSERT INTO actions(
          timestamp, scan_id, candidate_id, original_path, quarantine_path,
          action_type, size_bytes, rule_id, risk, status, error,
          payload_sha256, identity_version
        )
        VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, 'planned', NULL, ?9, 'sha256-tree-v1')
        "#,
        params![
            util::unix_now(),
            scan_id,
            candidate.id,
            candidate.path.to_string_lossy(),
            candidate.action_type,
            candidate.size_bytes as i64,
            candidate.rule_id,
            candidate.risk.to_string(),
            payload_sha256,
        ],
    )?;

    Ok(database.connection().last_insert_rowid())
}

fn set_action_intent(database: &Database, action_id: i64, quarantine_path: &Path) -> Result<()> {
    database.connection().execute(
        r#"
        UPDATE actions
        SET quarantine_path = ?1
        WHERE id = ?2
          AND status = 'planned'
        "#,
        params![quarantine_path.to_string_lossy(), action_id],
    )?;
    Ok(())
}

fn set_action_status(
    database: &Database,
    action_id: i64,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    database.connection().execute(
        r#"
        UPDATE actions
        SET status = ?1,
            error = ?2
        WHERE id = ?3
        "#,
        params![status, error, action_id],
    )?;
    Ok(())
}

fn record_action_failure(database: &Database, action_id: i64, error: &anyhow::Error) {
    let message = format!("{error:#}");
    let _ = set_action_status(database, action_id, "failed", Some(&message));
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
          c.id,
          c.path,
          c.size_bytes,
          c.rule_id,
          c.rule_label,
          c.category,
          c.risk,
          c.action_type,
          c.reversible,
          c.reason,
          c.blocked,
          c.blocked_reason,
          e.dev,
          e.inode
        FROM cleanup_candidates c
        LEFT JOIN entries e
          ON e.scan_id = c.scan_id
         AND e.path = c.path
        WHERE c.scan_id = ?1
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
                blocked: row.get::<_, i64>(10)? != 0,
                blocked_reason: row.get(11)?,
                scanned_dev: row.get::<_, Option<i64>>(12)?.map(|value| value as u64),
                scanned_inode: row.get::<_, Option<i64>>(13)?.map(|value| value as u64),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(canonicalize_clean_candidates(rows)
        .into_iter()
        .filter(|candidate| !candidate.blocked)
        .collect())
}

fn canonicalize_clean_candidates(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let decisions = {
        let inputs: Vec<_> = candidates
            .iter()
            .map(|candidate| rules::HierarchyInput {
                path: &candidate.path,
                risk: candidate.risk,
                blocked: candidate.blocked,
                blocked_reason: candidate.blocked_reason.as_deref(),
                reversible: candidate.reversible,
                action_type: &candidate.action_type,
                rule_id: &candidate.rule_id,
            })
            .collect();
        rules::canonicalize_hierarchy(&inputs)
    };

    decisions
        .into_iter()
        .map(|decision| {
            let mut candidate = candidates[decision.source_index].clone();
            candidate.risk = decision.effective_risk;
            candidate.blocked = decision.blocked;
            let prior_reason = candidate.blocked_reason.take();
            candidate.blocked_reason = if candidate.blocked {
                decision.blocked_reason.or(prior_reason)
            } else {
                None
            };
            candidate
        })
        .collect()
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

#[cfg(all(test, unix))]
mod tests {
    use super::{
        load_allowed_candidates, verify_candidate_at_path, verify_scanned_identity, Candidate,
    };
    use crate::db::Database;
    use crate::rules::Risk;
    use rusqlite::params;
    use std::fs;
    use std::os::unix::fs::{symlink, MetadataExt};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn candidate(path: PathBuf, dev: u64, inode: u64) -> Candidate {
        Candidate {
            id: 1,
            path,
            size_bytes: 0,
            rule_id: "test".into(),
            rule_label: "test".into(),
            category: "test".into(),
            risk: Risk::Low,
            action_type: "quarantine".into(),
            reversible: true,
            reason: "test".into(),
            blocked: false,
            blocked_reason: None,
            scanned_dev: Some(dev),
            scanned_inode: Some(inode),
        }
    }

    fn fixture_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tidyfs-{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn legacy_plan_hierarchy_filters_overlaps_before_cleanup_selection() {
        let db_path = fixture_path("legacy-hierarchy.db");
        let database = Database::open(&db_path).expect("open test database");
        database.migrate().expect("migrate test database");
        database
            .connection()
            .execute(
                "INSERT INTO scans(id, root_path, started_at, finished_at, status) VALUES (42, '/tmp', 1, 2, 'completed')",
                [],
            )
            .expect("insert scan");

        let insert = |id: i64, path: &str, risk: &str, blocked: bool| {
            database
                .connection()
                .execute(
                    r#"
                    INSERT INTO cleanup_candidates(
                      id, scan_id, path, size_bytes, rule_id, rule_label, category, risk,
                      action_type, reversible, reason, blocked, blocked_reason, created_at
                    ) VALUES (?1, 42, ?2, 100, ?3, ?3, 'cache', ?4, 'quarantine', 1, 'test', ?5, ?6, 3)
                    "#,
                    params![
                        id,
                        path,
                        format!("rule-{id}"),
                        risk,
                        if blocked { 1_i64 } else { 0_i64 },
                        blocked.then_some("blocked child"),
                    ],
                )
                .expect("insert cleanup candidate");
        };

        insert(1, "/tmp/hierarchy/a", "low", false);
        insert(2, "/tmp/hierarchy/a/child", "low", false);
        insert(3, "/tmp/hierarchy/b", "low", false);
        insert(4, "/tmp/hierarchy/b/child", "forbidden", true);

        let candidates = load_allowed_candidates(&database, 42).expect("load canonical candidates");
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>(),
            vec![2]
        );

        drop(database);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn accepts_the_same_scanned_inode() {
        let path = fixture_path("source-id");
        fs::write(&path, b"payload").expect("write fixture");
        let metadata = fs::symlink_metadata(&path).expect("fixture metadata");
        let candidate = candidate(path.clone(), metadata.dev(), metadata.ino());

        verify_scanned_identity(&candidate, &metadata).expect("same identity should pass");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_a_substituted_inode() {
        let path = fixture_path("source-id-mismatch");
        fs::write(&path, b"replacement").expect("write fixture");
        let metadata = fs::symlink_metadata(&path).expect("fixture metadata");
        let candidate = candidate(path.clone(), metadata.dev(), metadata.ino().wrapping_add(1));

        let error = verify_scanned_identity(&candidate, &metadata)
            .expect_err("substituted inode must be rejected");
        assert!(error.to_string().contains("candidate changed since scan"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_same_content_path_replacement() {
        let path = fixture_path("source-replacement");
        let replacement = fixture_path("source-replacement-new");
        fs::write(&path, b"same-content").expect("write original fixture");
        fs::write(&replacement, b"same-content").expect("write replacement fixture");

        let original_metadata = fs::symlink_metadata(&path).expect("original metadata");
        let replacement_metadata =
            fs::symlink_metadata(&replacement).expect("replacement metadata");
        assert_ne!(
            (original_metadata.dev(), original_metadata.ino()),
            (replacement_metadata.dev(), replacement_metadata.ino()),
            "fixtures must have distinct filesystem identities"
        );
        let candidate = candidate(
            path.clone(),
            original_metadata.dev(),
            original_metadata.ino(),
        );

        fs::rename(&replacement, &path).expect("replace original path");

        let error = verify_candidate_at_path(&candidate, &path)
            .expect_err("same-content replacement must be rejected by inode identity");
        assert!(error.to_string().contains("candidate changed since scan"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_symlink_replacement() {
        let path = fixture_path("source-symlink");
        let target = fixture_path("source-symlink-target");
        fs::write(&path, b"payload").expect("write original fixture");
        fs::write(&target, b"target").expect("write symlink target");
        let original_metadata = fs::symlink_metadata(&path).expect("original metadata");
        let candidate = candidate(
            path.clone(),
            original_metadata.dev(),
            original_metadata.ino(),
        );

        fs::remove_file(&path).expect("remove original fixture");
        symlink(&target, &path).expect("replace candidate with symlink");

        let error = verify_candidate_at_path(&candidate, &path)
            .expect_err("symlink replacement must be rejected");
        assert!(error
            .to_string()
            .contains("refusing to quarantine symlink path"));
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(target);
    }
}
