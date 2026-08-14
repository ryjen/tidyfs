use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tidyfs::ai_gateway::{LoopbackGatewayConfig, LoopbackGatewayProvider};
use tidyfs::ai_provider::AiProviderError;
use tidyfs::evaluation::{
    score_goal_recommendation, EvaluationBaseline, GoalEvaluationSuite, GoalSemanticScore,
};

const REPORT_VERSION: u32 = 1;

#[derive(Debug, Parser)]
#[command(about = "Run optional live-model quality evaluations against a local tidyfs goal gateway")]
struct Args {
    /// Numeric-loopback gateway endpoint, for example http://127.0.0.1:8000.
    #[arg(long)]
    endpoint: String,

    /// Versioned evaluation fixture suite.
    #[arg(long, default_value = "eval/fixtures/goal-recommendations-v1.json")]
    fixtures: PathBuf,

    /// Pinned semantic score thresholds.
    #[arg(long, default_value = "eval/baselines/goal-recommendations-v1.json")]
    baseline: PathBuf,

    /// Machine-readable report path.
    #[arg(long, default_value = "eval-results.json")]
    json_out: PathBuf,

    /// Optional prior report to compare for score/provider/model drift.
    #[arg(long)]
    compare: Option<PathBuf>,

    #[arg(long, default_value_t = 3000)]
    connect_timeout_ms: u64,

    #[arg(long, default_value_t = 15000)]
    timeout_ms: u64,

