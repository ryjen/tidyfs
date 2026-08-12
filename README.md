# tidyfs

`tidyfs` is an AI-enabled, conservative filesystem cleanup and disk-usage intelligence CLI.

Milestone 6.1 adds parallel subtree scanning plus tool-native adapter inspection and planning. Local AI analysis is available through an explicit loopback gateway:

```bash
tidyfs scan ~
tidyfs scan ~ --jobs 8
tidyfs analyze --endpoint http://127.0.0.1:8000 --limit 10
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
- bounded, read-only `analyze` command through a numeric loopback gateway
- canonical AI observation binding and strict provider-neutral JSON transport
- explicit AI path privacy modes (`full`, `basename`, `redacted`)
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

- integrating validated AI recommendations into deterministic planning
- semantic explanation of filesystem usage and cleanup plans
- AI-generated candidate rules or policy suggestions
- hardened non-loopback gateway transport, if remote inference becomes necessary

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

AI-generated output is untrusted input. It cannot authorize permanent deletion, bypass risk gates, suppress blocked-candidate reporting, or directly invoke arbitrary shell commands. The current `analyze` command does not feed recommendations into planning or mutation; that integration has a separate policy/revalidation boundary.

## Local AI analysis

`tidyfs analyze` reads already-indexed classification facts and sends a bounded set of candidates to an explicitly configured local gateway implementing `POST /v1/analyze`.

```bash
# Full paths are acceptable for an explicitly selected loopback model.
tidyfs analyze \
  --endpoint http://127.0.0.1:8000 \
  --limit 10

# Reduce path disclosure while preserving recognized ecosystem structure.
tidyfs analyze \
  --endpoint http://127.0.0.1:8000 \
  --root ~/src \
  --path-mode redacted \
  --limit 5

# Analyze a specific completed scan with a bounded response size/timeout.
tidyfs analyze \
  --endpoint http://[::1]:8000 \
  --scan-id 42 \
  --timeout-ms 15000 \
  --max-response-bytes 65536
```

The initial runtime deliberately accepts **numeric loopback addresses only** (`127.0.0.0/8` or `::1`). It performs no DNS resolution, carries no credentials, follows no redirects, and rejects transfer-encoded responses. Remote HTTP/HTTPS gateways are not silently enabled.

Path modes:

- `full` — exact indexed path; default for the explicit loopback-only transport
- `basename` — basename only
- `redacted` — removes user/project prefixes while preserving selected structures such as `.gradle/caches`, `.cache`, `DerivedData`, `node_modules`, and `/nix/store`

The request contains metadata only: path according to the selected privacy mode, size/age, bounded deterministic labels, policy context, candidate identity, and an observation digest over the exact post-privacy facts sent to inference. File contents are not sent.

Every accepted response must match the request ID and observation digest, pass strict JSON/schema validation, use the restricted action vocabulary (`ignore`, `review`, `quarantine`), and carry matching provenance. Provider failures produce no fallback recommendation.

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
cargo run -- analyze --endpoint http://127.0.0.1:8000 --limit 10
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
- send arbitrary file contents to the AI gateway
- allow the AI gateway to mutate the filesystem

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
-> optional bounded AI analysis (advisory only today)
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
