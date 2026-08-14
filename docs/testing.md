# Testing strategy

Tidyfs mutates filesystem state conservatively, so the test strategy emphasizes deterministic safety invariants at the bottom of the pyramid and hermetic filesystem behavior at the top.

## Test pyramid

### Unit tests

Unit tests live beside implementation code under `src/` and cover pure or narrowly scoped behavior such as:

- AI proposal validation and transport binding;
- path and filesystem-boundary helpers;
- plan and identity invariants;
- lock and utility behavior.

These should be the fastest and most numerous tests. New deterministic business rules should normally start here.

### Integration tests

Integration tests under `tests/` exercise multiple tidyfs components together using isolated temporary directories and SQLite databases. They cover mutation, recovery, risk thresholds, permission failures, concurrency, and quarantine/restore behavior.

Integration tests must never use a developer's real home directory or production filesystem state.

### End-to-end tests

`tests/hermetic_cli.rs` launches the compiled `tidyfs` binary through `CARGO_BIN_EXE_tidyfs` and verifies behavior through the CLI, filesystem, and database boundaries. Keep this layer focused on critical user journeys rather than duplicating every lower-level case.

## Property and fuzz testing

Coverage-guided fuzzing is appropriate for tidyfs where attacker-controlled, model-controlled, or unusually shaped input crosses a parser or validation boundary. It is less useful for tests that primarily depend on filesystem timing or large directory trees, where deterministic integration tests provide better diagnostics.

### Maintained fuzz targets

The maintained cargo-fuzz targets deliberately stay on pure, side-effect-free trust boundaries:

| Target | Boundary | Accepted-value invariants |
| --- | --- | --- |
| `ai_proposal_json` | model-controlled `AiCleanupProposal` JSON | accepted proposal validates and round-trips without semantic change |
| `ai_transport_response_json` | candidate-analysis response envelope plus proposal validation | request/observation binding holds, proposal validates, and the accepted action is explicitly request-allowed |
| `ai_goal_response_json` | goal-recommendation response envelope plus recommendation validation | request/plan binding holds, recommendation validates, and every accepted selected ID was supplied by tidyfs |

The transport/goal targets exercise two paths for each fuzz input:

1. parse it as the full untrusted response envelope and run the real binding validator;
2. when the inner proposal/recommendation parses, place it inside a known-valid request binding and run the validator again.

The second path prevents fuzz coverage from stopping at shallow request-ID/digest mismatches and drives mutations into bounded text, provenance, action/ID, cardinality, and semantic validation.

Each target has a valid seed corpus under `fuzz/corpus/<target>/` so scheduled fuzzing reaches deep validation paths early.

### Toolchain and local workflow

The fuzz harness uses a pinned nightly compiler and a pinned `cargo-fuzz` release. Install the same versions locally with:

```bash
rustup toolchain install nightly-2026-08-12
cargo install cargo-fuzz --version 0.13.2 --locked
mise run fuzz-build
```

Run short local campaigns across every maintained target with:

```bash
mise run fuzz
```

The all-target local campaign gives each target 30 seconds with a 5-second individual-input timeout and 2 GiB RSS limit. The original proposal-only compatibility tasks remain available as `mise run fuzz-ai-build` and `mise run fuzz-ai`.

### CI cadence and resource bounds

Pull-request CI is deterministic: it **builds every maintained fuzz target** so broken harnesses cannot merge unnoticed, but it does not execute coverage-guided campaigns as a PR blocker.

The dedicated `Fuzz` workflow executes coverage-guided campaigns:

- weekly at the existing Sunday schedule;
- on explicit `workflow_dispatch`;
- one matrix job per maintained target;
- 60 seconds maximum fuzz time per target;
- 5 seconds maximum for one input;
- 2 GiB libFuzzer RSS limit;
- 10-minute outer GitHub job timeout;
- fail-fast disabled so one crashing boundary does not hide results from the others.

The workflow is self-cancelling for superseded runs on the same ref. Fuzzing remains bounded and side-effect-free; it does not invoke filesystem cleanup, recovery, adapter commands, or a live model provider.

### Crash artifact retention and reproduction

A failed matrix job uploads only that target's `fuzz/artifacts/<target>/` directory for 14 days using an artifact name that includes the target and workflow run ID.

To reproduce a retained crash locally:

```bash
# after downloading/unpacking the workflow artifact
rustup toolchain install nightly-2026-08-12
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly-2026-08-12 fuzz run <target> /path/to/crash-or-timeout-artifact
```

For example:

```bash
cargo +nightly-2026-08-12 fuzz run ai_goal_response_json ./crash-0123456789abcdef
```

Minimize a reproducer when useful with cargo-fuzz/libFuzzer tooling, then convert every confirmed crash or invariant violation into a deterministic unit/integration regression before merging the fix. The fuzz artifact is discovery evidence, not the permanent regression test.

Good future fuzz/property targets include:

- rules/configuration deserialization;
- path normalization and candidate identity inputs;
- recovery metadata/state-machine inputs.

Avoid fuzzing destructive operations directly against a real filesystem. Use pure validation boundaries or an isolated synthetic filesystem harness.

## AI testing

AI-facing code is tested as an untrusted structured-input boundary. Required CI should remain deterministic and must not depend on a live model provider.

Required AI tests should cover:

- schema/version rejection;
- unknown or malformed fields;
- confidence, size, and cardinality bounds;
- request and observation/plan binding;
- action/risk safety constraints;
- provenance handling;
- serialization round trips for accepted values.

Live-model evaluations can be useful for recommendation quality, calibration, and regression discovery, but they should run as non-blocking evaluation jobs with pinned prompts/fixtures and recorded provider/model metadata. They must not replace deterministic contract tests and should never be granted mutation authority.

## Static analysis and dependency analysis

`scripts/static-analysis.sh` is the shared local/CI entrypoint for formatting and Clippy. CI invokes the same script to prevent local and hosted quality gates from drifting.

```bash
mise run static-analysis
```

RustSec dependency auditing is a separate software-composition-analysis gate:

```bash
mise run audit
```

## Local quality gate

Run the deterministic pull-request gate locally with:

```bash
mise run ci
mise run fuzz-build
```

The first command runs static analysis, the full deterministic test suite, and Cargo package verification. The second reproduces the deterministic all-target fuzz-harness build used by pull-request CI. Dependency auditing remains a separate task because `cargo-audit` is an optional Cargo subcommand rather than part of the standard Rust toolchain.