    #[arg(long, default_value_t = 65536)]
    max_response_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct ProviderModel {
    provider: String,
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeProvenance {
    tidyfs_version: String,
    endpoint: String,
    os: String,
    arch: String,
    started_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixtureResult {
    fixture_id: String,
    status: String,
    latency_ms: u64,
    provider: Option<String>,
    model: Option<String>,
    request_id: Option<String>,
    selected_candidate_ids: Vec<i64>,
    rationale: Vec<String>,
    caveats: Vec<String>,
    score: Option<GoalSemanticScore>,
    baseline_minimum_score: f64,
    baseline_met: Option<bool>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvaluationSummary {
    fixture_count: usize,
    scored_fixtures: usize,
    semantic_regressions: usize,
    contract_failures: usize,
    provider_errors: usize,
    average_score: Option<f64>,
    baseline_minimum_average_score: f64,
    baseline_average_met: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DriftComparison {
    previous_report: String,
    provider_model_changed: bool,
    previous_provider_models: Vec<ProviderModel>,
    current_provider_models: Vec<ProviderModel>,
    average_score_delta: Option<f64>,
    fixture_score_deltas: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvaluationReport {
    report_version: u32,
    suite_version: u32,
    suite_name: String,
    runtime: RuntimeProvenance,
    provider_models: Vec<ProviderModel>,
    fixtures: Vec<FixtureResult>,
    summary: EvaluationSummary,
    drift: Option<DriftComparison>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.connect_timeout_ms == 0 || args.timeout_ms == 0 || args.max_response_bytes == 0 {
        bail!("evaluation gateway timeout/response bounds must be greater than zero");
    }

    let suite: GoalEvaluationSuite = read_json(&args.fixtures)?;
    suite.validate().context("validating evaluation fixtures")?;
    let baseline: EvaluationBaseline = read_json(&args.baseline)?;
    baseline
        .validate_for(&suite)
        .context("validating evaluation baseline")?;

    let started_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut config = LoopbackGatewayConfig::from_endpoint(&args.endpoint)
        .context("validating evaluation loopback endpoint")?;
    config.connect_timeout = Duration::from_millis(args.connect_timeout_ms);
    config.io_timeout = Duration::from_millis(args.timeout_ms);
    config.max_response_bytes = args.max_response_bytes;
    let provider = LoopbackGatewayProvider::new(config);

    let mut fixture_results = Vec::with_capacity(suite.fixtures.len());
    let mut provider_models = BTreeSet::new();

    for fixture in &suite.fixtures {
        let request_id = format!("eval-v1:{}:{started_at_unix}", fixture.id);
        let request = fixture
            .request(request_id)
            .with_context(|| format!("building evaluation request for {}", fixture.id))?;
        let started = Instant::now();
        let response = provider.recommend_goal(&request);
        let latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let baseline_minimum_score = baseline
            .fixture_minimum_scores
            .get(&fixture.id)
            .copied()
            .unwrap_or(0.0);

        match response {
            Ok(recommendation) => {
                let score = score_goal_recommendation(fixture, &recommendation)
                    .with_context(|| format!("scoring evaluation fixture {}", fixture.id))?;
                let baseline_met = score.total_score >= baseline_minimum_score;
                provider_models.insert(ProviderModel {
                    provider: recommendation.provenance.provider.clone(),
                    model: recommendation.provenance.model.clone(),
                });
                fixture_results.push(FixtureResult {
                    fixture_id: fixture.id.clone(),
                    status: if baseline_met {
                        "ok".to_owned()
                    } else {
                        "semantic_regression".to_owned()
                    },
                    latency_ms,
                    provider: Some(recommendation.provenance.provider.clone()),
                    model: Some(recommendation.provenance.model.clone()),
                    request_id: recommendation.provenance.request_id.clone(),
                    selected_candidate_ids: recommendation.selected_candidate_ids.clone(),
                    rationale: recommendation.rationale.clone(),
                    caveats: recommendation.caveats.clone(),
                    score: Some(score),
                    baseline_minimum_score,
                    baseline_met: Some(baseline_met),
                    error: None,
                });
            }
            Err(AiProviderError::Unavailable(error)) => fixture_results.push(FixtureResult {
                fixture_id: fixture.id.clone(),
                status: "provider_error".to_owned(),
                latency_ms,
                provider: None,
                model: None,
                request_id: None,
                selected_candidate_ids: vec![],
                rationale: vec![],
                caveats: vec![],
                score: None,
                baseline_minimum_score,
                baseline_met: None,
                error: Some(error),
            }),
            Err(AiProviderError::InvalidResponse(error)) => fixture_results.push(FixtureResult {
                fixture_id: fixture.id.clone(),
                status: "contract_failure".to_owned(),
                latency_ms,
                provider: None,
                model: None,
                request_id: None,
                selected_candidate_ids: vec![],
                rationale: vec![],
                caveats: vec![],
                score: None,
                baseline_minimum_score,
                baseline_met: None,
                error: Some(error),
            }),
        }
    }

    let provider_models: Vec<_> = provider_models.into_iter().collect();
    let summary = summarize(&fixture_results, &baseline);
    let runtime = RuntimeProvenance {
        tidyfs_version: env!("CARGO_PKG_VERSION").to_owned(),
        endpoint: args.endpoint.clone(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        started_at_unix,
    };

    let mut report = EvaluationReport {
        report_version: REPORT_VERSION,
        suite_version: suite.suite_version,
        suite_name: suite.name,
        runtime,
        provider_models,
        fixtures: fixture_results,
        summary,
        drift: None,
    };

    if let Some(path) = &args.compare {
        let previous: EvaluationReport = read_json(path)?;
        if previous.report_version != REPORT_VERSION {
            bail!(
                "cannot compare evaluation report version {} with supported version {}",
                previous.report_version,
                REPORT_VERSION
            );
        }
        if previous.suite_version != report.suite_version
            || previous.suite_name != report.suite_name
        {
            bail!("comparison report uses a different evaluation suite");
        }
        report.drift = Some(compare_reports(path, &previous, &report));
    }

    write_report(&args.json_out, &report)?;
    print_report(&args.json_out, &report);

    if report.summary.contract_failures > 0 {
        bail!(
            "evaluation observed {} contract failure(s); convert any tidyfs defect into a deterministic regression test",
            report.summary.contract_failures
        );
    }
    if report.summary.provider_errors > 0 {
        bail!(
            "evaluation provider was unavailable for {} fixture(s)",
            report.summary.provider_errors
        );
    }

    // Semantic regressions are visible in the report but intentionally do not fail this
    // optional evaluation command. They represent model-quality drift, not cleanup authority.
    Ok(())
}

fn summarize(results: &[FixtureResult], baseline: &EvaluationBaseline) -> EvaluationSummary {
    let scores: Vec<_> = results
        .iter()
        .filter_map(|result| result.score.as_ref().map(|score| score.total_score))
        .collect();
    let average_score = (!scores.is_empty())
        .then(|| scores.iter().sum::<f64>() / scores.len() as f64);

    EvaluationSummary {
        fixture_count: results.len(),
        scored_fixtures: scores.len(),
        semantic_regressions: results
            .iter()
            .filter(|result| result.status == "semantic_regression")
            .count(),
        contract_failures: results
            .iter()
            .filter(|result| result.status == "contract_failure")
            .count(),
        provider_errors: results
            .iter()
            .filter(|result| result.status == "provider_error")
            .count(),
        average_score,
        baseline_minimum_average_score: baseline.minimum_average_score,
        baseline_average_met: average_score.map(|score| score >= baseline.minimum_average_score),
    }
}

fn compare_reports(
    path: &Path,
    previous: &EvaluationReport,
    current: &EvaluationReport,
) -> DriftComparison {
    let previous_scores: BTreeMap<_, _> = previous
        .fixtures
        .iter()
        .filter_map(|fixture| {
            fixture
                .score
                .as_ref()
                .map(|score| (fixture.fixture_id.clone(), score.total_score))
        })
        .collect();
    let fixture_score_deltas = current
        .fixtures
        .iter()
        .filter_map(|fixture| {
            let current_score = fixture.score.as_ref()?.total_score;
            let previous_score = previous_scores.get(&fixture.fixture_id)?;
            Some((fixture.fixture_id.clone(), current_score - previous_score))
        })
        .collect();

    DriftComparison {
        previous_report: path.display().to_string(),
        provider_model_changed: previous.provider_models != current.provider_models,
        previous_provider_models: previous.provider_models.clone(),
        current_provider_models: current.provider_models.clone(),
        average_score_delta: match (
            previous.summary.average_score,
            current.summary.average_score,
        ) {
            (Some(previous), Some(current)) => Some(current - previous),
            _ => None,
        },
        fixture_score_deltas,
    }
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn write_report(path: &Path, report: &EvaluationReport) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating report directory {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(report).context("serializing evaluation report")?;
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn print_report(path: &Path, report: &EvaluationReport) {
    println!("Live goal recommendation evaluation");
    println!("suite: {} v{}", report.suite_name, report.suite_version);
    println!("tidyfs: {}", report.runtime.tidyfs_version);
    println!("endpoint: {}", report.runtime.endpoint);
    println!("runtime: {}/{}", report.runtime.os, report.runtime.arch);
    if report.provider_models.is_empty() {
        println!("provider/model: unavailable");
    } else {
        for item in &report.provider_models {
            println!("provider/model: {}/{}", item.provider, item.model);
        }
    }
    println!();

    for fixture in &report.fixtures {
        match &fixture.score {
            Some(score) => println!(
                "{}: {} score={:.1} baseline={:.1} selected={} bytes={} target_met={} latency={}ms",
                fixture.fixture_id,
                fixture.status,
                score.total_score,
                fixture.baseline_minimum_score,
                score.selected_count,
                score.selected_bytes,
                score.target_met,
                fixture.latency_ms
            ),
            None => println!(
                "{}: {} latency={}ms error={}",
                fixture.fixture_id,
                fixture.status,
                fixture.latency_ms,
                fixture.error.as_deref().unwrap_or("unknown")
            ),
        }
    }

    println!();
    if let Some(score) = report.summary.average_score {
        println!(
            "average semantic score: {:.1} (baseline {:.1}, met={})",
            score,
            report.summary.baseline_minimum_average_score,
            report.summary.baseline_average_met.unwrap_or(false)
        );
    } else {
        println!("average semantic score: unavailable");
    }
    println!(
        "semantic_regressions={} contract_failures={} provider_errors={}",
        report.summary.semantic_regressions,
        report.summary.contract_failures,
        report.summary.provider_errors
    );
    if let Some(drift) = &report.drift {
        println!(
            "comparison: provider_model_changed={} average_score_delta={}",
            drift.provider_model_changed,
            drift
                .average_score_delta
                .map(|value| format!("{value:+.1}"))
                .unwrap_or_else(|| "n/a".to_owned())
        );
    }
    println!("json_report: {}", path.display());
}
