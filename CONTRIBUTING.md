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

The scheduled/manual `AI proposal fuzz smoke` job is exploratory and is not intended as a pull-request branch-protection check.
