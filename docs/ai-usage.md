# AI Usage

AI is optional and non-authoritative. The deterministic core remains useful with AI disabled.

## Core rule

```text
scanner observes authoritative facts
rules and protected-category policy determine eligibility
AI may analyze or conservatively enrich already-valid candidates
deterministic code revalidates freshness and risk
executor acts only after explicit approval through the existing reversible path
```

## AI can do today

- analyze bounded metadata for classified scan candidates;
- classify ambiguous usage for explanation;
- recommend `ignore`, `review`, or `quarantine` for an already-observed candidate;
- provide confidence, rationale, caveats, risk, and provider/model provenance;
- make an otherwise eligible deterministic candidate more conservative;
- contribute advisory evidence displayed by `explain`.

## AI cannot do

- invent an executable cleanup candidate;
- invent or execute shell commands;
- lower deterministic risk;
- override protected or blocked candidates;
- promote `tool_native` or `report_only` work into filesystem mutation;
- broaden the selected root;
- inspect arbitrary file contents through the current contract;
- perform quarantine, restore, recovery, or deletion;
- authorize permanent deletion;
- bypass `clean --safe --interactive` or source-identity checks.

## Read-only analysis

Use an explicitly configured local numeric-loopback gateway:

```bash
tidyfs analyze \
  --endpoint http://127.0.0.1:8000 \
  --limit 10
```

Useful bounds include:

```bash
tidyfs analyze \
  --endpoint http://127.0.0.1:8000 \
  --root ~/src \
  --path-mode redacted \
  --limit 5
```

`analyze` reads already-indexed facts and renders recommendations. It does not persist cleanup candidates or mutate the filesystem.

## AI-enriched deterministic planning

AI may conservatively enrich an existing deterministic plan:

```bash
tidyfs plan \
  --safe \
  --ai-endpoint http://127.0.0.1:8000 \
  --ai-path-mode redacted \
  --ai-limit 10
```

The planner sends AI only paths that deterministic rules and protected-category policy have already made eligible for reversible quarantine.

After inference, tidyfs:

1. validates the response schema and request/provenance correlation;
2. re-queries authoritative scan/index facts;
3. re-derives the observation digest;
4. rejects stale or mismatched advice;
5. applies one-way conservative conflict policy;
6. reapplies the selected risk threshold;
7. persists plan candidates and advisory evidence only after the selected AI calls complete successfully.

The model cannot make a previously ineligible candidate executable.

## Conservative conflict policy

For an already-eligible reversible quarantine candidate:

- deterministic blocks always win;
- effective risk is `max(deterministic risk, AI risk)`;
- `ignore` blocks the candidate;
- `review` blocks the candidate pending human review;
- low-confidence advice is review-only;
- `quarantine` merely preserves the existing deterministic action if every other gate still permits it.

AI therefore has **one-way authority**: it may preserve or reduce deterministic authority, never increase it.

## Freshness and candidate binding

Each request is bound to canonical post-privacy observation facts using SHA-256.

```text
candidate facts
  -> canonical observation
  -> observation digest
  -> AI request/response
  -> authoritative fact reconstruction
  -> digest comparison
  -> accept or reject recommendation
```

Stored AI evidence is not trusted as freshness authority. `tidyfs explain <path>` re-derives the current observation and marks stored evidence `fresh` or `stale`.

## Path privacy modes

AI receives no arbitrary file contents. Paths are explicitly transformed before inference:

- `full` — exact indexed path;
- `basename` — basename only;
- `redacted` — remove user/project-identifying prefixes while preserving useful ecosystem structures.

Examples of structure intentionally preserved by redaction include `.cache`, `.gradle/caches`, `DerivedData`, `node_modules`, and `/nix/store`.

Defaults:

- explicit read-only `analyze`: `full` is acceptable for a deliberately selected local loopback model;
- AI-enriched planning: `redacted` by default.

## Gateway restrictions

The current v1 runtime is local-only by design:

- numeric loopback addresses only (`127.0.0.0/8` or `::1`);
- explicit non-zero port;
- no DNS resolution;
- no redirects;
- no credentials;
- no generic chat/messages API;
- no MCP/tool invocation;
- bounded requests, headers, and responses;
- connect/read/write timeouts;
- strict `application/json` response handling;
- no transfer encoding/chunked response support.

Remote inference is intentionally not enabled by a configuration switch. Adding it requires a separate design covering TLS, authentication/capability identity, endpoint policy, credentials, path disclosure, and operational controls.

## Failure behavior

AI failures never synthesize a cleanup authorization.

These conditions fail closed:

- provider unavailable or timeout;
- malformed JSON;
- unknown contract/schema version;
- unknown response fields;
- unsupported action;
- mismatched request ID;
- mismatched provenance request ID;
- mismatched/stale observation digest;
- oversized response;
- unsupported HTTP semantics;
- changed candidate facts during inference.

For AI-enriched planning, failure leaves no newly accepted enriched recommendation for the affected planning attempt.

## Explainability

Persisted advisory evidence may include:

- classification and confidence;
- rationale and caveats;
- AI risk and recommended action;
- provider, model, and request ID;
- stable candidate identity;
- path privacy mode;
- selected risk context;
- observation digest;
- creation time and current freshness state.

Model-controlled terminal and bidirectional-control characters are escaped before rendering.

## Deterministic-only operation

AI is not required for normal use:

```bash
tidyfs scan ~
tidyfs classify --summary
tidyfs plan --safe
tidyfs clean --dry-run
tidyfs clean --safe --interactive
```

The same deterministic policy, risk, quarantine, recovery, and restore model remains authoritative whether AI is configured or not.

## Planned advisory work

The next product direction is not a generic chat mode. QART #51 evaluates a structured goal-oriented advisory layer over an existing deterministic plan—for example, recommending a validated subset of existing candidates to meet a reclaim target without creating new execution authority.

AI-generated disabled rule proposals, non-loopback transport, and tool-native cleanup execution remain separate future decisions.