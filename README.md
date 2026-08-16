# tidyfs

`tidyfs` is an AI-enabled, conservative filesystem cleanup and disk-usage intelligence CLI.

The project combines deterministic filesystem facts, policy-gated cleanup planning, optional AI-assisted analysis, and reversible execution. AI may classify, explain, enrich, or recommend among candidates that `tidyfs` already considers; deterministic code remains authoritative for eligibility, risk, reclaim-byte accounting, approval, mutation, and recovery.

```bash
tidyfs scan ~ --jobs 8
tidyfs top --depth 2 --limit 20
tidyfs plan --safe
tidyfs clean --dry-run
tidyfs clean --safe --interactive

# Optional local AI analysis / planning.
tidyfs analyze --endpoint http://127.0.0.1:8000 --limit 10
tidyfs plan --safe --ai-endpoint http://127.0.0.1:8000 --ai-path-mode redacted

# Read-only goal-oriented advice over an existing deterministic plan.
tidyfs recommend \
  --endpoint http://127.0.0.1:8000 \
  --target-bytes 21474836480 \
  --risk low
```

## Current capabilities

Implemented:

- Rust CLI
- recursive and parallel subtree scanning
- SQLite index and directory aggregation
- deterministic classification
- YAML cleanup rules and protected-category policy
- canonical non-overlapping cleanup hierarchy for totals, dry-run, and execution
- persisted cleanup plans and blocked-candidate reporting
- `clean --dry-run`
- explicit `--safe --interactive` reversible quarantine execution
- durable cleanup and restore action states
- interrupted-action reconciliation with `recover`
- action logging and `actions` listing
- `restore`
- read-only tool-native adapters for systemd journal, Docker, Podman, Nix, pnpm, pip, uv, and Go
- bounded read-only AI analysis through a numeric-loopback gateway
- canonical AI observation binding and strict provider-neutral JSON transport
- explicit AI path privacy modes (`full`, `basename`, `redacted`)
- conservative AI enrichment of already-eligible deterministic plan candidates
- authoritative post-inference observation revalidation and stale-recommendation rejection
- persisted advisory AI rationale, confidence, provenance, and observation evidence
- `explain` with freshness revalidation of stored AI evidence
- read-only goal-oriented `recommend` over canonical existing plan candidates
- deterministic goal-plan freshness binding and reclaim-byte calculation
- optional versioned live-model quality evaluation harness
- standalone Nix flake package/app/check/dev-shell contract
- tag-driven Linux release packaging with deterministic archive contents and SHA-256 verification
- coverage-guided fuzzing of bounded AI trust boundaries
- no permanent deletion

Planned or intentionally deferred:

- semantic explanation improvements
- AI-generated candidate rules or policy suggestions, if there is a demonstrated workflow need
- hardened non-loopback gateway transport, if remote inference becomes necessary
- executable tool-native adapter cleanup, which requires a separate command-authority and recovery design
- permanent deletion

## Authority and trust model

The high-level execution path is:

```text
filesystem facts / adapter facts
-> deterministic classifications
-> deterministic rules + protected-category policy
-> exact-path and ancestor/descendant hierarchy canonicalization
-> optional bounded AI enrichment of already-eligible quarantine candidates
-> authoritative post-inference revalidation
-> conservative conflict resolution
-> selected user risk threshold
-> persisted deterministic plan
-> dry-run preview
-> explicit interactive approval
-> source identity + filesystem checks
-> reversible quarantine
-> durable action state
-> recover or restore
```

Goal-oriented recommendation is a read-only branch over the persisted plan:

```text
persisted deterministic plan
-> canonical non-overlapping eligible candidate set
-> bounded target / risk / root constraints
-> optional loopback AI selection of supplied candidate IDs
-> request + plan-digest validation
-> post-inference plan re-read
-> deterministic selected-byte calculation
-> read-only recommendation output
```

The intended invariant is:

> AI may broaden understanding or make a plan more conservative; deterministic policy controls what can actually happen.

AI output is untrusted input. It cannot:

