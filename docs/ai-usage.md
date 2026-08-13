# AI Usage

AI is optional and non-authoritative. The deterministic core remains useful with AI disabled.

## Core rule

```text
scanner observes authoritative facts
rules and protected-category policy determine eligibility
AI may analyze, conservatively enrich, or recommend among already-valid candidates
deterministic code revalidates freshness, risk, and byte totals
executor acts only after explicit approval through the existing reversible path
```

## AI can do today

- analyze bounded metadata for classified scan candidates;
- classify ambiguous usage for explanation;
- recommend `ignore`, `review`, or `quarantine` for an already-observed candidate;
- provide confidence, rationale, caveats, risk, and provider/model provenance;
- make an otherwise eligible deterministic candidate more conservative;
- contribute advisory evidence displayed by `explain`;
- recommend a read-only subset of existing eligible persisted plan candidates for a numeric reclaim target.

## AI cannot do

- invent an executable cleanup candidate;
- invent or execute shell commands;
- lower deterministic risk;
- override protected or blocked candidates;
- promote `tool_native` or `report_only` work into filesystem mutation;
- broaden the selected root;
- inspect arbitrary file contents through the current contract;
- determine authoritative reclaim-byte totals;
- persist a goal recommendation as executable authority;
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

## Goal-oriented cleanup recommendations

After building and inspecting a deterministic plan, ask the local gateway to recommend a subset for a numeric reclaim target:

```bash
tidyfs plan --safe

tidyfs recommend \
  --endpoint http://127.0.0.1:8000 \
  --target-bytes 21474836480 \
  --risk low
```

`recommend` is read-only. It reads the persisted `cleanup_candidates` for the selected scan and supplies only candidates that are all of the following:

- currently unblocked;
- within the selected root;
- within the selected risk threshold;
- reversible;
- `quarantine` actions rather than `report_only`, `tool_native`, or other actions.

If multiple deterministic rules match the same filesystem path, `recommend` supplies one canonical candidate for that path so reclaimable bytes are not counted more than once.

The gateway receives candidate IDs and bounded facts through `POST /v1/recommend`. It may return only a subset of those IDs plus rationale/caveats. It does **not** return authoritative byte totals.

After inference, tidyfs:

1. validates the response contract, request ID, provenance request ID, and plan/goal digest;
2. rejects duplicate or unknown selected candidate IDs;
3. re-reads the eligible persisted plan using the same root/risk/limit constraints;
4. requires the exact supplied candidate facts to remain unchanged;
5. re-derives the plan/goal digest;
6. calculates selected reclaim bytes itself;
7. calculates `target_met` itself;
8. prints the recommendation without writing candidate or action authority.

A recommendation therefore does not alter what `clean` can execute. Cleanup remains a separate deterministic/interactive flow.

Path disclosure defaults to `redacted` for `recommend`. The current first slice accepts a numeric `--target-bytes`; free-form natural-language goal parsing is intentionally deferred.

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

Candidate-level analysis requests are bound to canonical post-privacy observation facts using SHA-256.

Goal-level recommendations are bound to the selected scan, target, risk/root constraints, and exact eligible candidate facts using an opaque plan/goal freshness digest. That digest is correlation evidence, not an authentication capability. The authoritative safety check remains the post-inference re-read and exact fact comparison.

```text
eligible persisted plan
  -> bounded canonical candidate set + goal
  -> goal/plan digest
  -> AI request/response
  -> authoritative persisted-plan re-read
  -> exact facts + digest comparison
  -> deterministic byte calculation
  -> read-only recommendation output
```

Stored AI evidence is not trusted as freshness authority. `tidyfs explain <path>` re-derives the current candidate observation and marks stored candidate-level evidence `fresh` or `stale`.

## Path privacy modes

AI receives no arbitrary file contents. Paths are explicitly transformed before inference:

- `full` — exact indexed path;
- `basename` — basename only;
- `redacted` — remove user/project-identifying prefixes while preserving useful ecosystem structures.

Examples of structure intentionally preserved by redaction include `.cache`, `.gradle/caches`, `DerivedData`, `node_modules`, and `/nix/store`.

Defaults:

- explicit read-only `analyze`: `full` is acceptable for a deliberately selected local loopback model;
- AI-enriched planning: `redacted` by default;
- goal-oriented `recommend`: `redacted` by default.

## Gateway restrictions

The current runtime is local-only by design:

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

Structured routes are:

- `POST /v1/analyze` for candidate-level advisory analysis;
- `POST /v1/recommend` for goal-oriented selection among supplied eligible plan candidate IDs.

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
- mismatched/stale observation or goal-plan digest;
- unknown or duplicate goal-selected candidate IDs;
- changed eligible plan facts during inference;
- oversized response;
- unsupported HTTP semantics.

For AI-enriched planning, failure leaves no newly accepted enriched recommendation for the affected planning attempt. For `recommend`, failure produces no accepted read-only goal recommendation and makes no candidate/action changes.

## Explainability

Persisted candidate-level advisory evidence may include:

- classification and confidence;
- rationale and caveats;
- AI risk and recommended action;
- provider, model, and request ID;
- stable candidate identity;
- path privacy mode;
- selected risk context;
- observation digest;
- creation time and current freshness state.

Goal-oriented recommendations are intentionally not persisted as executable authority in the first slice. Their terminal output includes selected candidate IDs/paths, deterministic reclaim totals, target satisfaction, model provenance, rationale, and caveats.

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

## Deferred advisory work

The accepted post-`v0.6.1` decision is recorded in ADR 0001 and implemented first as structured numeric-target recommendation over an existing plan.

AI-generated disabled rule proposals, free-form goal parsing, non-loopback transport, and tool-native cleanup execution remain separate future decisions.