# Contributing

## Local quality checks

Run the same deterministic checks used by CI before opening a pull request:

```bash
mise run ci
```

The underlying static-analysis entrypoint can also be run without mise:

```bash
bash scripts/static-analysis.sh
```

That gate runs formatting and Clippy with warnings denied. The deterministic test suite and package verification are available directly as:

```bash
cargo test --all-targets --all-features --locked
cargo package --locked
```

Install and run the dependency audit locally when changing dependencies:

```bash
cargo install cargo-audit --locked
mise run audit
```

For fuzzing, install the same pinned nightly toolchain and `cargo-fuzz` version used by CI:

```bash
rustup toolchain install nightly-2026-08-12
cargo install cargo-fuzz --version 0.13.2 --locked
mise run fuzz-ai-build
mise run fuzz-ai
```

See `docs/testing.md` for the test pyramid, fuzzing policy, and AI-specific testing guidance.

## Safety boundary

Changes that can mutate the filesystem require:

- explicit dry-run behavior;
- reversible execution unless a separately reviewed design says otherwise;
- isolated filesystem and database tests;
- documented failure and recovery behavior;
- no arbitrary shell command construction.

Do not test cleanup behavior against a real home directory or production filesystem.

Fuzz targets must operate on pure parsing/validation boundaries or isolated synthetic state. Reproduce any fuzz-discovered failure as a deterministic regression test before merging a fix.

AI-facing tests must treat model output as untrusted input. Required CI must not depend on a live model provider or grant model-controlled code mutation authority.

## Pull requests

Keep pull requests focused and describe:

- the user-visible behavior;
- safety and recovery implications;
- tests added or updated;
- known limitations and deferred work.

The stable CI checks intended for branch protection are:

- `Rust quality`
- `Dependency audit`
- `AI fuzz harness`

On pull requests, `AI fuzz harness` only performs a deterministic build of the fuzz target. Coverage-guided fuzz execution remains scheduled/manual and is not part of the pull-request branch-protection gate.
