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

The initial fuzz target is `fuzz/fuzz_targets/ai_proposal_json.rs`. It feeds arbitrary model-style JSON into `AiCleanupProposal`, validates accepted proposals, and asserts that accepted values serialize and deserialize without semantic change.

The fuzz harness uses a pinned nightly compiler and a pinned `cargo-fuzz` release. Install the same versions locally with:

```bash
rustup toolchain install nightly-2026-08-12
cargo install cargo-fuzz --version 0.13.2 --locked
mise run fuzz-ai-build
mise run fuzz-ai
```

Pull-request CI deterministically builds the fuzz harness so a broken target cannot merge unnoticed. Coverage-guided fuzz execution itself remains scheduled/manual rather than a pull-request blocker. When scheduled/manual fuzzing fails, CI preserves `fuzz/artifacts/` as a short-lived workflow artifact so the failing input can be reproduced. Any discovered crash or invariant violation should be minimized into a deterministic regression test before the fix is merged.

Good future fuzz/property targets include:

- rules/configuration deserialization;
- path normalization and candidate identity inputs;
- cleanup-plan deserialization and validation;
- recovery metadata/state-machine inputs.

Avoid fuzzing destructive operations directly against a real filesystem. Use pure validation boundaries or an isolated synthetic filesystem harness.

## AI testing

AI-facing code is tested as an untrusted structured-input boundary. Required CI should remain deterministic and must not depend on a live model provider.

Required AI tests should cover:

- schema/version rejection;
- unknown or malformed fields;
- confidence, size, and cardinality bounds;
- request and observation binding;
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
mise run fuzz-ai-build
```

The first command runs static analysis, the full deterministic test suite, and Cargo package verification. The second reproduces the deterministic fuzz-harness build used by pull-request CI. Dependency auditing remains a separate task because `cargo-audit` is an optional Cargo subcommand rather than part of the standard Rust toolchain.
