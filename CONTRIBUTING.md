# Contributing

## Local quality checks

Run the same checks used by CI before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
```

Install and run the dependency audit locally when changing dependencies:

```bash
cargo install cargo-audit --locked
cargo audit
```

## Safety boundary

Changes that can mutate the filesystem require:

- explicit dry-run behavior;
- reversible execution unless a separately reviewed design says otherwise;
- isolated filesystem and database tests;
- documented failure and recovery behavior;
- no arbitrary shell command construction.

Do not test cleanup behavior against a real home directory or production filesystem.

## Pull requests

Keep pull requests focused and describe:

- the user-visible behavior;
- safety and recovery implications;
- tests added or updated;
- known limitations and deferred work.

The stable CI checks intended for branch protection are:

- `Rust quality`
- `Dependency audit`
