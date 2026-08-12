# tidyfs

`tidyfs` is an AI-enabled, conservative filesystem cleanup and disk-usage intelligence CLI.

Milestone 6.1 adds parallel subtree scanning plus tool-native adapter inspection and planning:

```bash
tidyfs scan ~
tidyfs scan ~ --jobs 8
tidyfs plan --safe
tidyfs adapters
tidyfs plan --risk medium --include-adapters
tidyfs clean --dry-run --risk medium
```

The project goal is to combine AI-assisted cleanup intelligence with a deterministic, policy-gated, reversible execution core. AI may propose classifications, explanations, cleanup candidates, or rules; it does not bypass safety policy or directly perform destructive filesystem mutation.

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

Planned / evolving:

- AI-assisted classification and cleanup recommendations
- semantic explanation of filesystem usage and cleanup plans
- AI-generated candidate rules or policy suggestions
- bounded AI integration that feeds the deterministic planner rather than bypassing it

Adapters currently inspect/report only. They do not execute cleanup commands.

## AI trust boundary

AI is an advisory and planning layer, not the filesystem mutation authority.

```text
filesystem facts / adapter facts
-> deterministic classifications
-> AI-assisted analysis and suggestions
-> candidate rules / cleanup recommendations
-> deterministic planner
-> policy and risk validation
-> dry-run preview
-> explicit interactive approval
-> reversible quarantine execution
-> durable action state
-> recover or restore
```

The intended invariant is:

> AI may broaden understanding and improve recommendations; deterministic policy controls what can actually happen.

AI-generated output must therefore be treated as untrusted input to the same validation path as any other cleanup rule or candidate. It cannot authorize permanent deletion, bypass risk gates, suppress blocked-candidate reporting, or directly invoke arbitrary shell commands.

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
- allow AI output to bypass policy validation or approval
- inspect file contents as part of the current deterministic scan path

The current milestone does not yet invoke an AI provider. AI integration is intended to sit above the deterministic planner and safety gates described above.

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
-> optional AI-assisted analysis
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
