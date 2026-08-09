# tidyfs

`tidyfs` is a conservative disk-usage intelligence CLI.

Milestone 6.1 adds parallel subtree scanning plus tool-native adapter inspection and planning:

```bash
tidyfs scan ~
tidyfs scan ~ --jobs 8
tidyfs plan --safe
tidyfs adapters
tidyfs plan --risk medium --include-adapters
tidyfs clean --dry-run --risk medium
```

The project goal is to build a safe cleanup planner, not an autonomous file deleter.

## Current scope

Implemented:

- Rust CLI
- recursive filesystem scan
- SQLite index
- directory aggregation
- deterministic classification
- `explain` command
- YAML cleanup rules
- policy validation
- cleanup candidate persistence
- `plan` command
- blocked-candidate reporting
- `clean --dry-run`
- reversible quarantine execution
- durable cleanup and restore action states
- interrupted action reconciliation with `recover`
- action logging
- `actions` listing
- `restore`
- read-only tool-native adapters
- no permanent deletion
- no AI

Adapters currently inspect/report only. They do not execute cleanup commands.

## Adapter commands

```bash
cargo run -- adapters
cargo run -- plan --risk medium --include-adapters
```

Supported adapters:

- systemd journal
- Docker
- Podman
- Nix
- pnpm
- pip
- uv
- Go

## Install / run

```bash
cargo run -- scan ~
cargo run -- scan ~ --jobs 8
cargo run -- top --depth 2 --limit 20
cargo run -- classify --summary
cargo run -- explain ~/.cache --children
cargo run -- plan --safe
cargo run -- clean --dry-run
cargo run -- clean --safe --interactive
cargo run -- actions
cargo run -- recover --all
cargo run -- restore --latest
cargo run -- adapters
cargo run -- plan --risk medium --include-adapters
```

By default, the SQLite DB is stored at:

```text
~/.local/share/tidyfs/tidyfs.db
```

Quarantine data is stored at:

```text
~/.local/share/tidyfs/quarantine/
```

## Safety posture

Milestone 6 supports real filesystem mutation only through quarantine.

It does not:

- permanently delete files
- purge quarantine
- execute adapter cleanup commands
- run arbitrary shell commands
- call AI providers
- inspect file contents

Real quarantine execution requires both:

```bash
--safe --interactive
```

Cleanup and restore persist transitional action states before filesystem mutation. After an interrupted operation, reconcile the database against the two recorded paths with:

```bash
tidyfs recover --all
```

Recovery does not move, delete, or overwrite filesystem entries. See [Recoverable actions](docs/recovery.md) for the state machine, reconciliation matrix, threat model, current limitations, and operator procedure.

Adapter candidates use:

```text
action_type = tool_native
```

They are visible in plans and dry-runs, but are not executable yet.

## Planning model

```text
scan facts
-> deterministic classifications
-> YAML cleanup rules
-> adapter inspection
-> policy validation
-> cleanup candidates / blocked candidates
-> dry-run preview
-> interactive quarantine execution for reversible file candidates only
-> durable action state
-> recover or restore
```

## Parallel scanning

The scanner uses parallel workers over immediate child subtrees and a single SQLite writer.

```bash
tidyfs scan ~ --jobs 8
```

Why this shape:

- metadata reads are parallelized
- SQLite writes remain serialized and simple
- aggregation remains deterministic
- no shared mutable filesystem state
- no deletion behavior changes

This is not the final high-performance design, but it removes the obvious single-threaded traversal bottleneck while preserving the safety model.

## Development and quality gates

Run the same core checks locally that CI uses before opening or merging a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
cargo package --locked
```

CI also runs a RustSec dependency audit. The stable required check names are:

- `Rust quality`
- `Dependency audit`

The dependency-audit job is intentionally read-only with respect to repository contents and receives only the additional GitHub Checks permission required to publish its audit result.
