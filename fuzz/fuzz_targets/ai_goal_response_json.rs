#![no_main]

use libfuzzer_sys::fuzz_target;
use tidyfs::ai_contract::AiPathMode;
use tidyfs::ai_goal::{
    validate_goal_response, AiGoalCandidate, AiGoalRecommendation, AiGoalRequest,
    AiGoalTransportResponse, AI_GOAL_CONTRACT_VERSION,
};

fn request() -> AiGoalRequest {
    AiGoalRequest::new(
        "fuzz-goal-request".to_owned(),
        42,
        vec![
            AiGoalCandidate {
                candidate_id: 7,
                path: "<redacted>/.cache/pip".to_owned(),
                path_mode: AiPathMode::Redacted,
                size_bytes: 4096,
                risk: "low".to_owned(),
                rule_id: "cache-pip".to_owned(),
                category: "cache".to_owned(),
            },
            AiGoalCandidate {
                candidate_id: 8,
                path: "<redacted>/.cache/uv".to_owned(),
                path_mode: AiPathMode::Redacted,
                size_bytes: 8192,
                risk: "low".to_owned(),
                rule_id: "cache-uv".to_owned(),
                category: "cache".to_owned(),
            },
        ],
        4096,
        "low".to_owned(),
        Some("<redacted>/.cache".to_owned()),
    )
}

fn assert_accepted(request: &AiGoalRequest, response: AiGoalTransportResponse) {
    if let Ok(recommendation) = validate_goal_response(request, response) {
        recommendation
            .validate()
            .expect("accepted goal recommendation must remain valid");
        assert!(
            recommendation
                .selected_candidate_ids
                .iter()
                .all(|id| matches!(id, 7 | 8)),
            "accepted goal recommendation must select only supplied candidate IDs"
        );

        let encoded = serde_json::to_vec(&recommendation)
            .expect("accepted goal recommendation must serialize");
        let decoded: AiGoalRecommendation = serde_json::from_slice(&encoded)
            .expect("serialized goal recommendation must parse");
        assert_eq!(decoded, recommendation);
    }
}

fuzz_target!(|data: &[u8]| {
    let request = request();

    if let Ok(response) = serde_json::from_slice::<AiGoalTransportResponse>(data) {
        assert_accepted(&request, response);
    }

    if let Ok(recommendation) = serde_json::from_slice::<AiGoalRecommendation>(data) {
        let response = AiGoalTransportResponse {
            contract_version: AI_GOAL_CONTRACT_VERSION,
            request_id: request.request_id.clone(),
            plan_digest: request.plan_digest.clone(),
            recommendation,
        };
        assert_accepted(&request, response);
    }
});
