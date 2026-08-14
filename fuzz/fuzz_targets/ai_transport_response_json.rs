#![no_main]

use libfuzzer_sys::fuzz_target;
use tidyfs::ai_contract::{
    validate_transport_response, AiDeterministicFacts, AiObservation, AiPathMode,
    AiTransportRequest, AiTransportResponse,
};

fn request() -> AiTransportRequest {
    AiTransportRequest::new(
        "fuzz-request".to_owned(),
        AiObservation {
            scan_id: 42,
            candidate_key: "scan:42:path:cache".to_owned(),
            path: "<redacted>/.cache/pip".to_owned(),
            path_mode: AiPathMode::Redacted,
            size_bytes: 4096,
            age_seconds: Some(86_400),
            labels: vec!["cache".to_owned()],
            deterministic: AiDeterministicFacts {
                classification: Some("cache".to_owned()),
                matched_rule: Some("cache-pip".to_owned()),
                protected: false,
                max_allowed_risk: "low".to_owned(),
            },
            adapter: None,
        },
    )
}

fuzz_target!(|data: &[u8]| {
    let Ok(response) = serde_json::from_slice::<AiTransportResponse>(data) else {
        return;
    };

    let request = request();
    if let Ok(proposal) = validate_transport_response(&request, response) {
        proposal
            .validate()
            .expect("accepted transport proposal must remain valid");
        assert!(
            request
                .constraints
                .allowed_actions
                .contains(&proposal.recommended_action),
            "accepted transport proposal must use a request-allowed action"
        );

        let encoded = serde_json::to_vec(&proposal).expect("accepted proposal must serialize");
        let decoded = serde_json::from_slice(&encoded).expect("serialized proposal must parse");
        assert_eq!(decoded, proposal);
    }
});
