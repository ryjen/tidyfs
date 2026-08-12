use crate::ai::{AiCleanupProposal, AiProposalValidationError, AiRecommendedAction};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{self, Write as _};

pub const AI_TRANSPORT_CONTRACT_VERSION: u32 = 1;
pub const AI_ANALYSIS_TASK: &str = "cleanup_candidate_analysis";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiPathMode {
    Full,
    Basename,
    Redacted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiDeterministicFacts {
    pub classification: Option<String>,
    pub matched_rule: Option<String>,
    pub protected: bool,
    pub max_allowed_risk: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiObservation {
    pub scan_id: i64,
    pub candidate_key: String,
    pub path: String,
    pub path_mode: AiPathMode,
    pub size_bytes: u64,
    pub age_seconds: Option<u64>,
    pub labels: Vec<String>,
    pub deterministic: AiDeterministicFacts,
    pub adapter: Option<String>,
}

impl AiObservation {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut output = Vec::new();
        frame(&mut output, b"tidyfs-ai-observation-v1");
        frame(&mut output, &self.scan_id.to_be_bytes());
        frame(&mut output, self.candidate_key.as_bytes());
        frame(&mut output, self.path.as_bytes());
        frame(
            &mut output,
            match self.path_mode {
                AiPathMode::Full => b"full",
                AiPathMode::Basename => b"basename",
                AiPathMode::Redacted => b"redacted",
            },
        );
        frame(&mut output, &self.size_bytes.to_be_bytes());
        frame_optional_u64(&mut output, self.age_seconds);

        let mut labels = self.labels.clone();
        labels.sort();
        labels.dedup();
        frame(&mut output, &(labels.len() as u64).to_be_bytes());
        for label in labels {
            frame(&mut output, label.as_bytes());
        }

        frame_optional_string(&mut output, self.deterministic.classification.as_deref());
        frame_optional_string(&mut output, self.deterministic.matched_rule.as_deref());
        frame(&mut output, &[u8::from(self.deterministic.protected)]);
        frame(
            &mut output,
            self.deterministic.max_allowed_risk.as_bytes(),
        );
        frame_optional_string(&mut output, self.adapter.as_deref());
        output
    }

    pub fn digest(&self) -> String {
        format!("sha256:{}", sha256_hex(&self.canonical_bytes()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiObservationBinding {
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTransportCandidate {
    #[serde(flatten)]
    pub facts: AiObservation,
    pub observation: AiObservationBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTransportConstraints {
    pub allowed_actions: Vec<AiRecommendedAction>,
    pub file_contents_available: bool,
    pub mutation_authority: bool,
}

impl Default for AiTransportConstraints {
    fn default() -> Self {
        Self {
            allowed_actions: vec![
                AiRecommendedAction::Ignore,
                AiRecommendedAction::Review,
                AiRecommendedAction::Quarantine,
            ],
            file_contents_available: false,
            mutation_authority: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTransportRequest {
    pub contract_version: u32,
    pub request_id: String,
    pub task: String,
    pub candidate: AiTransportCandidate,
    pub constraints: AiTransportConstraints,
}

impl AiTransportRequest {
    pub fn new(request_id: String, facts: AiObservation) -> Self {
        let digest = facts.digest();
        Self {
            contract_version: AI_TRANSPORT_CONTRACT_VERSION,
            request_id,
            task: AI_ANALYSIS_TASK.to_owned(),
            candidate: AiTransportCandidate {
                facts,
                observation: AiObservationBinding { digest },
            },
            constraints: AiTransportConstraints::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTransportResponse {
    pub contract_version: u32,
    pub request_id: String,
    pub proposal: AiCleanupProposal,
    pub observation: AiObservationBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiTransportValidationError {
    UnsupportedContractVersion(u32),
    RequestIdMismatch,
    ObservationDigestMismatch,
    InvalidProposal(AiProposalValidationError),
}

impl fmt::Display for AiTransportValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedContractVersion(version) => {
                write!(f, "unsupported AI transport contract version: {version}")
            }
            Self::RequestIdMismatch => write!(f, "AI response request id does not match request"),
            Self::ObservationDigestMismatch => {
                write!(f, "AI response observation digest does not match request")
            }
            Self::InvalidProposal(error) => write!(f, "AI response proposal rejected: {error}"),
        }
    }
}

impl Error for AiTransportValidationError {}

pub fn validate_transport_response(
    request: &AiTransportRequest,
    response: AiTransportResponse,
) -> Result<AiCleanupProposal, AiTransportValidationError> {
    if response.contract_version != AI_TRANSPORT_CONTRACT_VERSION {
        return Err(AiTransportValidationError::UnsupportedContractVersion(
            response.contract_version,
        ));
    }
    if response.request_id != request.request_id {
        return Err(AiTransportValidationError::RequestIdMismatch);
    }
    if response.observation.digest != request.candidate.observation.digest {
        return Err(AiTransportValidationError::ObservationDigestMismatch);
    }
    response
        .proposal
        .validate()
        .map_err(AiTransportValidationError::InvalidProposal)?;
    Ok(response.proposal)
}

fn frame(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    output.extend_from_slice(bytes);
}

fn frame_optional_u64(output: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            frame(output, b"some");
            frame(output, &value.to_be_bytes());
        }
        None => frame(output, b"none"),
    }
}

fn frame_optional_string(output: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            frame(output, b"some");
            frame(output, value.as_bytes());
        }
        None => frame(output, b"none"),
    }
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = sha256(input);
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state = [
        0x6a09e667_u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
        0x1f83d9ab, 0x5be0cd19,
    ];
    let mut padded = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for block in padded.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for (i, chunk) in block.chunks_exact(4).take(16).enumerate() {
            w[i] = u32::from_be_bytes(chunk.try_into().expect("four-byte chunk"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut output = [0_u8; 32];
    for (i, word) in state.iter().enumerate() {
        output[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiProvenance, AiRecommendedAction, AiRisk, AI_PROPOSAL_SCHEMA_VERSION};

    fn observation() -> AiObservation {
        AiObservation {
            scan_id: 42,
            candidate_key: "scan-42:candidate-7".to_owned(),
            path: "/home/user/.cache/example".to_owned(),
            path_mode: AiPathMode::Full,
            size_bytes: 46 * 1024 * 1024 * 1024,
            age_seconds: Some(3600),
            labels: vec!["generated_artifact".to_owned(), "cache".to_owned()],
            deterministic: AiDeterministicFacts {
                classification: Some("cache".to_owned()),
                matched_rule: None,
                protected: false,
                max_allowed_risk: "medium".to_owned(),
            },
            adapter: None,
        }
    }

    fn proposal() -> AiCleanupProposal {
        AiCleanupProposal {
            schema_version: AI_PROPOSAL_SCHEMA_VERSION,
            classification: "regenerable_build_cache".to_owned(),
            confidence: 0.9,
            rationale: vec!["matches generated cache layout".to_owned()],
            caveats: vec![],
            risk: AiRisk::Medium,
            recommended_action: AiRecommendedAction::Review,
            provenance: AiProvenance {
                provider: "test".to_owned(),
                model: "test-model".to_owned(),
                request_id: Some("req-1".to_owned()),
            },
        }
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn observation_digest_is_stable_for_label_order_and_duplicates() {
        let left = observation();
        let mut right = observation();
        right.labels = vec![
            "cache".to_owned(),
            "generated_artifact".to_owned(),
            "cache".to_owned(),
        ];
        assert_eq!(left.digest(), right.digest());
    }

    #[test]
    fn observation_digest_changes_when_supplied_facts_change() {
        let left = observation();
        let mut right = observation();
        right.size_bytes += 1;
        assert_ne!(left.digest(), right.digest());
    }

    #[test]
    fn transport_request_binds_the_observation_digest() {
        let request = AiTransportRequest::new("req-1".to_owned(), observation());
        assert_eq!(request.contract_version, AI_TRANSPORT_CONTRACT_VERSION);
        assert_eq!(request.task, AI_ANALYSIS_TASK);
        assert_eq!(request.candidate.observation.digest, request.candidate.facts.digest());
        assert!(!request.constraints.file_contents_available);
        assert!(!request.constraints.mutation_authority);
    }

    #[test]
    fn response_must_match_request_and_observation() {
        let request = AiTransportRequest::new("req-1".to_owned(), observation());
        let valid = AiTransportResponse {
            contract_version: AI_TRANSPORT_CONTRACT_VERSION,
            request_id: request.request_id.clone(),
            proposal: proposal(),
            observation: request.candidate.observation.clone(),
        };
        assert_eq!(validate_transport_response(&request, valid), Ok(proposal()));

        let mismatched = AiTransportResponse {
            contract_version: AI_TRANSPORT_CONTRACT_VERSION,
            request_id: "other".to_owned(),
            proposal: proposal(),
            observation: request.candidate.observation.clone(),
        };
        assert_eq!(
            validate_transport_response(&request, mismatched),
            Err(AiTransportValidationError::RequestIdMismatch)
        );
    }
}
