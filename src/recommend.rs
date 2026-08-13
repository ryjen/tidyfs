use crate::db::Database;
use crate::rules::{self, Risk};
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tidyfs::ai_contract::AiPathMode;
use tidyfs::ai_gateway::{new_gateway_request_id, LoopbackGatewayConfig, LoopbackGatewayProvider};
use tidyfs::ai_goal::{
    goal_plan_digest, AiGoalCandidate, AiGoalConstraints, AiGoalRecommendation, AiGoalRequest,
    AI_MAX_GOAL_CANDIDATES,
};

#[derive(Debug)]
pub struct RecommendQuery {
    pub endpoint: String,
    pub scan_id: Option<i64>,
    pub target_bytes: u64,
    pub root: Option<PathBuf>,
    pub max_risk: Risk,
    pub path_mode: AiPathMode,
    pub limit: usize,
    pub connect_timeout_ms: u64,
    pub timeout_ms: u64,
    pub max_response_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanCandidate {
    id: i64,
    path: PathBuf,
    size_bytes: u64,
    rule_id: String,
    category: String,
    risk: Risk,
}

pub fn run_recommend(database: &Database, query: RecommendQuery) -> Result<()> {
    validate_query(&query)?;

    let scan = match query.scan_id {
        Some(id) => database.get_scan(id)?,
        None => database.latest_completed_scan()?,
    };
    let root_filter = query
        .root
        .as_ref()
        .map(|path| util::normalize_path_best_effort(path));
    if let Some(root) = &root_filter {
        if !root.starts_with(&scan.root_path) {
            bail!(
                "recommend root {} is outside scan root {}",
                root.display(),
                scan.root_path.display()
            );
        }
    }

    let original = load_eligible_candidates(
        database,
        scan.id,
        root_filter.as_deref(),
        query.max_risk,
        query.limit,
    )?;
    if original.is_empty() {
        bail!("no eligible reversible quarantine candidates found; run `tidyfs plan --safe` first");
    }

    let contract_candidates = build_contract_candidates(&original, query.path_mode);
    let contract_root = root_filter
        .as_deref()
        .map(|root| privacy_path(root, query.path_mode));
    let request = AiGoalRequest::new(
        new_gateway_request_id(),
        scan.id,
        contract_candidates.clone(),
        query.target_bytes,
        query.max_risk.to_string(),
        contract_root,
    );
    request
        .validate()
        .context("validating goal recommendation request")?;

    let mut config = LoopbackGatewayConfig::from_endpoint(&query.endpoint)
        .context("validating goal recommendation gateway endpoint")?;
    config.connect_timeout = Duration::from_millis(query.connect_timeout_ms);
    config.io_timeout = Duration::from_millis(query.timeout_ms);
    config.max_response_bytes = query.max_response_bytes;
    let provider = LoopbackGatewayProvider::new(config);

    let recommendation = provider
        .recommend_goal(&request)
        .context("requesting goal-oriented cleanup recommendation")?;

    let current = load_eligible_candidates(
        database,
        scan.id,
        root_filter.as_deref(),
        query.max_risk,
        query.limit,
    )?;
    let current_contract = build_contract_candidates(&current, query.path_mode);
    verify_plan_is_current(&request, &contract_candidates, &current_contract)
        .context("AI goal recommendation became stale before it could be accepted")?;

    let selected = select_recommended_candidates(&current, &recommendation)?;
    let selected_bytes = selected_reclaim_bytes(&selected)?;
    let target_met = selected_bytes >= query.target_bytes;

    print_recommendation(
        scan.id,
        &scan.root_path,
        root_filter.as_deref(),
        &query,
        &request,
        &recommendation,
        &selected,
        selected_bytes,
        target_met,
        current.len(),
    );

    Ok(())
}

fn validate_query(query: &RecommendQuery) -> Result<()> {
    if query.target_bytes == 0 {
        bail!("recommend --target-bytes must be greater than zero");
    }
    if query.limit == 0 || query.limit > AI_MAX_GOAL_CANDIDATES {
        bail!("recommend --limit must be between 1 and {AI_MAX_GOAL_CANDIDATES}");
    }
    if query.connect_timeout_ms == 0 || query.timeout_ms == 0 {
        bail!("AI gateway timeouts must be greater than zero");
    }
    if query.max_response_bytes == 0 {
        bail!("AI gateway max response bytes must be greater than zero");
    }
    Ok(())
}

fn load_eligible_candidates(
    database: &Database,
    scan_id: i64,
    root: Option<&Path>,
    max_risk: Risk,
    limit: usize,
) -> Result<Vec<PlanCandidate>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT id, path, size_bytes, rule_id, category, risk, action_type, reversible
        FROM cleanup_candidates
        WHERE scan_id = ?1
          AND blocked = 0
        ORDER BY path ASC, risk ASC, rule_id ASC, id ASC
        "#,
    )?;

    let rows = stmt.query_map(params![scan_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            PathBuf::from(row.get::<_, String>(1)?),
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)? != 0,
        ))
    })?;

    // A path can match more than one deterministic rule. Recommendation byte totals
    // describe unique filesystem payloads, so retain one canonical eligible row per path.
    let mut by_path: BTreeMap<PathBuf, PlanCandidate> = BTreeMap::new();
    for row in rows {
        let (id, path, raw_size, rule_id, category, risk_text, action_type, reversible) = row?;
        let risk = parse_risk(&risk_text)
            .with_context(|| format!("invalid persisted cleanup risk for candidate {id}"))?;
        if raw_size < 0
            || !reversible
            || action_type != "quarantine"
            || !rules::risk_allows(risk, max_risk)
            || root.is_some_and(|root| !path.starts_with(root))
        {
            continue;
        }

        by_path.entry(path.clone()).or_insert(PlanCandidate {
            id,
            path,
            size_bytes: raw_size as u64,
            rule_id,
            category,
            risk,
        });
    }

    let mut candidates: Vec<_> = by_path.into_values().collect();
    candidates.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(limit);
    candidates.sort_by_key(|candidate| candidate.id);
    Ok(candidates)
}