- create an executable cleanup candidate
- lower deterministic risk
- remove a policy block
- make a blocked ancestor bypass a protected or stricter descendant
- convert `report_only` or `tool_native` work into raw filesystem mutation
- broaden the selected scan root
- authoritatively determine reclaim-byte totals
- persist a goal recommendation as cleanup authority
- authorize permanent deletion
- bypass `clean --safe --interactive`
- invoke arbitrary shell commands
- directly call filesystem mutation primitives

See [AI architecture](docs/ai-architecture.md), [cleanup hierarchy](docs/cleanup-hierarchy.md), and the [threat model](docs/threat-model.md) for the detailed boundaries.

## Local AI analysis

`tidyfs analyze` sends bounded metadata from an existing completed scan to an explicitly selected local gateway implementing `POST /v1/analyze`.

```bash
# Full paths are acceptable for an explicitly selected loopback model.
tidyfs analyze \
  --endpoint http://127.0.0.1:8000 \
  --limit 10

# Reduce path disclosure while preserving useful ecosystem structure.
tidyfs analyze \
  --endpoint http://127.0.0.1:8000 \
  --root ~/src \
  --path-mode redacted \
  --limit 5
```

The runtime accepts **numeric loopback addresses only** (`127.0.0.0/8` or `::1`). It performs no DNS resolution, carries no credentials, follows no redirects, rejects transfer-encoded responses, and bounds request/response sizes and timeouts.

Path modes:

- `full` — exact indexed path; default for explicit `analyze`
- `basename` — basename only
- `redacted` — removes user/project prefixes while preserving selected structures such as `.gradle/caches`, `.cache`, `DerivedData`, `node_modules`, and `/nix/store`; default for AI-assisted planning and goal recommendation

The inference contract contains metadata only: transformed path, size/scan-relative age, bounded deterministic labels, policy context, stable candidate identity, and correlation/freshness bindings. Arbitrary file contents are not sent.

## AI-enriched planning

Use `--ai-endpoint` on `plan` to enrich a bounded number of already-eligible deterministic quarantine paths:

```bash
tidyfs plan \
  --safe \
  --ai-endpoint http://127.0.0.1:8000 \
  --ai-path-mode redacted \
  --ai-limit 10
```

For every live candidate-level recommendation, `tidyfs`:

1. builds the canonical observation from authoritative scan/index facts;
2. sends the bounded observation to the local gateway;
3. validates schema, request ID, provenance, action vocabulary, and digest correlation;
4. re-queries authoritative facts after inference;
5. re-derives the canonical observation and rejects stale results;
6. applies deterministic policy before one-way conservative AI conflict resolution;
7. persists cleanup candidates and advisory AI evidence only after the selected analysis succeeds.

Stored AI evidence remains audit/explanation evidence, not a freshness authority. `tidyfs explain <path>` re-derives whether that evidence is still current.

## Goal-oriented recommendations

`recommend` answers a bounded cleanup question over an already-persisted deterministic plan without changing that plan:

```bash
tidyfs plan --safe

tidyfs recommend \
  --endpoint http://127.0.0.1:8000 \
  --target-bytes 21474836480 \
  --risk low \
  --path-mode redacted
```

Before inference, `tidyfs` exposes only canonical, pairwise non-overlapping, unblocked, reversible quarantine candidates inside the requested root/risk boundary. The gateway may select only candidate IDs supplied in the request. After inference, the persisted plan is re-read and revalidated; unknown, duplicate, stale, or otherwise invalid selections fail closed.

The model does not author reclaim totals. `tidyfs` calculates selected bytes and `target_met` from the revalidated candidate set. The result is recommendation output only; cleanup still requires a separate deterministic `clean` flow.

## Adapter inspection

Adapters inspect tool-owned cleanup domains without giving `tidyfs` external-tool mutation authority:

```bash
tidyfs adapters
tidyfs plan --risk medium --include-adapters
tidyfs clean --dry-run --risk medium
```

Adapter candidates use:

```text
action_type = tool_native
```

