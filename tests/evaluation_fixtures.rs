use std::collections::BTreeSet;
use std::fs;
use tidyfs::evaluation::{EvaluationBaseline, GoalEvaluationSuite};

#[test]
fn committed_live_evaluation_corpus_is_versioned_and_valid() {
    let suite: GoalEvaluationSuite = serde_json::from_slice(
        &fs::read("eval/fixtures/goal-recommendations-v1.json")
            .expect("read evaluation fixtures"),
    )
    .expect("parse evaluation fixtures");
    suite.validate().expect("valid evaluation fixtures");
    assert!(!suite.fixtures.is_empty());

    let baseline: EvaluationBaseline = serde_json::from_slice(
        &fs::read("eval/baselines/goal-recommendations-v1.json")
            .expect("read evaluation baseline"),
    )
    .expect("parse evaluation baseline");
    baseline
        .validate_for(&suite)
        .expect("baseline matches fixture suite");

    let fixture_ids: BTreeSet<_> = suite
        .fixtures
        .iter()
        .map(|fixture| fixture.id.as_str())
        .collect();
    let baseline_ids: BTreeSet<_> = baseline
        .fixture_minimum_scores
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        baseline_ids, fixture_ids,
        "every fixture needs a pinned score threshold"
    );
}
