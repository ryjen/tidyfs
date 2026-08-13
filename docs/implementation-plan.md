# Implementation and Delivery Roadmap

`tidyfs` is developed in vertical slices that preserve a deterministic, useful core while adding reversible execution, tool-native inspection, and optional AI advisory behavior around that core.

This document records **delivered milestones and current sequencing**. Detailed security and recovery invariants live in `threat-model.md`, `safety-model.md`, and `recovery.md`.

## Milestone 1: Analyzer spine — complete

Goal: replace basic `du` for common developer-machine inspection.

Delivered:

- Rust CLI
- recursive `scan`
- SQLite index
- directory aggregation
- `top`
- permission-error handling
- repeatable scan metadata

## Milestone 2: Deterministic classification — complete

Goal: identify known filesystem categories without requiring AI.

Delivered:

- deterministic path classification
- classification persistence
- protected/sensitive categories
- `classify`
- `explain`

## Milestone 3: Rule engine and planner — complete

Goal: generate deterministic cleanup proposals under explicit policy.

Delivered:

- YAML rules
- path/glob matchers
- risk tiers
- protected-category and action policy
- `plan`
- blocked candidate reporting
- deterministic risk-threshold handling

## Milestone 4: Exact dry-run — complete

Goal: preview cleanup without touching files.

Delivered:

- `clean --dry-run`
- exact candidate/action preview
- risk-aware candidate selection
- no filesystem mutation

## Milestone 5: Reversible execution and recovery — complete

Goal: permit real cleanup without permanent deletion and remain recoverable across failures.

Delivered:

- explicit `--safe --interactive` execution gate
- same-filesystem quarantine
- durable action states
- payload identity verification
- source device/inode and symlink/path-substitution checks
- serialized mutation lock
- explicit interrupted-action detection
- `recover`
- atomic no-overwrite `restore` on supported platforms
- failure-injection integration coverage
- no permanent deletion

The residual pathname TOCTOU limitation is documented rather than represented as eliminated.

## Milestone 6: Tool-native inspection — complete

Goal: reason about tool-owned data without treating it as raw filesystem cleanup.

Delivered read-only adapters:

- systemd journal
- Docker
- Podman
- Nix
- pnpm
- pip
- uv
- Go

Adapter candidates remain `tool_native` and non-executable. `tidyfs` shows preview/reclaim information and suggested native commands but does not execute them.

## Milestone 6.1: Parallel scanning and packaging/release baseline — complete

Delivered:

- parallel immediate-subtree scanning with a single SQLite writer
- Cargo package verification
- dual MIT/Apache licensing artifacts
- formatting, Clippy, tests, package verification, and RustSec CI gates
- deterministic Linux release bundle construction
- tag/version binding
- isolated write-scoped GitHub Release publication
- exact-tag manual recovery entrypoint for intentional existing tags
- independently validated `v0.6.1` Linux artifact and checksum

## Milestone 7: AI advisory analysis and conservative planning — complete

The original roadmap described an “optional AI explainer.” The delivered architecture is intentionally stronger and narrower: AI is a structured advisory layer over authoritative filesystem facts, while deterministic policy remains the mutation authority.

Delivered:

- versioned `AiCleanupProposal` contract
- provider-neutral `AiAnalysisProvider` port
- canonical bounded observation encoding and SHA-256 freshness binding
- numeric-loopback `/v1/analyze` gateway adapter
- strict request/response correlation and bounded transport
- `full`, `basename`, and `redacted` path privacy modes
- read-only `tidyfs analyze`
- conservative AI enrichment of already-eligible deterministic quarantine candidates
- post-inference authoritative observation reconstruction
- stale recommendation rejection
- effective-risk revalidation
- persisted advisory rationale/confidence/provenance/digest evidence
- `explain` freshness reporting
- fail-closed provider and schema behavior
- no AI mutation primitive, arbitrary shell, adapter execution, or permanent deletion

The authority chain is:

```text
scan/index facts
  -> deterministic classification/rules/policy
  -> optional bounded AI advice
  -> strict validation
  -> authoritative observation revalidation
  -> conservative conflict resolution
  -> selected user risk threshold
  -> dry-run / explicit approval
  -> reversible quarantine
```

AI may preserve or reduce existing deterministic authority; it may not increase it.

## Milestone 8: Goal-oriented advisory planning — first slice

QART #51 selected goal-oriented advisory planning over existing deterministic candidates as the post-`v0.6.1` product direction. ADR 0001 records the authority boundary.

The first slice adds a read-only recommendation workflow:

```text
persisted deterministic plan
  -> unblocked reversible quarantine candidates
  -> canonicalize one filesystem payload using its highest deterministic risk
  -> root/risk filter + bounded reclaim target
  -> /v1/recommend over explicit candidate IDs
  -> strict request/provenance/plan binding validation
  -> authoritative plan re-read after inference
  -> selected-ID subset validation
  -> tidyfs-computed reclaim bytes and target_met
  -> read-only output
```

The model may choose only candidate IDs supplied by `tidyfs`. It cannot create candidates, lower deterministic risk, remove blocks, broaden the root, determine authoritative byte totals, persist executable authority, or trigger cleanup.

The first slice intentionally uses a numeric reclaim target rather than a free-form natural-language goal. Recommendation output remains separate from `clean` and does not modify cleanup/action state.

## Current release baseline

`v0.6.1` is the current validated public Linux release baseline.

The release record includes:

- a green `main` tag point;
- deterministic archive contents;
- SHA-256 checksum verification;
- successful GitHub Release publication;
- independent retained-artifact verification;
- binary smoke tests.

## Deferred decisions

Do not treat the old “AI explainer / Ollama/OpenAI-compatible client / ask command” section as an active implementation plan. The implemented provider boundary deliberately avoids coupling the core to a generic chat API or hosted-provider SDK.

Explicitly deferred until separately justified:

- natural-language/free-form goal parsing;
- non-loopback/remote model transport;
- AI-generated enabled rules;
- tool-native cleanup execution;
- permanent deletion;
- broader model/tool authority.

## Delivery rule for future milestones

Prefer the smallest vertical slice that produces an observable user outcome while preserving current invariants:

```text
contract / decision
  -> read-only behavior
  -> deterministic validation
  -> failure-path tests
  -> integration with existing planner
  -> only then consider new mutation authority
```

Any new mutation, command-execution, remote-trust, or credential boundary requires focused design/security review rather than being smuggled into an otherwise advisory feature.
