use crate::adapters;
use crate::ai_facts::{self, IndexedCandidate};
use crate::ai_planning::{self, AiEvidence};
use crate::db::Database;
use crate::rules::{self, Risk, Rule};
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tidyfs::ai_contract::AiPathMode;
use tidyfs::ai_gateway::{LoopbackGatewayConfig, LoopbackGatewayProvider};

const MAX_AI_PLAN_CANDIDATES: usize = 100;

#[derive(Debug, Clone, Copy)]
struct AiPlanOptions<'a> {
    endpoint: &'a str,
    path_mode: AiPathMode,
    max_risk: Risk,
    limit: usize,
}

#[derive(Debug)]
pub struct PlanQuery {
    pub scan_id: Option<i64>,
    pub max_risk: Risk,
    pub root: Option<PathBuf>,
    pub include_blocked: bool,
    pub include_adapters: bool,
    pub limit: usize,
    pub ai_endpoint: Option<String>,
    pub ai_path_mode: AiPathMode,
    pub ai_limit: usize,
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
    ai: Option<AiEvidence>,
}

pub fn run_plan(database: &mut Database, query: PlanQuery) -> Result<()> {
    if query.ai_endpoint.is_some()
        && (query.ai_limit == 0 || query.ai_limit > MAX_AI_PLAN_CANDIDATES)
    {
        bail!("plan --ai-limit must be between 1 and {MAX_AI_PLAN_CANDIDATES}");
    }

    let scan = match query.scan_id {
        Some(id) => database.get_scan(id)?,
        None => database.latest_completed_scan()?,
    };

    let root_filter = query
        .root
        .as_ref()
        .map(|p| util::normalize_path_best_effort(p));
    if let Some(root) = &root_filter {
        if !root.starts_with(&scan.root_path) {
            bail!(
                "plan root {} is outside scan root {}",
                root.display(),
                scan.root_path.display()
            );
        }
    }

    let ai_enabled = query.ai_endpoint.is_some();
    if ai_enabled {
        invalidate_prior_ai_plan(database, scan.id)?;
    }

    let rules = rules::load_builtin_rules()?;
    let paths = ai_facts::load_candidates(database, scan.id)?;

    let mut candidates = Vec::new();

    for path in &paths {
        if let Some(root) = &root_filter {
            if !path.path.starts_with(root) {
                continue;
            }
        }

        for rule in &rules {
            if rule_matches(rule, path) {
                let blocked_reason = validate_policy(rule, path, query.max_risk);
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
                    ai: None,
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
                ai: None,
            });
        }
    }

    let evidence = if let Some(endpoint) = &query.ai_endpoint {
        analyze_plan_candidates(
            database,
            scan.id,
            &paths,
            &mut candidates,
            AiPlanOptions {
                endpoint,
                path_mode: query.ai_path_mode,
                max_risk: query.max_risk,
                limit: query.ai_limit,
            },
        )?
    } else {
        Vec::new()
    };

    candidates.sort_by(|a, b| {
        a.blocked
            .cmp(&b.blocked)
            .then_with(|| b.size_bytes.cmp(&a.size_bytes))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.rule_id.cmp(&b.rule_id))
    });

    persist_plan(database, scan.id, &candidates, ai_enabled, &evidence)?;

    print_plan(
        scan.id,
        &scan.root_path,
        query.max_risk,
        &candidates,
        query.include_blocked,
        query.limit,
        ai_enabled,
        evidence.len(),
    );

    Ok(())
}

fn invalidate_prior_ai_plan(database: &mut Database, scan_id: i64) -> Result<()> {
    let tx = database.transaction()?;
    tx.execute(
        "DELETE FROM cleanup_candidates WHERE scan_id = ?1",
        params![scan_id],
    )?;
    tx.execute(
        "DELETE FROM ai_recommendations WHERE scan_id = ?1",
        params![scan_id],
    )?;
    tx.commit()?;
    Ok(())
}

