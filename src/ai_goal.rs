use crate::ai::{
    AiProvenance, AI_MAX_CAVEAT_ITEMS, AI_MAX_EXPLANATION_ITEM_BYTES, AI_MAX_PROVENANCE_FIELD_BYTES,
    AI_MAX_RATIONALE_ITEMS,
};
use crate::ai_contract::AiPathMode;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, hash_map::DefaultHasher};
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};

pub const AI_GOAL_CONTRACT_VERSION: u32 = 1;
pub const AI_GOAL_SCHEMA_VERSION: u32 = 1;
pub const AI_GOAL_TASK: &str = "cleanup_goal_recommendation";
pub const AI_MAX_GOAL_CANDIDATES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiGoalCandidate {
    pub candidate_id: i64,
    pub path: String,
    pub path_mode: AiPathMode,
    pub size_bytes: u64,
    pub risk: String,
    pub rule_id: String,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiGoalConstraints {
    pub target_bytes: u64,
    pub max_risk: String,
    pub root: Option<String>,
    pub file_contents_available: bool,
    pub mutation_authority: bool,
    pub candidate_creation_authority: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiGoalRequest {
    pub contract_version: u32,
    pub request_id: String,
    pub task: String,
    pub scan_id: i64,
    pub candidates: Vec<AiGoalCandidate>,
    pub constraints: AiGoalConstraints,
    pub plan_digest: String,
}

impl AiGoalRequest {
    pub fn new(
        request_id: String,
        scan_id: i64,
        candidates: Vec<AiGoalCandidate>,
        target_bytes: u64,
        max_risk: String,
        root: Option<String>,
    ) -> Self {
        let constraints = AiGoalConstraints {
            target_bytes,
            max_risk,
            root,
            file_contents_available: false,
            mutation_authority: false,
            candidate_creation_authority: false,
        };
        let plan_digest = goal_plan_digest(scan_id, &candidates, &constraints);
        Self {
            contract_version: AI_GOAL_CONTRACT_VERSION,
            request_id,
            task: AI_GOAL_TASK.to_owned(),
            scan_id,
            candidates,
            constraints,
            plan_digest,
        }
    }

    pub fn binding_is_current(&self) -> bool {
        self.plan_digest == goal_plan_digest(self.scan_id, &self.candidates, &self.constraints)
    }

    pub fn validate(&self) -> Result<(), AiGoalValidationError> {
        if self.contract_version != AI_GOAL_CONTRACT_VERSION {
            return Err(AiGoalValidationError::UnsupportedContractVersion(
                self.contract_version,
            ));
        }
        if self.request_id.trim().is_empty() {
            return Err(AiGoalValidationError::EmptyRequestId);
        }
        if self.task != AI_GOAL_TASK {
            return Err(AiGoalValidationError::UnexpectedTask);
        }
        if self.constraints.target_bytes == 0 {
            return Err(AiGoalValidationError::ZeroTarget);
        }
        if self.candidates.is_empty() {
            return Err(AiGoalValidationError::NoCandidates);
        }
        if self.candidates.len() > AI_MAX_GOAL_CANDIDATES {
            return Err(AiGoalValidationError::TooManyCandidates);
        }
        if self.constraints.file_contents_available
            || self.constraints.mutation_authority
            || self.constraints.candidate_creation_authority
        {
            return Err(AiGoalValidationError::UnsafeConstraint);
        }
        if !self.binding_is_current() {
            return Err(AiGoalValidationError::PlanDigestMismatch);
        }

        let mut ids = BTreeSet::new();
        for candidate in &self.candidates {
            if candidate.candidate_id <= 0 {
                return Err(AiGoalValidationError::InvalidCandidateId);
            }
            if !ids.insert(candidate.candidate_id) {
                return Err(AiGoalValidationError::DuplicateCandidateId(
                    candidate.candidate_id,
                ));
            }
            if candidate.risk.trim().is_empty()
                || candidate.rule_id.trim().is_empty()
                || candidate.category.trim().is_empty()
            {
                return Err(AiGoalValidationError::InvalidCandidateFacts);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiGoalRecommendation {
    pub schema_version: u32,
    pub selected_candidate_ids: Vec<i64>,
    pub rationale: Vec<String>,
    #[serde(default)]
    pub caveats: Vec<String>,
    pub provenance: AiProvenance,
}

impl AiGoalRecommendation {
    pub fn validate(&self) -> Result<(), AiGoalValidationError> {
        if self.schema_version != AI_GOAL_SCHEMA_VERSION {
            return Err(AiGoalValidationError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        if self.selected_candidate_ids.len() > AI_MAX_GOAL_CANDIDATES {
            return Err(AiGoalValidationError::TooManySelectedCandidates);
        }
        let mut ids = BTreeSet::new();
        for id in &self.selected_candidate_ids {
            if *id <= 0 {
                return Err(AiGoalValidationError::InvalidCandidateId);
            }
            if !ids.insert(*id) {
                return Err(AiGoalValidationError::DuplicateSelectedCandidateId(*id));
            }
        }

        validate_explanation(&self.rationale, &self.caveats)?;
        validate_provenance(&self.provenance)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiGoalTransportResponse {
    pub contract_version: u32,
    pub request_id: String,
    pub plan_digest: String,
    pub recommendation: AiGoalRecommendation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiGoalValidationError {
    UnsupportedContractVersion(u32),
    UnsupportedSchemaVersion(u32),
    EmptyRequestId,
    UnexpectedTask,
    ZeroTarget,
    NoCandidates,
    TooManyCandidates,
    TooManySelectedCandidates,
    UnsafeConstraint,
    PlanDigestMismatch,
    RequestIdMismatch,
    InvalidCandidateId,
    DuplicateCandidateId(i64),
    DuplicateSelectedCandidateId(i64),
    UnknownSelectedCandidateId(i64),
    InvalidCandidateFacts,
    MissingRationale,
    TooManyRationaleItems,
    RationaleItemTooLong,
    TooManyCaveatItems,
    CaveatItemTooLong,
    EmptyProvider,
    EmptyModel,
    ProvenanceFieldTooLong,
    ProvenanceRequestIdMismatch,
}

impl fmt::Display for AiGoalValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedContractVersion(version) => {
                write!(f, "unsupported AI goal contract version: {version}")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(f, "unsupported AI goal recommendation schema version: {version}")
            }
            Self::EmptyRequestId => write!(f, "AI goal request id must not be empty"),
            Self::UnexpectedTask => write!(f, "AI goal request task is invalid"),
            Self::ZeroTarget => write!(f, "AI goal target bytes must be greater than zero"),
            Self::NoCandidates => write!(f, "AI goal request contains no eligible candidates"),
            Self::TooManyCandidates => write!(
                f,
                "AI goal request exceeds {AI_MAX_GOAL_CANDIDATES} candidates"
            ),
            Self::TooManySelectedCandidates => write!(
                f,
                "AI goal recommendation exceeds {AI_MAX_GOAL_CANDIDATES} selected candidates"
            ),
            Self::UnsafeConstraint => write!(f, "AI goal request grants unsupported authority"),
            Self::PlanDigestMismatch => write!(f, "AI goal plan digest does not match request facts"),
            Self::RequestIdMismatch => write!(f, "AI goal response request id does not match request"),
            Self::InvalidCandidateId => write!(f, "AI goal candidate id must be positive"),
            Self::DuplicateCandidateId(id) => {
                write!(f, "AI goal request contains duplicate candidate id {id}")
            }
            Self::DuplicateSelectedCandidateId(id) => {
                write!(f, "AI goal response selected duplicate candidate id {id}")
            }
            Self::UnknownSelectedCandidateId(id) => {
                write!(f, "AI goal response selected unknown candidate id {id}")
            }
            Self::InvalidCandidateFacts => write!(f, "AI goal candidate facts are incomplete"),
            Self::MissingRationale => write!(f, "AI goal recommendation requires rationale"),
            Self::TooManyRationaleItems => write!(f, "AI goal recommendation has too many rationale items"),
            Self::RationaleItemTooLong => write!(f, "AI goal recommendation rationale item is too long"),
            Self::TooManyCaveatItems => write!(f, "AI goal recommendation has too many caveat items"),
            Self::CaveatItemTooLong => write!(f, "AI goal recommendation caveat item is too long"),
            Self::EmptyProvider => write!(f, "AI goal provenance provider must not be empty"),
            Self::EmptyModel => write!(f, "AI goal provenance model must not be empty"),
            Self::ProvenanceFieldTooLong => write!(f, "AI goal provenance field is too long"),
            Self::ProvenanceRequestIdMismatch => {
                write!(f, "AI goal provenance request id does not match response request id")
            }
        }
    }
}

impl Error for AiGoalValidationError {}

pub fn validate_goal_response(
    request: &AiGoalRequest,
    response: AiGoalTransportResponse,
) -> Result<AiGoalRecommendation, AiGoalValidationError> {
    request.validate()?;
    if response.contract_version != AI_GOAL_CONTRACT_VERSION {
        return Err(AiGoalValidationError::UnsupportedContractVersion(
            response.contract_version,
        ));
    }
    if response.request_id != request.request_id {
        return Err(AiGoalValidationError::RequestIdMismatch);
    }
    if response.plan_digest != request.plan_digest {
        return Err(AiGoalValidationError::PlanDigestMismatch);
    }
    if response.recommendation.provenance.request_id.as_deref()
        != Some(response.request_id.as_str())
    {
        return Err(AiGoalValidationError::ProvenanceRequestIdMismatch);
    }
    response.recommendation.validate()?;

    let allowed: BTreeSet<_> = request
        .candidates
        .iter()
        .map(|candidate| candidate.candidate_id)
        .collect();
    for id in &response.recommendation.selected_candidate_ids {
        if !allowed.contains(id) {
            return Err(AiGoalValidationError::UnknownSelectedCandidateId(*id));
        }
    }
    Ok(response.recommendation)
}

pub fn goal_plan_digest(
    scan_id: i64,
    candidates: &[AiGoalCandidate],
    constraints: &AiGoalConstraints,
) -> String {
    // This is an opaque correlation/freshness digest, not an authenticity token. The
    // authoritative safety check is a post-inference re-read and exact fact comparison.
    let mut ordered = candidates.to_vec();
    ordered.sort_by_key(|candidate| candidate.candidate_id);

    let left = hash_goal_binding(
        "tidyfs-ai-goal-v1:left",
        scan_id,
        &ordered,
        constraints,
    );
    let right = hash_goal_binding(
        "tidyfs-ai-goal-v1:right",
        scan_id,
        &ordered,
        constraints,
    );
    format!("goal-v1:{left:016x}{right:016x}")
}

fn hash_goal_binding(
    domain: &str,
    scan_id: i64,
    candidates: &[AiGoalCandidate],
    constraints: &AiGoalConstraints,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    domain.hash(&mut hasher);
    scan_id.hash(&mut hasher);
    constraints.target_bytes.hash(&mut hasher);
    constraints.max_risk.hash(&mut hasher);
    constraints.root.hash(&mut hasher);
    constraints.file_contents_available.hash(&mut hasher);
    constraints.mutation_authority.hash(&mut hasher);
    constraints.candidate_creation_authority.hash(&mut hasher);
    candidates.len().hash(&mut hasher);
    for candidate in candidates {
        candidate.candidate_id.hash(&mut hasher);
        candidate.path.hash(&mut hasher);
        path_mode_name(candidate.path_mode).hash(&mut hasher);
        candidate.size_bytes.hash(&mut hasher);
        candidate.risk.hash(&mut hasher);
        candidate.rule_id.hash(&mut hasher);
        candidate.category.hash(&mut hasher);
    }
    hasher.finish()
}

fn path_mode_name(mode: AiPathMode) -> &'static str {
    match mode {
        AiPathMode::Full => "full",
        AiPathMode::Basename => "basename",
        AiPathMode::Redacted => "redacted",
    }
}

fn validate_explanation(
    rationale: &[String],
    caveats: &[String],
) -> Result<(), AiGoalValidationError> {
    if rationale.is_empty() || rationale.iter().any(|item| item.trim().is_empty()) {
        return Err(AiGoalValidationError::MissingRationale);
    }
    if rationale.len() > AI_MAX_RATIONALE_ITEMS {
        return Err(AiGoalValidationError::TooManyRationaleItems);
    }
    if rationale
        .iter()
        .any(|item| item.len() > AI_MAX_EXPLANATION_ITEM_BYTES)
    {
        return Err(AiGoalValidationError::RationaleItemTooLong);
    }
    if caveats.len() > AI_MAX_CAVEAT_ITEMS {
        return Err(AiGoalValidationError::TooManyCaveatItems);
    }
    if caveats
        .iter()
        .any(|item| item.len() > AI_MAX_EXPLANATION_ITEM_BYTES)
    {
        return Err(AiGoalValidationError::CaveatItemTooLong);
    }
    Ok(())
}

fn validate_provenance(provenance: &AiProvenance) -> Result<(), AiGoalValidationError> {
    if provenance.provider.trim().is_empty() {
        return Err(AiGoalValidationError::EmptyProvider);
    }
    if provenance.model.trim().is_empty() {
        return Err(AiGoalValidationError::EmptyModel);
    }
    if provenance.provider.len() > AI_MAX_PROVENANCE_FIELD_BYTES
        || provenance.model.len() > AI_MAX_PROVENANCE_FIELD_BYTES
        || provenance
            .request_id
            .as_ref()
            .is_some_and(|value| value.len() > AI_MAX_PROVENANCE_FIELD_BYTES)
    {
        return Err(AiGoalValidationError::ProvenanceFieldTooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: i64, size_bytes: u64) -> AiGoalCandidate {
        AiGoalCandidate {
            candidate_id: id,
            path: format!("<redacted>/.cache/{id}"),
            path_mode: AiPathMode::Redacted,
            size_bytes,
            risk: "low".to_owned(),
            rule_id: format!("rule-{id}"),
            category: "cache".to_owned(),
        }
    }

    fn request() -> AiGoalRequest {
        AiGoalRequest::new(
            "req-1".to_owned(),
            42,
            vec![candidate(7, 1024), candidate(8, 2048)],
            2048,
            "low".to_owned(),
            Some("<redacted>/.cache".to_owned()),
        )
    }

    fn recommendation(ids: Vec<i64>) -> AiGoalRecommendation {
        AiGoalRecommendation {
            schema_version: AI_GOAL_SCHEMA_VERSION,
            selected_candidate_ids: ids,
            rationale: vec!["best reclaim value within supplied low-risk candidates".to_owned()],
            caveats: vec![],
            provenance: AiProvenance {
                provider: "fake".to_owned(),
                model: "goal-test".to_owned(),
                request_id: Some("req-1".to_owned()),
            },
        }
    }

    #[test]
    fn plan_digest_is_order_independent_and_changes_with_facts_or_goal() {
        let left = request();
        let mut reordered = request();
        reordered.candidates.reverse();
        reordered.plan_digest = goal_plan_digest(
            reordered.scan_id,
            &reordered.candidates,
            &reordered.constraints,
        );
        assert_eq!(left.plan_digest, reordered.plan_digest);

        let mut changed = request();
        changed.candidates[0].size_bytes += 1;
        changed.plan_digest = goal_plan_digest(changed.scan_id, &changed.candidates, &changed.constraints);
        assert_ne!(left.plan_digest, changed.plan_digest);

        let mut changed_goal = request();
        changed_goal.constraints.target_bytes += 1;
        changed_goal.plan_digest = goal_plan_digest(
            changed_goal.scan_id,
            &changed_goal.candidates,
            &changed_goal.constraints,
        );
        assert_ne!(left.plan_digest, changed_goal.plan_digest);
    }

    #[test]
    fn request_rejects_duplicate_ids_or_tampered_binding() {
        let mut duplicate = request();
        duplicate.candidates[1].candidate_id = duplicate.candidates[0].candidate_id;
        duplicate.plan_digest = goal_plan_digest(
            duplicate.scan_id,
            &duplicate.candidates,
            &duplicate.constraints,
        );
        assert!(matches!(
            duplicate.validate(),
            Err(AiGoalValidationError::DuplicateCandidateId(7))
        ));

        let mut tampered = request();
        tampered.candidates[0].size_bytes += 1;
        assert_eq!(
            tampered.validate(),
            Err(AiGoalValidationError::PlanDigestMismatch)
        );
    }

    #[test]
    fn response_rejects_unknown_or_duplicate_selected_ids() {
        let request = request();
        let unknown = AiGoalTransportResponse {
            contract_version: AI_GOAL_CONTRACT_VERSION,
            request_id: "req-1".to_owned(),
            plan_digest: request.plan_digest.clone(),
            recommendation: recommendation(vec![7, 99]),
        };
        assert_eq!(
            validate_goal_response(&request, unknown),
            Err(AiGoalValidationError::UnknownSelectedCandidateId(99))
        );

        let duplicate = AiGoalTransportResponse {
            contract_version: AI_GOAL_CONTRACT_VERSION,
            request_id: "req-1".to_owned(),
            plan_digest: request.plan_digest.clone(),
            recommendation: recommendation(vec![7, 7]),
        };
        assert_eq!(
            validate_goal_response(&request, duplicate),
            Err(AiGoalValidationError::DuplicateSelectedCandidateId(7))
        );
    }

    #[test]
    fn response_requires_exact_request_and_plan_binding() {
        let request = request();
        let wrong_request = AiGoalTransportResponse {
            contract_version: AI_GOAL_CONTRACT_VERSION,
            request_id: "other".to_owned(),
            plan_digest: request.plan_digest.clone(),
            recommendation: recommendation(vec![7]),
        };
        assert_eq!(
            validate_goal_response(&request, wrong_request),
            Err(AiGoalValidationError::RequestIdMismatch)
        );

        let wrong_digest = AiGoalTransportResponse {
            contract_version: AI_GOAL_CONTRACT_VERSION,
            request_id: "req-1".to_owned(),
            plan_digest: "goal-v1:stale".to_owned(),
            recommendation: recommendation(vec![7]),
        };
        assert_eq!(
            validate_goal_response(&request, wrong_digest),
            Err(AiGoalValidationError::PlanDigestMismatch)
        );
    }
}