fn build_contract_candidates(
    candidates: &[PlanCandidate],
    path_mode: AiPathMode,
) -> Vec<AiGoalCandidate> {
    candidates
        .iter()
        .map(|candidate| AiGoalCandidate {
            candidate_id: candidate.id,
            path: privacy_path(&candidate.path, path_mode),
            path_mode,
            size_bytes: candidate.size_bytes,
            risk: candidate.risk.to_string(),
            rule_id: candidate.rule_id.clone(),
            category: candidate.category.clone(),
        })
        .collect()
}

fn verify_plan_is_current(
    request: &AiGoalRequest,
    original: &[AiGoalCandidate],
    current: &[AiGoalCandidate],
) -> Result<()> {
    if original != current {
        bail!("persisted eligible plan changed during inference");
    }

    let current_digest = goal_plan_digest(request.scan_id, current, &request.constraints);
    if current_digest != request.plan_digest {
        bail!(
            "persisted goal plan digest changed during inference: analyzed={}, current={}",
            request.plan_digest,
            current_digest
        );
    }
    Ok(())
}

fn select_recommended_candidates<'a>(
    current: &'a [PlanCandidate],
    recommendation: &AiGoalRecommendation,
) -> Result<Vec<&'a PlanCandidate>> {
    let by_id: BTreeMap<_, _> = current
        .iter()
        .map(|candidate| (candidate.id, candidate))
        .collect();
    let mut seen = BTreeSet::new();
    let mut selected = Vec::with_capacity(recommendation.selected_candidate_ids.len());
    for id in &recommendation.selected_candidate_ids {
        if !seen.insert(*id) {
            bail!("AI goal recommendation selected duplicate candidate id {id}");
        }
        let candidate = by_id.get(id).with_context(|| {
            format!("AI goal recommendation selected unknown candidate id {id}")
        })?;
        selected.push(*candidate);
    }
    Ok(selected)
}

