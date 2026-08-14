use crate::ai_goal::{AiGoalCandidate, AiGoalRecommendation, AiGoalRequest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

pub const EVALUATION_SUITE_VERSION: u32 = 1;
pub const EVALUATION_BASELINE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalEvaluationSuite {
    pub suite_version: u32,
    pub name: String,
    pub fixtures: Vec<GoalEvaluationFixture>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalEvaluationFixture {
    pub id: String,
    pub description: String,
    pub scan_id: i64,
    pub target_bytes: u64,
    pub max_risk: String,
    pub root: Option<String>,
    pub candidates: Vec<AiGoalCandidate>,
    pub expectations: GoalEvaluationExpectations,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalEvaluationExpectations {
    pub expected_target_met: bool,
    #[serde(default)]
    pub preferred_candidate_ids_any: Vec<i64>,
    #[serde(default)]
    pub avoid_candidate_ids: Vec<i64>,
    pub max_selected_candidates: Option<usize>,
    #[serde(default)]
    pub rationale_terms_any: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationBaseline {
    pub baseline_version: u32,
    pub suite_version: u32,
    pub minimum_average_score: f64,
    pub fixture_minimum_scores: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalSemanticScore {
    pub total_score: f64,
    pub selection_quality: f64,
    pub target_quality: f64,
    pub explanation_relevance: f64,
    pub compactness: f64,
    pub conservatism: f64,
    pub selected_bytes: u64,
    pub target_met: bool,
    pub preferred_hit: bool,
    pub avoided_forbidden_preferences: bool,
    pub selected_count: usize,
    pub matched_rationale_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationValidationError {
    UnsupportedSuiteVersion(u32),
    UnsupportedBaselineVersion(u32),
    BaselineSuiteVersionMismatch { baseline: u32, suite: u32 },
    EmptySuiteName,
    EmptyFixtureId,
    DuplicateFixtureId(String),
    InvalidBaselineScore(String),
    InvalidFixture(String),
    UnknownSelectedCandidateId(i64),
    ReclaimTotalOverflow,
}

impl fmt::Display for EvaluationValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSuiteVersion(version) => {
                write!(f, "unsupported evaluation suite version: {version}")
            }
            Self::UnsupportedBaselineVersion(version) => {
                write!(f, "unsupported evaluation baseline version: {version}")
            }
            Self::BaselineSuiteVersionMismatch { baseline, suite } => write!(
                f,
                "evaluation baseline suite version {baseline} does not match fixture suite version {suite}"
            ),
            Self::EmptySuiteName => write!(f, "evaluation suite name must not be empty"),
            Self::EmptyFixtureId => write!(f, "evaluation fixture id must not be empty"),
            Self::DuplicateFixtureId(id) => write!(f, "duplicate evaluation fixture id {id:?}"),
            Self::InvalidBaselineScore(field) => {
                write!(f, "evaluation baseline score is outside 0..=100: {field}")
            }
            Self::InvalidFixture(message) => write!(f, "invalid evaluation fixture: {message}"),
            Self::UnknownSelectedCandidateId(id) => {
                write!(f, "evaluation recommendation selected unknown candidate id {id}")
            }
            Self::ReclaimTotalOverflow => write!(f, "evaluation selected-byte total overflowed u64"),
        }
    }
}

impl Error for EvaluationValidationError {}

impl GoalEvaluationSuite {
    pub fn validate(&self) -> Result<(), EvaluationValidationError> {
        if self.suite_version != EVALUATION_SUITE_VERSION {
            return Err(EvaluationValidationError::UnsupportedSuiteVersion(
                self.suite_version,
            ));
        }
        if self.name.trim().is_empty() {
            return Err(EvaluationValidationError::EmptySuiteName);
        }

        let mut fixture_ids = BTreeSet::new();
        for fixture in &self.fixtures {
            if fixture.id.trim().is_empty() {
                return Err(EvaluationValidationError::EmptyFixtureId);
            }
            if !fixture_ids.insert(fixture.id.clone()) {
                return Err(EvaluationValidationError::DuplicateFixtureId(
                    fixture.id.clone(),
                ));
            }
            fixture.validate()?;
        }
        Ok(())
    }
}

impl GoalEvaluationFixture {
    pub fn request(&self, request_id: String) -> Result<AiGoalRequest, EvaluationValidationError> {
        let request = AiGoalRequest::new(
            request_id,
            self.scan_id,
            self.candidates.clone(),
            self.target_bytes,
            self.max_risk.clone(),
            self.root.clone(),
        );
        request.validate().map_err(|error| {
            EvaluationValidationError::InvalidFixture(format!("{}: {error}", self.id))
        })?;
        Ok(request)
    }

    fn validate(&self) -> Result<(), EvaluationValidationError> {
        self.request(format!("eval-fixture:{}", self.id))?;

        let known_ids: BTreeSet<_> = self
            .candidates
            .iter()
            .map(|candidate| candidate.candidate_id)
            .collect();
        for id in self
            .expectations
            .preferred_candidate_ids_any
            .iter()
            .chain(self.expectations.avoid_candidate_ids.iter())
        {
            if !known_ids.contains(id) {
                return Err(EvaluationValidationError::InvalidFixture(format!(
                    "{} expectation references unknown candidate id {id}",
                    self.id
                )));
            }
        }
        if self
            .expectations
            .max_selected_candidates
            .is_some_and(|limit| limit == 0 || limit > self.candidates.len())
        {
            return Err(EvaluationValidationError::InvalidFixture(format!(
                "{} max_selected_candidates is outside 1..={}",
                self.id,
                self.candidates.len()
            )));
        }
        if self
            .expectations
            .rationale_terms_any
            .iter()
            .any(|term| term.trim().is_empty())
        {
            return Err(EvaluationValidationError::InvalidFixture(format!(
                "{} contains an empty rationale term",
                self.id
            )));
        }
        Ok(())
    }
}

impl EvaluationBaseline {
    pub fn validate_for(&self, suite: &GoalEvaluationSuite) -> Result<(), EvaluationValidationError> {
        if self.baseline_version != EVALUATION_BASELINE_VERSION {
            return Err(EvaluationValidationError::UnsupportedBaselineVersion(
                self.baseline_version,
            ));
        }
        if self.suite_version != suite.suite_version {
            return Err(EvaluationValidationError::BaselineSuiteVersionMismatch {
                baseline: self.suite_version,
                suite: suite.suite_version,
            });
        }
        if !valid_score(self.minimum_average_score) {
            return Err(EvaluationValidationError::InvalidBaselineScore(
                "minimum_average_score".to_owned(),
            ));
        }
        let fixture_ids: BTreeSet<_> = suite.fixtures.iter().map(|fixture| &fixture.id).collect();
        for (id, score) in &self.fixture_minimum_scores {
            if !fixture_ids.contains(id) {
                return Err(EvaluationValidationError::InvalidFixture(format!(
                    "baseline references unknown fixture {id:?}"
                )));
            }
            if !valid_score(*score) {
                return Err(EvaluationValidationError::InvalidBaselineScore(format!(
                    "fixture_minimum_scores.{id}"
                )));
            }
        }
        Ok(())
    }
}

pub fn score_goal_recommendation(
    fixture: &GoalEvaluationFixture,
    recommendation: &AiGoalRecommendation,
) -> Result<GoalSemanticScore, EvaluationValidationError> {
    let by_id: BTreeMap<_, _> = fixture
        .candidates
        .iter()
        .map(|candidate| (candidate.candidate_id, candidate))
        .collect();

    let mut selected_bytes = 0_u64;
    for id in &recommendation.selected_candidate_ids {
        let candidate = by_id
            .get(id)
            .ok_or(EvaluationValidationError::UnknownSelectedCandidateId(*id))?;
        selected_bytes = selected_bytes
            .checked_add(candidate.size_bytes)
            .ok_or(EvaluationValidationError::ReclaimTotalOverflow)?;
    }
    let target_met = selected_bytes >= fixture.target_bytes;

    let selected_ids: BTreeSet<_> = recommendation.selected_candidate_ids.iter().copied().collect();
    let preferred_hit = fixture.expectations.preferred_candidate_ids_any.is_empty()
        || fixture
            .expectations
            .preferred_candidate_ids_any
            .iter()
            .any(|id| selected_ids.contains(id));
    let avoided_forbidden_preferences = fixture
        .expectations
        .avoid_candidate_ids
        .iter()
        .all(|id| !selected_ids.contains(id));

    let selection_quality = if preferred_hit && avoided_forbidden_preferences {
        100.0
    } else if preferred_hit || avoided_forbidden_preferences {
        50.0
    } else {
        0.0
    };

    let target_quality = if target_met == fixture.expectations.expected_target_met {
        100.0
    } else {
        0.0
    };

    let combined_explanation = recommendation
        .rationale
        .iter()
        .chain(recommendation.caveats.iter())
        .map(|item| item.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let mut matched_rationale_terms = Vec::new();
    for term in &fixture.expectations.rationale_terms_any {
        if combined_explanation.contains(&term.to_lowercase()) {
            matched_rationale_terms.push(term.clone());
        }
    }
    let explanation_relevance = if fixture.expectations.rationale_terms_any.is_empty() {
        100.0
    } else {
        (matched_rationale_terms.len() as f64
            / fixture.expectations.rationale_terms_any.len() as f64)
            * 100.0
    };

    let compactness = match fixture.expectations.max_selected_candidates {
        Some(limit) if recommendation.selected_candidate_ids.len() <= limit => 100.0,
        Some(_) => 0.0,
        None => 100.0,
    };

    let conservatism = if fixture.expectations.expected_target_met {
        if target_met { 100.0 } else { 0.0 }
    } else {
        100.0
    };

    let total_score = selection_quality * 0.25
        + target_quality * 0.30
        + explanation_relevance * 0.20
        + compactness * 0.10
        + conservatism * 0.15;

    Ok(GoalSemanticScore {
        total_score,
        selection_quality,
        target_quality,
        explanation_relevance,
        compactness,
        conservatism,
        selected_bytes,
        target_met,
        preferred_hit,
        avoided_forbidden_preferences,
        selected_count: recommendation.selected_candidate_ids.len(),
        matched_rationale_terms,
    })
}

fn valid_score(score: f64) -> bool {
    score.is_finite() && (0.0..=100.0).contains(&score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::AiProvenance;
    use crate::ai_contract::AiPathMode;
    use crate::ai_goal::AI_GOAL_SCHEMA_VERSION;

    fn fixture() -> GoalEvaluationFixture {
        GoalEvaluationFixture {
            id: "compact-cache-target".to_owned(),
            description: "prefer one compact cache candidate".to_owned(),
            scan_id: 42,
            target_bytes: 8_000,
            max_risk: "low".to_owned(),
            root: Some("<redacted>/.cache".to_owned()),
            candidates: vec![
                AiGoalCandidate {
                    candidate_id: 7,
                    path: "<redacted>/.cache/pip".to_owned(),
                    path_mode: AiPathMode::Redacted,
                    size_bytes: 10_000,
                    risk: "low".to_owned(),
                    rule_id: "pip-cache".to_owned(),
                    category: "cache".to_owned(),
                },
                AiGoalCandidate {
                    candidate_id: 8,
                    path: "<redacted>/.cache/uv".to_owned(),
                    path_mode: AiPathMode::Redacted,
                    size_bytes: 5_000,
                    risk: "low".to_owned(),
                    rule_id: "uv-cache".to_owned(),
                    category: "cache".to_owned(),
                },
            ],
            expectations: GoalEvaluationExpectations {
                expected_target_met: true,
                preferred_candidate_ids_any: vec![7],
                avoid_candidate_ids: vec![],
                max_selected_candidates: Some(1),
                rationale_terms_any: vec!["cache".to_owned(), "target".to_owned()],
            },
        }
    }

    fn recommendation(ids: Vec<i64>, rationale: &str) -> AiGoalRecommendation {
        AiGoalRecommendation {
            schema_version: AI_GOAL_SCHEMA_VERSION,
            selected_candidate_ids: ids,
            rationale: vec![rationale.to_owned()],
            caveats: vec![],
            provenance: AiProvenance {
                provider: "fixture".to_owned(),
                model: "test".to_owned(),
                request_id: Some("eval-fixture".to_owned()),
            },
        }
    }

    #[test]
    fn validates_fixture_and_baseline_contracts() {
        let suite = GoalEvaluationSuite {
            suite_version: EVALUATION_SUITE_VERSION,
            name: "goal-quality".to_owned(),
            fixtures: vec![fixture()],
        };
        suite.validate().expect("valid fixture suite");

        let baseline = EvaluationBaseline {
            baseline_version: EVALUATION_BASELINE_VERSION,
            suite_version: EVALUATION_SUITE_VERSION,
            minimum_average_score: 75.0,
            fixture_minimum_scores: BTreeMap::from([("compact-cache-target".to_owned(), 70.0)]),
        };
        baseline.validate_for(&suite).expect("valid baseline");
    }

    #[test]
    fn strong_recommendation_scores_full_marks() {
        let score = score_goal_recommendation(
            &fixture(),
            &recommendation(vec![7], "cache selection meets the target"),
        )
        .expect("score recommendation");
        assert_eq!(score.total_score, 100.0);
        assert!(score.target_met);
    }

    #[test]
    fn conservative_irrelevant_recommendation_scores_lower() {
        let score = score_goal_recommendation(
            &fixture(),
            &recommendation(vec![8], "small candidate"),
        )
        .expect("score recommendation");
        assert!(score.total_score < 50.0);
        assert!(!score.target_met);
        assert!(!score.preferred_hit);
    }
}
