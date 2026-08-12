use crate::db::Database;
use crate::rules::Risk;
use crate::util;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tidyfs::ai::{AiCleanupProposal, AiRecommendedAction, AiRisk};
use tidyfs::ai_contract::{AiDeterministicFacts, AiObservation, AiPathMode};
use tidyfs::ai_gateway::{LoopbackGatewayConfig, LoopbackGatewayProvider};
use tidyfs::ai_provider::{analyze_validated, AiAnalysisRequest};

const MAX_ANALYZE_CANDIDATES: usize = 100;
const MAX_LABELS_PER_CANDIDATE: usize = 32;

#[derive(Debug)]
pub struct AnalyzeQuery {
    pub endpoint: String,
    pub scan_id: Option<i64>,
    pub root: Option<PathBuf>,
    pub limit: usize,
    pub path_mode: AiPathMode,
    pub max_risk: Risk,
    pub connect_timeout_ms: u64,
    pub timeout_ms: u64,
    pub max_response_bytes: usize,
}

#[derive(Debug, Clone)]
struct IndexedCandidate {
    identity_id: i64,
    path: PathBuf,
    labels: Vec<String>,
    size_bytes: u64,
    max_mtime: Option<i64>,
}

pub fn run_analyze(database: &Database, query: AnalyzeQuery) -> Result<()> {
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
                "analyze root {} is outside scan root {}",
                root.display(),
                scan.root_path.display()
            );
        }
    }

    let mut candidates = load_candidates(database, scan.id)?;
    candidates.retain(|candidate| {
        root_filter
            .as_ref()
            .is_none_or(|root| candidate.path.starts_with(root))
    });
    candidates.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    candidates.truncate(query.limit);

    let mut config = LoopbackGatewayConfig::from_endpoint(&query.endpoint)
        .context("validating AI gateway endpoint")?;
    config.connect_timeout = Duration::from_millis(query.connect_timeout_ms);
    config.io_timeout = Duration::from_millis(query.timeout_ms);
    config.max_response_bytes = query.max_response_bytes;
    let provider = LoopbackGatewayProvider::new(config);

    println!("scan_id: {}", scan.id);
    println!("scan_root: {}", scan.root_path.display());
    println!("path_mode: {}", path_mode_name(query.path_mode));
    println!("candidates: {}", candidates.len());
    println!("mutation_authority: false");

    if candidates.is_empty() {
        println!("no classified candidates matched the selected scan/root");
        return Ok(());
    }

    for (index, candidate) in candidates.iter().enumerate() {
        let observation = observation_for(scan.id, candidate, query.path_mode, query.max_risk);
        let request = AiAnalysisRequest::new(observation);
        let proposal = analyze_validated(&provider, &request).with_context(|| {
            format!(
                "AI analysis failed for candidate {}",
                request.observation.candidate_key
            )
        })?;

        println!();
        print_proposal(index + 1, &request, &proposal);
    }

    Ok(())
}

fn validate_query(query: &AnalyzeQuery) -> Result<()> {
    if query.limit == 0 || query.limit > MAX_ANALYZE_CANDIDATES {
        bail!(
            "analyze --limit must be between 1 and {MAX_ANALYZE_CANDIDATES}"
        );
    }
    if query.connect_timeout_ms == 0 || query.timeout_ms == 0 {
        bail!("AI gateway timeouts must be greater than zero");
    }
    if query.max_response_bytes == 0 {
        bail!("AI gateway max response bytes must be greater than zero");
    }
    Ok(())
}

fn load_candidates(database: &Database, scan_id: i64) -> Result<Vec<IndexedCandidate>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT
          c.id,
          c.path,
          c.label,
          COALESCE(dt.allocated_size_bytes, e.allocated_size_bytes, 0) AS size_bytes,
          COALESCE(dt.max_mtime, e.mtime)
        FROM classifications c
        LEFT JOIN directory_totals dt
          ON dt.scan_id = c.scan_id
         AND dt.path = c.path
        LEFT JOIN entries e
          ON e.scan_id = c.scan_id
         AND e.path = c.path
        WHERE c.scan_id = ?1
        ORDER BY c.path ASC, c.id ASC
        "#,
    )?;

    let rows = stmt.query_map(params![scan_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            PathBuf::from(row.get::<_, String>(1)?),
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?.max(0) as u64,
            row.get::<_, Option<i64>>(4)?,
        ))
    })?;

    let mut grouped: BTreeMap<PathBuf, IndexedCandidate> = BTreeMap::new();
    for row in rows {
        let (classification_id, path, label, size_bytes, max_mtime) = row?;
        grouped
            .entry(path.clone())
            .and_modify(|candidate| {
                candidate.identity_id = candidate.identity_id.min(classification_id);
                candidate.labels.push(label.clone());
                candidate.size_bytes = candidate.size_bytes.max(size_bytes);
                candidate.max_mtime = max_optional(candidate.max_mtime, max_mtime);
            })
            .or_insert_with(|| IndexedCandidate {
                identity_id: classification_id,
                path,
                labels: vec![label],
                size_bytes,
                max_mtime,
            });
    }

    for candidate in grouped.values_mut() {
        candidate.labels.sort();
        candidate.labels.dedup();
        candidate.labels.truncate(MAX_LABELS_PER_CANDIDATE);
    }

    Ok(grouped.into_values().collect())
}

