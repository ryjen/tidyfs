use crate::ai::{AiCleanupProposal, AiProposalValidationError};
use crate::ai_contract::AiObservation;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAnalysisRequest {
    pub observation: AiObservation,
    pub observation_digest: String,
}

impl AiAnalysisRequest {
    pub fn new(observation: AiObservation) -> Self {
        let observation_digest = observation.digest();
        Self {
            observation,
            observation_digest,
        }
    }

    pub fn observation_is_bound(&self) -> bool {
        self.observation_digest == self.observation.digest()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiProviderError {
    Unavailable(String),
    InvalidResponse(String),
}

impl fmt::Display for AiProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "AI provider unavailable: {message}"),
            Self::InvalidResponse(message) => {
                write!(f, "AI provider returned invalid response: {message}")
            }
        }
    }
}

impl Error for AiProviderError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiAnalysisError {
    Provider(AiProviderError),
    InvalidProposal(AiProposalValidationError),
    ObservationDigestMismatch,
}

impl fmt::Display for AiAnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provider(error) => error.fmt(f),
            Self::InvalidProposal(error) => write!(f, "AI proposal rejected: {error}"),
            Self::ObservationDigestMismatch => {
                write!(f, "AI analysis request observation digest is stale or invalid")
            }
        }
    }
}

impl Error for AiAnalysisError {}

pub trait AiAnalysisProvider {
    fn analyze(&self, request: &AiAnalysisRequest) -> Result<AiCleanupProposal, AiProviderError>;
}

pub fn analyze_validated<P: AiAnalysisProvider>(
    provider: &P,
    request: &AiAnalysisRequest,
) -> Result<AiCleanupProposal, AiAnalysisError> {
    if !request.observation_is_bound() {
        return Err(AiAnalysisError::ObservationDigestMismatch);
    }

    let proposal = provider
        .analyze(request)
        .map_err(AiAnalysisError::Provider)?;
    proposal
        .validate()
        .map_err(AiAnalysisError::InvalidProposal)?;
    Ok(proposal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiProvenance, AiRecommendedAction, AiRisk, AI_PROPOSAL_SCHEMA_VERSION};
    use crate::ai_contract::{AiDeterministicFacts, AiPathMode};

    #[derive(Clone)]
    struct FakeProvider {
        result: Result<AiCleanupProposal, AiProviderError>,
    }

    impl AiAnalysisProvider for FakeProvider {
        fn analyze(
            &self,
            _request: &AiAnalysisRequest,
        ) -> Result<AiCleanupProposal, AiProviderError> {
            self.result.clone()
        }
    }

    fn request() -> AiAnalysisRequest {
        AiAnalysisRequest::new(AiObservation {
            scan_id: 42,
            candidate_key: "scan-42:candidate-7".to_owned(),
            path: "/home/user/.cache/example".to_owned(),
            path_mode: AiPathMode::Full,
            size_bytes: 46 * 1024 * 1024 * 1024,
            age_seconds: Some(3600),
            labels: vec!["cache".to_owned()],
            deterministic: AiDeterministicFacts {
                classification: Some("cache".to_owned()),
                matched_rule: None,
                protected: false,
                max_allowed_risk: "medium".to_owned(),
            },
            adapter: None,
        })
    }

    fn valid_proposal() -> AiCleanupProposal {
        AiCleanupProposal {
            schema_version: AI_PROPOSAL_SCHEMA_VERSION,
            classification: "regenerable_build_cache".to_owned(),
            confidence: 0.86,
            rationale: vec!["matches a known build-cache layout".to_owned()],
            caveats: vec!["modified recently".to_owned()],
            risk: AiRisk::Medium,
            recommended_action: AiRecommendedAction::Review,
            provenance: AiProvenance {
                provider: "fake".to_owned(),
                model: "test-model".to_owned(),
                request_id: Some("test-request".to_owned()),
            },
        }
    }

    #[test]
    fn accepts_valid_provider_proposal() {
        let expected = valid_proposal();
        let provider = FakeProvider {
            result: Ok(expected.clone()),
        };

        assert_eq!(analyze_validated(&provider, &request()), Ok(expected));
    }

    #[test]
    fn rejects_request_if_observation_changes_after_binding() {
        let provider = FakeProvider {
            result: Ok(valid_proposal()),
        };
        let mut request = request();
        request.observation.size_bytes += 1;

        assert_eq!(
            analyze_validated(&provider, &request),
            Err(AiAnalysisError::ObservationDigestMismatch)
        );
    }

    #[test]
    fn rejects_invalid_provider_proposal_at_boundary() {
        let mut proposal = valid_proposal();
        proposal.confidence = 2.0;
        let provider = FakeProvider {
            result: Ok(proposal),
        };

        assert_eq!(
            analyze_validated(&provider, &request()),
            Err(AiAnalysisError::InvalidProposal(
                AiProposalValidationError::InvalidConfidence
            ))
        );
    }

    #[test]
    fn provider_failure_does_not_produce_a_proposal() {
        let provider = FakeProvider {
            result: Err(AiProviderError::Unavailable("offline".to_owned())),
        };

        assert_eq!(
            analyze_validated(&provider, &request()),
            Err(AiAnalysisError::Provider(AiProviderError::Unavailable(
                "offline".to_owned()
            )))
        );
    }
}