fn selected_reclaim_bytes(candidates: &[&PlanCandidate]) -> Result<u64> {
    candidates.iter().try_fold(0_u64, |total, candidate| {
        total
            .checked_add(candidate.size_bytes)
            .context("selected reclaim-byte total overflowed u64")
    })
}

#[allow(clippy::too_many_arguments)]
fn print_recommendation(
    scan_id: i64,
    scan_root: &Path,
    root_filter: Option<&Path>,
    query: &RecommendQuery,
    request: &AiGoalRequest,
    recommendation: &AiGoalRecommendation,
    selected: &[&PlanCandidate],
    selected_bytes: u64,
    target_met: bool,
    eligible_count: usize,
) {
    println!("Goal-oriented cleanup recommendation");
    println!();
    println!("scan_id: {scan_id}");
    println!("scan_root: {}", scan_root.display());
    if let Some(root) = root_filter {
        println!("filter_root: {}", root.display());
    }
    println!("risk_threshold: {}", query.max_risk);
    println!("path_mode: {}", path_mode_name(query.path_mode));
    println!(
        "target_reclaimable: {}",
        util::format_bytes(query.target_bytes)
    );
    println!("eligible_candidates: {eligible_count}");
    println!("plan_digest: {}", request.plan_digest);
    println!("mutation_authority: false");
    println!("selected_candidates: {}", selected.len());
    println!(
        "selected_reclaimable: {}",
        util::format_bytes(selected_bytes)
    );
    println!("target_met: {target_met}");
    println!();

    if selected.is_empty() {
        println!("Selected cleanup candidates: none");
    } else {
        println!("Selected cleanup candidates:");
        for candidate in selected {
            println!(
                "  id={}  {:>10}  {}",
                candidate.id,
                util::format_bytes(candidate.size_bytes),
                candidate.path.display()
            );
            println!("           rule: {}", candidate.rule_id);
            println!("           risk: {}", candidate.risk);
        }
    }

    println!();
    println!(
        "AI provenance: {}/{}",
        util::terminal_safe(&recommendation.provenance.provider),
        util::terminal_safe(&recommendation.provenance.model)
    );
    if let Some(request_id) = &recommendation.provenance.request_id {
        println!("AI request_id: {}", util::terminal_safe(request_id));
    }
    println!("AI rationale:");
    for item in &recommendation.rationale {
        println!("  - {}", util::terminal_safe(item));
    }
    if !recommendation.caveats.is_empty() {
        println!("AI caveats:");
        for item in &recommendation.caveats {
            println!("  - {}", util::terminal_safe(item));
        }
    }
    println!();
    println!("Recommendation only. No filesystem changes were made.");
    println!("Run and inspect a deterministic plan/clean flow separately before any cleanup.");
}

fn parse_risk(value: &str) -> Result<Risk> {
    match value {
        "low" => Ok(Risk::Low),
        "medium" => Ok(Risk::Medium),
        "high" => Ok(Risk::High),
        "forbidden" => Ok(Risk::Forbidden),
        other => bail!("unknown risk value {other:?}"),
    }
}

fn privacy_path(path: &Path, mode: AiPathMode) -> String {
    match mode {
        AiPathMode::Full => path.to_string_lossy().into_owned(),
        AiPathMode::Basename => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<root>".to_owned()),
        AiPathMode::Redacted => redact_path(path),
    }
}

