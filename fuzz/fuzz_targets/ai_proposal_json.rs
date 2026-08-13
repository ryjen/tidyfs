#![no_main]

use libfuzzer_sys::fuzz_target;
use tidyfs::ai::AiCleanupProposal;

fuzz_target!(|data: &[u8]| {
    let Ok(proposal) = serde_json::from_slice::<AiCleanupProposal>(data) else {
        return;
    };

    if proposal.validate().is_ok() {
        let encoded = serde_json::to_vec(&proposal).expect("validated proposal must serialize");
        let decoded: AiCleanupProposal =
            serde_json::from_slice(&encoded).expect("serialized proposal must deserialize");
        assert_eq!(decoded, proposal);
    }
});
