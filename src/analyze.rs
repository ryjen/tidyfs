use crate::ai_facts;
use crate::db::Database;
use crate::rules::Risk;
use crate::util;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tidyfs::ai::{AiCleanupProposal, AiRecommendedAction, AiRisk};
use tidyfs::ai_contract::AiPathMode;
use tidyfs::ai_gateway::{LoopbackGatewayConfig, LoopbackGatewayProvider};
use tidyfs::ai_provider::{analyze_validated, AiAnalysisRequest};

const MAX_ANALYZE_CANDIDATES: usize = 100;

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

    let mut candidates = ai_facts::load_candidates(database, scan.id)?;
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
    println!("path_mode: {}", ai_facts::path_mode_name(query.path_mode));
    println!("candidates: {}", candidates.len());
    println!("mutation_authority: false");

    if candidates.is_empty() {
        println!("no classified candidates matched the selected scan/root");
        return Ok(());
    }

    for (index, candidate) in candidates.iter().enumerate() {
        let observation =
            ai_facts::observation_for(scan.id, candidate, query.path_mode, query.max_risk);
        let request = AiAnalysisRequest::new(observation);
        let proposal = analyze_validated(&provider, &request).with_context(|| {
            format!(
                "AI analysis failed for candidate {}",
                request.observation.candidate_key
            )
        })?;

        ai_facts::reconstruct_bound_observation(
            database,
            scan.id,
            &candidate.path,
            query.path_mode,
            query.max_risk,
            &request.observation_digest,
        )
        .with_context(|| {
            format!(
                "AI analysis became stale before it could be accepted for candidate {}",
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
        bail!("analyze --limit must be between 1 and {MAX_ANALYZE_CANDIDATES}");
    }
    if query.connect_timeout_ms == 0 || query.timeout_ms == 0 {
        bail!("AI gateway timeouts must be greater than zero");
    }
    if query.max_response_bytes == 0 {
        bail!("AI gateway max response bytes must be greater than zero");
    }
    Ok(())
}

fn print_proposal(index: usize, request: &AiAnalysisRequest, proposal: &AiCleanupProposal) {
    println!("candidate {index}:");
    println!("  key: {}", request.observation.candidate_key);
    println!("  path: {}", request.observation.path);
    println!(
        "  size: {}",
        util::format_bytes(request.observation.size_bytes)
    );
    println!("  observation: {}", request.observation_digest);
    println!(
        "  classification: {}",
        util::terminal_safe(&proposal.classification)
    );
    println!("  confidence: {:.3}", proposal.confidence);
    println!("  risk: {}", ai_risk_name(proposal.risk));
    println!(
        "  recommendation: {}",
        action_name(proposal.recommended_action)
    );
    println!(
        "  provenance: {}/{}",
        util::terminal_safe(&proposal.provenance.provider),
        util::terminal_safe(&proposal.provenance.model)
    );
    if let Some(request_id) = &proposal.provenance.request_id {
        println!("  request_id: {}", util::terminal_safe(request_id));
    }
    println!("  rationale:");
    for rationale in &proposal.rationale {
        println!("    - {}", util::terminal_safe(rationale));
    }
    if !proposal.caveats.is_empty() {
        println!("  caveats:");
        for caveat in &proposal.caveats {
            println!("    - {}", util::terminal_safe(caveat));
        }
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
