# tidyfs

`tidyfs` is an AI-enabled, conservative filesystem cleanup and disk-usage intelligence CLI.

Milestone 6.1 adds parallel subtree scanning plus tool-native adapter inspection and planning. Local AI analysis and conservative plan enrichment are available through an explicit loopback gateway:

```bash
tidyfs scan ~
tidyfs scan ~ --jobs 8
tidyfs analyze --endpoint http://127.0.0.1:8000 --limit 10
tidyfs plan --safe
tidyfs plan --safe --ai-endpoint http://127.0.0.1:8000 --ai-path-mode redacted
tidyfs adapters
tidyfs plan --risk medium --include-adapters
tidyfs clean --dry-run --risk medium
```

The project goal is to combine AI-assisted cleanup intelligence with a deterministic, policy-gated, reversible execution core. AI may enrich classifications, explanations, and cleanup recommendations; it does not bypass safety policy or directly perform destructive filesystem mutation.

## Current scope

Implemented:

- Rust CLI
- recursive filesystem scan
- SQLite index
- directory aggregation
- deterministic classification
- `explain` command with revalidated stored AI evidence
- bounded, read-only `analyze` command through a numeric loopback gateway
- canonical AI observation binding and strict provider-neutral JSON transport
- explicit AI path privacy modes (`full`, `basename`, `redacted`)
- conservative AI enrichment of already-eligible deterministic plan candidates
- authoritative observation reconstruction and stale-recommendation rejection
- persisted advisory AI rationale/confidence/provenance/observation evidence
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

- semantic explanation improvements for filesystem usage and cleanup plans
- AI-generated candidate rules or policy suggestions
- hardened non-loopback gateway transport, if remote inference becomes necessary

Adapters currently inspect/report only. They do not execute cleanup commands.

## AI trust boundary

AI is an advisory and planning layer, not the filesystem mutation authority.

```text
filesystem facts / adapter facts
-> deterministic classifications
-> deterministic rule + protected-category policy
-> optional bounded AI enrichment of already-eligible quarantine candidates
-> authoritative observation revalidation
-> conservative conflict resolution
-> selected user risk threshold
-> dry-run preview
-> explicit interactive approval
-> reversible quarantine execution
-> durable action state
-> recover or restore
```

The intended invariant is:

> AI may broaden understanding and make a plan more conservative; deterministic policy controls what can actually happen.

AI-generated output is untrusted input. It cannot create a cleanup candidate, lower deterministic risk, remove a policy block, convert `report_only` or `tool_native` work into raw filesystem mutation, authorize permanent deletion, suppress blocked-candidate reporting, or directly invoke arbitrary shell commands.

The planner evaluates AI only for unique paths that deterministic rules have already made eligible for reversible quarantine. Static policy is applied first. The AI result may:

- recommend `ignore` or `review`, which blocks the candidate
- raise effective risk, which is checked again against the selected user threshold
- recommend `quarantine`; this only preserves the existing deterministic quarantine action when confidence and policy still allow it

Low-confidence recommendations are review-only. Provider failure or stale observation binding fails the AI-enriched plan before candidate/evidence persistence rather than synthesizing a fallback recommendation.

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

The runtime deliberately accepts **numeric loopback addresses only** (`127.0.0.0/8` or `::1`). It performs no DNS resolution, carries no credentials, follows no redirects, and rejects transfer-encoded responses. Remote HTTP/HTTPS gateways are not silently enabled.

Path modes:

- `full` — exact indexed path; default for the explicit `analyze` command
- `basename` — basename only
- `redacted` — removes user/project prefixes while preserving selected structures such as `.gradle/caches`, `.cache`, `DerivedData`, `node_modules`, and `/nix/store`; default for AI-enriched planning

The request contains metadata only: path according to the selected privacy mode, size/scan-relative age, bounded deterministic labels, policy context, stable candidate identity, and an observation digest over the exact post-privacy facts sent to inference. File contents are not sent.

Every accepted response must match the request ID and observation digest, pass strict JSON/schema validation, use the restricted action vocabulary (`ignore`, `review`, `quarantine`), and carry matching provenance. Model-controlled text and list sizes are bounded, and terminal control/bidirectional-control characters are escaped before display.

## AI-enriched planning

Use `--ai-endpoint` on `plan` to enrich a bounded number of already-eligible deterministic quarantine paths:

```bash
tidyfs plan \
  --safe \
  --ai-endpoint http://127.0.0.1:8000 \
  --ai-path-mode redacted \
  --ai-limit 10
```

Planning never trusts stored evidence as a freshness authority. For every live recommendation, tidyfs:

1. builds the canonical observation from authoritative scan/index facts;
2. sends that observation to the local gateway;
3. validates response schema, request ID, provenance, and digest correlation;
4. re-queries the authoritative scan/index facts after inference;
5. re-derives the canonical observation digest and rejects any mismatch;
6. applies deterministic policy first, then one-way conservative AI conflict resolution;
7. persists cleanup candidates and advisory AI evidence together only after every selected AI call succeeds.

Persisted AI evidence contains rationale, confidence, provenance, path privacy mode, risk context, candidate identity, and observation digest. `tidyfs explain <path>` shows the latest stored evidence and re-derives its freshness from current authoritative scan facts. A stale recommendation remains audit evidence but is clearly marked `stale` and is not planning authority.

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
cargo run -- plan --safe --ai-endpoint http://127.0.0.1:8000 --ai-path-mode redacted
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
- allow AI to create new executable candidates
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

They are visible in plans and dry-runs, but are not executable yet and are never promoted to filesystem mutation by AI advice.

## Planning model

```text
scan facts
-> deterministic classifications
-> YAML cleanup rules
-> adapter inspection
-> deterministic policy validation
-> optional bounded AI enrichment of already-eligible quarantine candidates
-> post-inference observation revalidation
-> one-way conservative conflict policy
-> cleanup candidates / blocked candidates + advisory evidence
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