fn redact_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    let markers = [
        ".gradle/caches",
        "Library/Developer/Xcode/DerivedData",
        "DerivedData",
        ".cargo/registry",
        ".pnpm-store",
        "node_modules",
        ".cache",
        "/nix/store",
    ];

    for marker in markers {
        if let Some(index) = value.find(marker) {
            if marker == "/nix/store" {
                return "/nix/store/<redacted>".to_owned();
            }
            let suffix = &value[index..];
            if marker == ".cache" {
                let mut parts = suffix.split('/');
                let first = parts.next().unwrap_or(".cache");
                return parts.next().map_or_else(
                    || format!("<redacted>/{first}"),
                    |child| format!("<redacted>/{first}/{child}"),
                );
            }
            return format!("<redacted>/{marker}");
        }
    }

    "<redacted>".to_owned()
}

fn path_mode_name(mode: AiPathMode) -> &'static str {
    match mode {
        AiPathMode::Full => "full",
        AiPathMode::Basename => "basename",
        AiPathMode::Redacted => "redacted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tidyfs::ai_goal::AiGoalRequest;

    fn temp_db_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("tidyfs-{name}-{}-{nonce}.db", std::process::id()))
    }

    fn seeded_database(name: &str) -> (Database, PathBuf) {
        let path = temp_db_path(name);
        let database = Database::open(&path).expect("open test db");
        database.migrate().expect("migrate test db");
        database
            .connection()
            .execute(
                "INSERT INTO scans(id, root_path, started_at, finished_at, status) VALUES (42, '/tmp/tidyfs-root', 1, 2, 'completed')",
                [],
            )
            .expect("insert scan");
        (database, path)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_candidate(
        database: &Database,
        id: i64,
        path: &str,
        size: i64,
        rule_id: &str,
        risk: &str,
        action: &str,
        reversible: bool,
        blocked: bool,
    ) {
        database
            .connection()
            .execute(
                r#"
                INSERT INTO cleanup_candidates(
                  id, scan_id, path, size_bytes, rule_id, rule_label, category, risk,
                  action_type, reversible, reason, blocked, blocked_reason, created_at
                ) VALUES (?1, 42, ?2, ?3, ?4, ?4, 'cache', ?5, ?6, ?7, 'test', ?8, NULL, 3)
                "#,
                params![
                    id,
                    path,
                    size,
                    rule_id,
                    risk,
                    action,
                    i64::from(reversible),
                    i64::from(blocked),
                ],
            )
            .expect("insert cleanup candidate");
    }

    #[test]
    fn eligible_plan_excludes_blocked_non_reversible_tool_and_over_risk_rows() {
        let (database, path) = seeded_database("eligible");
        insert_candidate(
            &database,
            1,
            "/tmp/tidyfs-root/a",
            100,
            "a",
            "low",
            "quarantine",
            true,
            false,
        );
        insert_candidate(
            &database,
            2,
            "/tmp/tidyfs-root/b",
            200,
            "b",
            "low",
            "tool_native",
            true,
            false,
        );
        insert_candidate(
            &database,
            3,
            "/tmp/tidyfs-root/c",
            300,
            "c",
            "low",
            "quarantine",
            false,
            false,
        );
        insert_candidate(
            &database,
            4,
            "/tmp/tidyfs-root/d",
            400,
            "d",
            "low",
            "quarantine",
            true,
            true,
        );
        insert_candidate(
            &database,
            5,
            "/tmp/tidyfs-root/e",
            500,
            "e",
            "medium",
            "quarantine",
            true,
            false,
        );

        let candidates = load_eligible_candidates(&database, 42, None, Risk::Low, 100).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn eligible_plan_counts_each_path_once() {
        let (database, path) = seeded_database("dedup");
        insert_candidate(
            &database,
            1,
            "/tmp/tidyfs-root/cache",
            100,
            "a",
            "low",
            "quarantine",
            true,
            false,
        );
        insert_candidate(
            &database,
            2,
            "/tmp/tidyfs-root/cache",
            100,
            "b",
            "low",
            "quarantine",
            true,
            false,
        );

        let candidates = load_eligible_candidates(&database, 42, None, Risk::Low, 100).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, 1);
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn exact_post_inference_plan_change_is_rejected() {
        let candidates = vec![PlanCandidate {
            id: 1,
            path: PathBuf::from("/tmp/tidyfs-root/cache"),
            size_bytes: 100,
            rule_id: "cache".to_owned(),
            category: "cache".to_owned(),
            risk: Risk::Low,
        }];
        let original = build_contract_candidates(&candidates, AiPathMode::Redacted);
        let request = AiGoalRequest::new(
            "req".to_owned(),
            42,
            original.clone(),
            50,
            "low".to_owned(),
            None,
        );
        let mut changed = original.clone();
        changed[0].size_bytes += 1;
        assert!(verify_plan_is_current(&request, &original, &changed).is_err());
    }

    #[test]
    fn reclaim_total_is_deterministic_and_checked() {
        let left = PlanCandidate {
            id: 1,
            path: PathBuf::from("/a"),
            size_bytes: 100,
            rule_id: "a".to_owned(),
            category: "cache".to_owned(),
            risk: Risk::Low,
        };
        let right = PlanCandidate {
            id: 2,
            path: PathBuf::from("/b"),
            size_bytes: 250,
            rule_id: "b".to_owned(),
            category: "cache".to_owned(),
            risk: Risk::Low,
        };
        assert_eq!(selected_reclaim_bytes(&[&left, &right]).unwrap(), 350);
    }

    #[test]
    fn recommend_round_trip_does_not_mutate_plan_or_actions() {
        let (database, path) = seeded_database("roundtrip");
        insert_candidate(
            &database,
            7,
            "/tmp/tidyfs-root/.cache/pip",
            4096,
            "pip-cache",
            "low",
            "quarantine",
            true,
            false,
        );

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
        let address = listener.local_addr().expect("local address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let body = read_http_request(&mut stream);
            let request: AiGoalRequest = serde_json::from_slice(&body).expect("goal request");
            let response = serde_json::json!({
                "contract_version": 1,
                "request_id": request.request_id,
                "plan_digest": request.plan_digest,
                "recommendation": {
                    "schema_version": 1,
                    "selected_candidate_ids": [7],
                    "rationale": ["bounded low-risk reclaim"],
                    "caveats": ["review before cleanup"],
                    "provenance": {
                        "provider": "fake",
                        "model": "goal-test",
                        "request_id": request.request_id
                    }
                }
            });
            write_json_response(&mut stream, &serde_json::to_vec(&response).unwrap());
        });

        let before_candidates: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM cleanup_candidates", [], |row| {
                row.get(0)
            })
            .unwrap();
        let before_actions: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
            .unwrap();

        run_recommend(
            &database,
            RecommendQuery {
                endpoint: format!("http://{address}"),
                scan_id: Some(42),
                target_bytes: 1024,
                root: None,
                max_risk: Risk::Low,
                path_mode: AiPathMode::Redacted,
                limit: 100,
                connect_timeout_ms: 1000,
                timeout_ms: 1000,
                max_response_bytes: 16 * 1024,
            },
        )
        .expect("read-only recommendation");
        server.join().unwrap();

        let after_candidates: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM cleanup_candidates", [], |row| {
                row.get(0)
            })
            .unwrap();
        let after_actions: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before_candidates, after_candidates);
        assert_eq!(before_actions, after_actions);

        drop(database);
        let _ = std::fs::remove_file(path);
    }

    fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            assert!(read > 0, "request closed before body was complete");
            request.extend_from_slice(&buffer[..read]);
            if let Some(boundary) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&request[..boundary]).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap();
                if request.len() >= boundary + 4 + length {
                    return request[boundary + 4..boundary + 4 + length].to_vec();
                }
            }
        }
    }

    fn write_json_response(stream: &mut TcpStream, body: &[u8]) {
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }
}