fn observation_for(
    scan_id: i64,
    candidate: &IndexedCandidate,
    path_mode: AiPathMode,
    max_risk: Risk,
) -> AiObservation {
    let path = privacy_path(&candidate.path, path_mode);
    let classification = candidate.labels.first().cloned();
    let protected = candidate.labels.iter().any(|label| is_protected_label(label));
    let age_seconds = candidate.max_mtime.and_then(|mtime| {
        let age = util::unix_now().saturating_sub(mtime);
        (age >= 0).then_some(age as u64)
    });

    AiObservation {
        scan_id,
        candidate_key: format!("scan-{scan_id}:classification-{}", candidate.identity_id),
        path,
        path_mode,
        size_bytes: candidate.size_bytes,
        age_seconds,
        labels: candidate.labels.clone(),
        deterministic: AiDeterministicFacts {
            classification,
            matched_rule: None,
            protected,
            max_allowed_risk: max_risk.to_string(),
        },
        adapter: None,
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
                return parts
                    .next()
                    .map_or_else(|| format!("<redacted>/{first}"), |child| {
                        format!("<redacted>/{first}/{child}")
                    });
            }
            return format!("<redacted>/{marker}");
        }
    }

    "<redacted>".to_owned()
}

fn is_protected_label(label: &str) -> bool {
    matches!(
        label,
        "secret_material"
            | "git_repo"
            | "database"
            | "vm_image"
            | "browser_profile"
            | "docker_data"
            | "podman_data"
            | "nix_store"
            | "systemd_journal"
    )
}

fn max_optional(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn print_proposal(index: usize, request: &AiAnalysisRequest, proposal: &AiCleanupProposal) {
    println!("candidate {index}:");
    println!("  key: {}", request.observation.candidate_key);
    println!("  path: {}", request.observation.path);
    println!("  size: {}", util::format_bytes(request.observation.size_bytes));
    println!("  observation: {}", request.observation_digest);
    println!("  classification: {}", proposal.classification);
    println!("  confidence: {:.3}", proposal.confidence);
    println!("  risk: {}", ai_risk_name(proposal.risk));
    println!(
        "  recommendation: {}",
        action_name(proposal.recommended_action)
    );
    println!(
        "  provenance: {}/{}",
        proposal.provenance.provider, proposal.provenance.model
    );
    if let Some(request_id) = &proposal.provenance.request_id {
        println!("  request_id: {request_id}");
    }
    println!("  rationale:");
    for rationale in &proposal.rationale {
        println!("    - {rationale}");
    }
    if !proposal.caveats.is_empty() {
        println!("  caveats:");
        for caveat in &proposal.caveats {
            println!("    - {caveat}");
        }
    }
}

fn path_mode_name(mode: AiPathMode) -> &'static str {
    match mode {
        AiPathMode::Full => "full",
        AiPathMode::Basename => "basename",
        AiPathMode::Redacted => "redacted",
    }
}

fn ai_risk_name(risk: AiRisk) -> &'static str {
    match risk {
        AiRisk::Low => "low",
        AiRisk::Medium => "medium",
        AiRisk::High => "high",
    }
}

fn action_name(action: AiRecommendedAction) -> &'static str {
    match action {
        AiRecommendedAction::Ignore => "ignore",
        AiRecommendedAction::Review => "review",
        AiRecommendedAction::Quarantine => "quarantine",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_preserves_known_structure_without_user_prefix() {
        assert_eq!(
            redact_path(Path::new("/home/alice/work/private/.gradle/caches/modules-2")),
            "<redacted>/.gradle/caches"
        );
        assert_eq!(
            redact_path(Path::new("/home/alice/.cache/pip/http-v2")),
            "<redacted>/.cache/pip"
        );
        assert_eq!(
            redact_path(Path::new("/nix/store/abc-secret-package")),
            "/nix/store/<redacted>"
        );
        assert_eq!(
            redact_path(Path::new("/home/alice/work/private-project/target")),
            "<redacted>"
        );
    }

    #[test]
    fn protected_labels_match_planner_sensitive_categories() {
        assert!(is_protected_label("secret_material"));
        assert!(is_protected_label("nix_store"));
        assert!(!is_protected_label("cache"));
    }
}