fn analyze_plan_candidates(
    database: &Database,
    scan_id: i64,
    paths: &[IndexedCandidate],
    candidates: &mut [PlannedCandidate],
    options: AiPlanOptions<'_>,
) -> Result<Vec<AiEvidence>> {
    let config = LoopbackGatewayConfig::from_endpoint(options.endpoint)
        .context("validating plan AI gateway endpoint")?;
    let provider = LoopbackGatewayProvider::new(config);

    let mut eligible: BTreeMap<PathBuf, u64> = BTreeMap::new();
    for candidate in candidates.iter() {
        if candidate.blocked
            || !candidate.reversible
            || candidate.action_type != "quarantine"
            || candidate.path.to_string_lossy().starts_with("adapter://")
        {
            continue;
        }
        eligible
            .entry(candidate.path.clone())
            .and_modify(|size| *size = (*size).max(candidate.size_bytes))
            .or_insert(candidate.size_bytes);
    }

    let mut selected: Vec<_> = eligible.into_iter().collect();
    selected.sort_by(|(left_path, left_size), (right_path, right_size)| {
        right_size
            .cmp(left_size)
            .then_with(|| left_path.cmp(right_path))
    });
    selected.truncate(options.limit);

    let mut evidence = Vec::with_capacity(selected.len());
    for (path, _) in selected {
        let facts = paths
            .iter()
            .find(|item| item.path == path)
            .with_context(|| {
                format!(
                    "classified facts missing for AI plan path {}",
                    path.display()
                )
            })?;
        evidence.push(ai_planning::analyze_candidate(
            &provider,
            database,
            scan_id,
            facts,
            options.path_mode,
            options.max_risk,
        )?);
    }

    let evidence_by_path: BTreeMap<_, _> = evidence
        .iter()
        .map(|item| (item.path.clone(), item.clone()))
        .collect();

    for candidate in candidates.iter_mut() {
        let Some(item) = evidence_by_path.get(&candidate.path) else {
            continue;
        };
        let decision = ai_planning::conservative_policy(
            candidate.risk,
            &candidate.action_type,
            candidate.reversible,
            candidate.blocked,
            candidate.blocked_reason.as_deref(),
            &item.proposal,
            options.max_risk,
        );
        candidate.risk = decision.risk;
        candidate.blocked = decision.blocked;
        candidate.blocked_reason = decision.blocked_reason;
        candidate.ai = Some(item.clone());
    }

    Ok(evidence)
}

