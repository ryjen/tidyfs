# Live-model advisory evaluations

Live-model evaluation is a **quality and drift** layer, not a safety-authority layer. Required tidyfs CI remains deterministic and provider-free; model output is still validated as untrusted structured input before any evaluation score is calculated.

The initial evaluation suite covers goal-oriented recommendations over already-supplied deterministic candidates. It does not scan the host filesystem, read arbitrary file contents, create cleanup candidates, invoke `clean`, execute adapters, or grant mutation authority.

## Versioned corpus

The committed corpus is:

```text
eval/fixtures/goal-recommendations-v1.json
eval/baselines/goal-recommendations-v1.json
```

Fixtures contain only bounded/redacted candidate facts that are valid under the existing `AiGoalRequest` contract. Each fixture defines qualitative expectations for:

- whether the supplied candidate set can/should satisfy the numeric target;
- useful candidate preference where a compact choice is objectively available;
- candidates that should be avoided for that fixture, when applicable;
- maximum useful selection cardinality;
- explanation terms relevant to the scenario.

The baseline file pins minimum per-fixture and suite-average semantic scores. It is a product-quality threshold, not cleanup authorization and not a replacement for deterministic contract tests.

Both files are parsed and validated in the ordinary deterministic test suite.

## Scorecard

A successful, contract-valid recommendation receives separate 0–100 dimensions:

- **selection quality** — preferred selection hit and avoidance expectations;
- **target quality** — whether `target_met` matches the fixture expectation using tidyfs-computed candidate bytes;
- **explanation relevance** — expected scenario terms found in rationale/caveats;
- **compactness** — whether the recommendation stays within the fixture's useful selection cardinality;
- **conservatism** — flags failure to meet a feasible target.

The weighted semantic score is:

```text
25% selection quality
30% target quality
20% explanation relevance
10% compactness
15% conservatism
```

Semantic scores are deliberately separate from deterministic validation. A low score or baseline regression is reported but does **not** make the evaluation command fail and never expands filesystem authority.

## Failure classes

The harness distinguishes three outcomes:

### Semantic regression

The gateway returned a structurally valid, correctly bound recommendation, but the score fell below the pinned fixture/suite threshold.

This is model-quality drift. It appears in the human and JSON reports and the command remains successful so an optional evaluation does not become an ordinary merge gate.

### Provider error

The explicitly selected loopback provider is unavailable or times out. The run writes its report and exits non-zero because the requested evaluation could not be completed.

### Contract failure

The provider response violates the real tidyfs response contract, request/provenance correlation, plan binding, selected-ID rules, or other deterministic validation.

The run writes its report and exits non-zero. If investigation shows a tidyfs safety/contract defect rather than provider non-conformance, add a minimized deterministic regression test before fixing it. Model variance is never accepted as a reason to weaken the contract.

## Run locally

Start an explicitly selected local gateway that implements the existing numeric-loopback `POST /v1/recommend` contract, then run:

```bash
TIDYFS_EVAL_ENDPOINT=http://127.0.0.1:8000 mise run eval-live
```

The default machine-readable result is:

```text
eval-results.json
```

Choose another path with:

```bash
TIDYFS_EVAL_ENDPOINT=http://127.0.0.1:8000 \
TIDYFS_EVAL_JSON_OUT=/tmp/tidyfs-eval.json \
  mise run eval-live
```

The harness reuses `LoopbackGatewayConfig`, so the same transport constraints apply as normal tidyfs AI use: numeric loopback only, no DNS, no redirects, no credentials, bounded response size, and explicit timeouts.

## Report provenance

Every JSON report records:

- evaluation report/suite version;
- tidyfs package version;
- endpoint;
- OS and architecture;
- run start time;
- per-fixture latency;
- provider/model/request provenance returned by accepted recommendations;
- selected candidate IDs and tidyfs-computed semantic score dimensions;
- baseline thresholds and whether they were met;
- provider/contract failures separately from semantic regressions.

This gives enough evidence to distinguish a model/provider change from a tidyfs code/fixture change.

## Compare provider/model drift

Keep a prior JSON report from a known model/runtime and compare a later run:

```bash
TIDYFS_EVAL_ENDPOINT=http://127.0.0.1:8000 \
TIDYFS_EVAL_COMPARE=baseline-qwen.json \
TIDYFS_EVAL_JSON_OUT=current.json \
  mise run eval-live
```

The current report then includes:

- whether provider/model provenance changed;
- average semantic-score delta;
- per-fixture score deltas.

Prior reports are intentionally not committed as universal truth because local model/provider choices vary. The committed fixture and score-threshold files are the provider-neutral baseline; concrete model reports are run evidence.

## CI policy

Do **not** add a live model to required pull-request CI.

Ordinary CI should only:

- compile the evaluation example;
- validate the committed fixture/baseline schema;
- unit-test deterministic scoring;
- continue exercising the real goal-response contract with deterministic tests/fuzzing.

A future scheduled evaluation may invoke `mise run eval-live` on a trusted self-hosted environment with a deliberately selected local provider. Such a job should remain non-blocking for semantic regressions and must preserve the same no-mutation authority boundary.

## Updating the corpus

A corpus/baseline change is a product-quality decision and should be reviewed like one:

1. explain why the scenario or expectation changed;
2. preserve versioning when semantics are incompatible;
3. do not lower a safety/contract check to accommodate a model;
4. record material score-threshold changes in the PR;
5. turn any discovered deterministic safety defect into a normal regression test.
