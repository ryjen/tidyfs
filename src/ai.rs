use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

pub const AI_PROPOSAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRecommendedAction {
    Ignore,
    Review,
    Quarantine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiProvenance {
    pub provider: String,
    pub model: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiCleanupProposal {
    pub schema_version: u32,
    pub classification: String,
    pub confidence: f32,
    pub rationale: Vec<String>,
    #[serde(default)]
    pub caveats: Vec<String>,
    pub risk: AiRisk,
    pub recommended_action: AiRecommendedAction,
    pub provenance: AiProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiProposalValidationError {
    UnsupportedSchemaVersion(u32),
    EmptyClassification,
    InvalidConfidence,
    MissingRationale,
    EmptyProvider,
    EmptyModel,
}

impl fmt::Display for AiProposalValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion(version) => {
                write!(f, "unsupported AI proposal schema version: {version}")
            }
            Self::EmptyClassification => write!(f, "AI proposal classification must not be empty"),
            Self::InvalidConfidence => {
                write!(f, "AI proposal confidence must be finite and in 0.0..=1.0")
            }
            Self::MissingRationale => write!(
                f,
                "AI proposal must include at least one non-empty rationale item"
            ),
            Self::EmptyProvider => write!(f, "AI proposal provenance provider must not be empty"),
            Self::EmptyModel => write!(f, "AI proposal provenance model must not be empty"),
        }
    }
}

impl Error for AiProposalValidationError {}

impl AiCleanupProposal {
    pub fn validate(&self) -> Result<(), AiProposalValidationError> {
        if self.schema_version != AI_PROPOSAL_SCHEMA_VERSION {
            return Err(AiProposalValidationError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }

        if self.classification.trim().is_empty() {
            return Err(AiProposalValidationError::EmptyClassification);
        }

        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(AiProposalValidationError::InvalidConfidence);
        }

        if self.rationale.is_empty() || self.rationale.iter().any(|item| item.trim().is_empty()) {
            return Err(AiProposalValidationError::MissingRationale);
        }

        if self.provenance.provider.trim().is_empty() {
            return Err(AiProposalValidationError::EmptyProvider);
        }

        if self.provenance.model.trim().is_empty() {
            return Err(AiProposalValidationError::EmptyModel);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_proposal() -> AiCleanupProposal {
        AiCleanupProposal {
            schema_version: AI_PROPOSAL_SCHEMA_VERSION,
            classification: "regenerable_build_cache".to_owned(),
            confidence: 0.86,
            rationale: vec!["matches a known build-cache layout".to_owned()],
            caveats: vec!["modified recently".to_owned()],
            risk: AiRisk::Medium,
            recommended_action: AiRecommendedAction::Quarantine,
            provenance: AiProvenance {
                provider: "supervisor-gateway".to_owned(),
                model: "filesystem-specialist".to_owned(),
                request_id: Some("req-123".to_owned()),
            },
        }
    }

    #[test]
    fn accepts_valid_proposal() {
        assert_eq!(valid_proposal().validate(), Ok(()));
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let mut proposal = valid_proposal();
        proposal.schema_version = AI_PROPOSAL_SCHEMA_VERSION + 1;

        assert_eq!(
            proposal.validate(),
            Err(AiProposalValidationError::UnsupportedSchemaVersion(
                AI_PROPOSAL_SCHEMA_VERSION + 1
            ))
        );
    }

    #[test]
    fn rejects_out_of_range_or_non_finite_confidence() {
        for confidence in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            let mut proposal = valid_proposal();
            proposal.confidence = confidence;
            assert_eq!(
                proposal.validate(),
                Err(AiProposalValidationError::InvalidConfidence)
            );
        }
    }

    #[test]
    fn rejects_missing_explanation_or_provenance() {
        let mut proposal = valid_proposal();
        proposal.rationale.clear();
        assert_eq!(
            proposal.validate(),
            Err(AiProposalValidationError::MissingRationale)
        );

        let mut proposal = valid_proposal();
        proposal.provenance.provider = " ".to_owned();
        assert_eq!(
            proposal.validate(),
            Err(AiProposalValidationError::EmptyProvider)
        );
    }

    #[test]
    fn rejects_destructive_action_from_serialized_model_output() {
        let yaml = r#"
schema_version: 1
classification: regenerable_build_cache
confidence: 0.86
rationale:
  - matches a known build-cache layout
risk: medium
recommended_action: delete
provenance:
  provider: supervisor-gateway
  model: filesystem-specialist
  request_id: req-123
"#;

        let result = serde_yaml::from_str::<AiCleanupProposal>(yaml);

        assert!(result.is_err());
    }
}