They may appear in plans and dry-runs but are not executable by the quarantine executor. AI cannot promote them into raw filesystem mutation.

See [adapter design](docs/adapters.md).

## Safety and recovery

Real filesystem mutation is intentionally narrow:

```bash
tidyfs clean --safe --interactive
```

Current execution:

- quarantines only reversible filesystem candidates
- does not permanently delete
- rejects symlink substitution
- checks scanned source device/inode identity on supported platforms
- requires same-filesystem quarantine rather than copy/delete fallback
- verifies payload identity around the move
- persists transitional action state before filesystem mutation
- serializes cleanup/restore/recovery mutation flows

After interruption, reconcile recorded action state against the observed filesystem without moving or overwriting files:

```bash
tidyfs recover --all
```

Restore quarantined data explicitly:

```bash
tidyfs restore --latest
# or
tidyfs restore --action <id>
```

See [recoverable actions](docs/recovery.md) for the state machine, reconciliation matrix, residual TOCTOU limitation, and operator procedure.

## Install and run

With Cargo:

```bash
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
```

With Nix:

```bash
nix build .#tidyfs
nix run .# -- --help
nix flake check
nix develop
```

The flake exports named/default packages, a default app, deterministic checks, a development shell, and formatter. It is standalone and does not depend on Dubnium host configuration. See [Nix packaging](docs/nix.md).

Default local state:

```text
~/.local/share/tidyfs/tidyfs.db
~/.local/share/tidyfs/quarantine/
```

## Parallel scanning

The scanner parallelizes immediate child subtrees while keeping SQLite writes serialized:

```bash
tidyfs scan ~ --jobs 8
```

This keeps aggregation deterministic and avoids shared mutable filesystem state while removing the obvious single-threaded traversal bottleneck. See [parallel scanning](docs/parallel-scanning.md).

## AI quality evaluations

Correctness and safety tests do not depend on a live model. Model output is treated as untrusted structured input and deterministic validation remains the merge/release evidence for authority boundaries.

An optional evaluation layer measures advisory recommendation quality and drift over a committed, redacted fixture corpus:

```bash
TIDYFS_EVAL_ENDPOINT=http://127.0.0.1:8000 mise run eval-live
```

The harness records machine-readable and human-readable scorecards with runtime/provider/model/request provenance. Semantic-quality regressions are reported without becoming cleanup authority or ordinary PR correctness gates; provider and contract failures remain distinct error classes.

See [live-model evaluations](docs/evaluations.md).

## Development and quality gates

Canonical local checks:

```bash
mise run static-analysis
mise run test
mise run package
mise run ci
mise run fuzz-build
mise run audit
nix flake check
```

The deterministic CI quality surface includes:

- **Rust quality** — formatting, Clippy with warnings denied, locked tests, package verification
- **Dependency audit** — RustSec
- **Fuzz harnesses** — compile every maintained fuzz target using pinned nightly/cargo-fuzz tooling
- **Nix flake** — standalone package/check/app validation

Linux CI, fuzzing, and release jobs use the exact `runner-tidyfs` JIT runner route. Coverage-guided fuzz campaigns remain scheduled/manual rather than ordinary PR runtime work.

## Distribution

`Cargo.toml` is the package/version source of truth. The Linux release workflow verifies the tag/version relationship, builds with locked dependencies, produces deterministic archive contents, generates a SHA-256 checksum, and separates read-only build work from release publication authority.

Signing/SLSA-style provenance, additional binary targets, and any crates.io publication policy remain separate distribution decisions.

## Documentation

Key design documents:

- [Architecture](docs/architecture.md)
- [AI architecture](docs/ai-architecture.md)
- [AI usage](docs/ai-usage.md)
- [Threat model](docs/threat-model.md)
- [Cleanup hierarchy](docs/cleanup-hierarchy.md)
- [Recovery](docs/recovery.md)
- [Adapters](docs/adapters.md)
- [Nix packaging](docs/nix.md)
- [Live-model evaluations](docs/evaluations.md)