fn rule_matches(rule: &Rule, item: &IndexedCandidate) -> bool {
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
        if !m
            .path_contains_any
            .iter()
            .any(|needle| path.contains(needle))
        {
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

fn validate_policy(rule: &Rule, item: &IndexedCandidate, max_risk: Risk) -> Option<String> {
    if item.labels.iter().any(|label| {
        matches!(
            label.as_str(),
            "secret_material" | "git_repo" | "database" | "vm_image" | "browser_profile"
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
        return Some(
            "policy requires a future tool-native adapter; raw file cleanup is blocked".to_string(),
        );
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

fn persist_plan(
    database: &mut Database,
    scan_id: i64,
    candidates: &[PlannedCandidate],
    replace_ai_evidence: bool,
    evidence: &[AiEvidence],
) -> Result<()> {
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
    if replace_ai_evidence {
        ai_planning::replace_evidence(&tx, scan_id, evidence, now)?;
    }
    tx.commit()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn print_plan(
    scan_id: i64,
    scan_root: &Path,
    max_risk: Risk,
    candidates: &[PlannedCandidate],
    include_blocked: bool,
    limit: usize,
    ai_enabled: bool,
    ai_analyzed_paths: usize,
) {
    let allowed: Vec<_> = candidates.iter().filter(|c| !c.blocked).collect();
    let blocked: Vec<_> = candidates.iter().filter(|c| c.blocked).collect();

    let allowed_bytes: u64 = allowed.iter().map(|c| c.size_bytes).sum();

    println!("scan_id: {scan_id}");
    println!("scan_root: {}", scan_root.display());
    println!("risk_threshold: {max_risk}");
    println!(
        "include_adapters: {}",
        candidates
            .iter()
            .any(|c| c.path.to_string_lossy().starts_with("adapter://"))
    );
    println!("ai_enriched: {ai_enabled}");
    println!("ai_analyzed_paths: {ai_analyzed_paths}");
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
            println!(
                "  ... {} more allowed candidates omitted",
                allowed.len() - limit
            );
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
                println!(
                    "  ... {} more blocked candidates omitted",
                    blocked.len() - limit
                );
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

    if let Some(ai) = &c.ai {
        println!(
            "           AI: {} ({:.0}%) risk={} recommendation={}",
            util::terminal_safe(&ai.proposal.classification),
            ai.proposal.confidence * 100.0,
            ai_planning::ai_risk_name(ai.proposal.risk),
            ai_planning::action_name(ai.proposal.recommended_action)
        );
        println!("           AI observation: {}", ai.observation_digest);
        println!(
            "           AI provenance: {}/{}",
            util::terminal_safe(&ai.proposal.provenance.provider),
            util::terminal_safe(&ai.proposal.provenance.model)
        );
        for rationale in &ai.proposal.rationale {
            println!(
                "           AI rationale: {}",
                util::terminal_safe(rationale)
            );
        }
        for caveat in &ai.proposal.caveats {
            println!("           AI caveat: {}", util::terminal_safe(caveat));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clean::{run_clean, CleanQuery};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tidyfs::ai_contract::AiTransportRequest;

    fn temporary_base() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tidyfs-ai-plan-dry-run-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn ai_review_blocks_plan_and_dry_run_does_not_mutate() {
        let base = temporary_base();
        let root = base.join("workspace");
        let candidate_path = root.join("__pycache__");
        fs::create_dir_all(&candidate_path).expect("create candidate directory");

        let db_path = base.join("tidyfs.db");
        let mut database = Database::open(&db_path).expect("open test database");
        database.migrate().expect("migrate test database");
        database
            .connection()
            .execute(
                r#"
                INSERT INTO scans(
                  id, root_path, started_at, finished_at, status,
                  one_file_system, include_pseudo
                ) VALUES (?1, ?2, 1000, 2000, 'completed', 0, 0)
                "#,
                params![42_i64, root.to_string_lossy().to_string()],
            )
            .expect("insert scan");
        database
            .connection()
            .execute(
                r#"
                INSERT INTO entries(
                  id, scan_id, path, parent_path, name, entry_type,
                  size_bytes, allocated_size_bytes, mtime
                ) VALUES (?1, 42, ?2, ?3, '__pycache__', 'dir', 1024, 1024, 1000)
                "#,
                params![
                    7_i64,
                    candidate_path.to_string_lossy().to_string(),
                    root.to_string_lossy().to_string()
                ],
            )
            .expect("insert entry");
        database
            .connection()
            .execute(
                r#"
                INSERT INTO classifications(
                  id, scan_id, path, label, confidence, source, reason
                ) VALUES (1, 42, ?1, 'python_bytecode_cache', 1.0, 'test', 'test')
                "#,
                params![candidate_path.to_string_lossy().to_string()],
            )
            .expect("insert classification");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
        let address = listener.local_addr().expect("gateway address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let body = read_http_body(&mut stream);
            let request: AiTransportRequest =
                serde_json::from_slice(&body).expect("parse transport request");
            let request_id = request.request_id.clone();
            let digest = request.candidate.observation.digest.clone();
            let response = serde_json::json!({
                "contract_version": 1,
                "request_id": request_id,
                "proposal": {
                    "schema_version": 1,
                    "classification": "generated_python_cache",
                    "confidence": 0.99,
                    "rationale": ["review requested by specialist"],
                    "caveats": [],
                    "risk": "low",
                    "recommended_action": "review",
                    "provenance": {
                        "provider": "fake",
                        "model": "test",
                        "request_id": request_id
                    }
                },
                "observation": { "digest": digest }
            });
            let response = serde_json::to_vec(&response).expect("serialize response");
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            );
            stream.write_all(headers.as_bytes()).expect("write headers");
            stream.write_all(&response).expect("write response");
        });

        run_plan(
            &mut database,
            PlanQuery {
                scan_id: Some(42),
                max_risk: Risk::Low,
                root: None,
                include_blocked: true,
                include_adapters: false,
                limit: 10,
                ai_endpoint: Some(format!("http://{address}")),
                ai_path_mode: AiPathMode::Redacted,
                ai_limit: 1,
            },
        )
        .expect("AI-enriched plan");
        server.join().expect("fake gateway thread");

        let (blocked, reason): (i64, Option<String>) = database
            .connection()
            .query_row(
                "SELECT blocked, blocked_reason FROM cleanup_candidates WHERE scan_id = 42 AND path = ?1",
                params![candidate_path.to_string_lossy().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("planned candidate");
        assert_eq!(blocked, 1);
        assert_eq!(reason.as_deref(), Some("AI advisory requires review"));

        run_clean(
            &database,
            CleanQuery {
                scan_id: Some(42),
                dry_run: true,
                safe: true,
                interactive: false,
                max_risk: Risk::Low,
                root: None,
                limit: 10,
            },
        )
        .expect("dry-run clean");

        assert!(candidate_path.is_dir());
        let action_count: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
            .expect("action count");
        assert_eq!(action_count, 0);

        drop(database);
        fs::remove_dir_all(base).expect("remove test data");
    }

    fn read_http_body(stream: &mut TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            assert!(read > 0, "request closed before body was complete");
            request.extend_from_slice(&buffer[..read]);
            if let Some(boundary) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&request[..boundary]).expect("request headers");
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .expect("content length");
                let start = boundary + 4;
                if request.len() >= start + length {
                    return request[start..start + length].to_vec();
                }
            }
        }
    }
}
